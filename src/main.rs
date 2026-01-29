mod daemon;
mod config;
mod servers;
mod filesystem;
mod router;
mod network;
mod container;
mod websocket;
mod auth;
mod cli;
mod remote;
mod billing;

use axum::routing::get;
use axum::Router;
use clap::Parser;
use daemon::timer::Timer;
use std::sync::Arc;
use tower_http::cors::{CorsLayer, Any};
use axum::middleware;

#[derive(Parser)]
#[command(name = "lightd")]
#[command(about = "Light daemon application", long_about = None)]
struct Cli {
    
    #[arg(long = "dev")]
    dev: bool,
    
    #[arg(long = "servers")]
    servers: Option<i64>,
    
    #[arg(long = "token")]
    token: Option<String>,
}

#[tokio::main]
async fn main() {
    let timer = Timer::new();
    timer.start().await;
    
    let cli = Cli::parse();

    if cli.dev {
        // Starts lightd in dev mode
        // Allowing lightd to send trace logs for not important things
        tracing_subscriber::fmt::init();
        run_system_mode(timer).await;
    } else if let Some(token_cmd) = cli.token {
        // Token management commands
        cli::token::handle_token_command(&token_cmd).await;
    } else if let Some(page) = cli.servers {
        // List servers, optionally paginated by 'page'
        servers::list::list_servers(page).await;
    } else if cli.servers.is_some() {
        // List all servers if flag is present without a page (defaults to page 1)
        servers::list::list_servers(1).await;
    } else {
        // Run main application without tracing warnings
        main_app(timer).await;
    }
}

async fn main_app(timer: Timer) {
    daemon::start::print_banner_async().await;
    let storage_result = daemon::start::check_storage().await;
    match storage_result {
        Ok(()) => {
            // Storage check passed, continue
        }
        Err(e) => {
            tracing::error!("Storage error! Please double check the config.json! {}", e);
            return;
        }
    }
    // Initialize volume handler
    let config = config::config::Config::load("config.json")
        .expect("Failed to load config");
    
    // Initialize billing tracker
    let billing_rates = billing::tracker::BillingRates {
        memory_per_gb_hour: config.monitoring.billing.memory_per_gb_hour,
        cpu_per_vcpu_hour: config.monitoring.billing.cpu_per_vcpu_hour,
        storage_per_gb_hour: config.monitoring.billing.storage_per_gb_hour,
        egress_per_gb: config.monitoring.billing.egress_per_gb,
    };
    
    let billing_tracker = Arc::new(billing::tracker::BillingTracker::new(
        billing_rates,
        config.monitoring.interval_ms,
    ).expect("Failed to initialize billing tracker"));
    
    // Start billing monitoring if enabled
    if config.monitoring.enabled {
        billing_tracker.clone().start_monitoring().await;
        tracing::info!("Billing monitoring started");
    }
    
    // Initialize token manager
    let tokens_db_path = format!("{}/tokens.db", config.storage.base_path);
    let token_manager = Arc::new(auth::tokens::TokenManager::new(&tokens_db_path)
        .expect("Failed to initialize token manager"));
    
    // Spawn token cleanup task (runs every 5 minutes)
    let token_manager_cleanup = token_manager.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
            if let Err(e) = token_manager_cleanup.cleanup_expired() {
                tracing::error!("Failed to cleanup expired tokens: {}", e);
            }
        }
    });
    
    let volume_handler = Arc::new(filesystem::handler::VolumeHandler::new(
        config.storage.volumes_path.clone()
    ));
    
    // Initialize network pool
    let network_db_path = format!("{}/network.db", config.storage.base_path);
    let network_pool = Arc::new(network::pool::NetworkPool::new(&network_db_path)
        .expect("Failed to initialize network pool"));
    
    // Initialize default ports (25565-25569) on first startup
    let default_ports = vec![
        ("0.0.0.0".to_string(), 25565, "tcp".to_string()),
        ("0.0.0.0".to_string(), 25566, "tcp".to_string()),
        ("0.0.0.0".to_string(), 25567, "tcp".to_string()),
        ("0.0.0.0".to_string(), 25568, "tcp".to_string()),
        ("0.0.0.0".to_string(), 25569, "tcp".to_string()),
    ];
    
    // Check if pool is empty and add default ports
    match network_pool.get_all_ports().await {
        Ok(ports) if ports.is_empty() => {
            tracing::info!("Initializing default port pool (25565-25569)");
            for (ip, port, protocol) in default_ports {
                if let Err(e) = network_pool.add_port(ip.clone(), port, Some(protocol.clone())).await {
                    tracing::error!("Failed to add default port {}: {}", port, e);
                }
            }
        }
        Ok(ports) => {
            tracing::info!("Network pool initialized with {} ports", ports.len());
        }
        Err(e) => {
            tracing::error!("Failed to check network pool: {}", e);
        }
    }
    
    // Initialize firewall manager
    let firewall_db_path = format!("{}/firewall.db", config.storage.base_path);
    let firewall_manager = Arc::new(network::firewall::FirewallManager::new(&firewall_db_path)
        .expect("Failed to initialize firewall manager"));
    
    // Initialize container manager
    let containers_db_path = format!("{}/containers.db", config.storage.base_path);
    let container_manager = Arc::new(container::manager::ContainerManager::new(&containers_db_path)
        .expect("Failed to initialize container manager"));
    
    // Initialize lifecycle manager with event channel
    let (lifecycle_manager, mut lifecycle_rx) = container::lifecycle::LifecycleManager::new(container_manager.clone())
        .expect("Failed to initialize lifecycle manager");
    let lifecycle_manager = Arc::new(lifecycle_manager);
    
    // Initialize power manager with event channel
    let (power_manager, mut power_rx) = container::power::PowerManager::new(container_manager.clone())
        .expect("Failed to initialize power manager");
    let power_manager = Arc::new(power_manager);
    
    // Initialize container updater with event channel
    let (container_updater, mut update_rx) = container::update::ContainerUpdater::new(container_manager.clone())
        .expect("Failed to initialize container updater");
    let _container_updater = Arc::new(container_updater);
    
    // Initialize network rebinder with event channel
    let (network_rebinder, mut network_rx) = container::network::NetworkRebinder::new(container_manager.clone())
        .expect("Failed to initialize network rebinder");
    let network_rebinder = Arc::new(network_rebinder);
    
    // Initialize WebSocket event hub
    let event_hub = Arc::new(websocket::EventHub::new());
    
    // Initialize console streamer
    let console_streamer = Arc::new(websocket::ConsoleStreamer::new(
        container_manager.clone(),
        event_hub.clone(),
    ).expect("Failed to initialize console streamer"));
    
    // Initialize stats collector
    let stats_collector = Arc::new(websocket::StatsCollector::new(
        container_manager.clone(),
        event_hub.clone(),
    ).expect("Failed to initialize stats collector"));
    
    // Initialize remote sync manager if enabled
    let remote_sync = if let Some(remote_config) = &config.remote {
        if remote_config.enabled {
            let sync_manager = Arc::new(remote::client::RemoteSyncManager::new(
                remote_config.url.clone(),
                remote_config.token.clone(),
            ));
            
            // Start health check loop
            sync_manager.start_health_check().await;
            
            tracing::info!("Remote sync enabled: {}", remote_config.url);
            Some(sync_manager)
        } else {
            None
        }
    } else {
        None
    };
    
    // Check Docker availability at startup
    match lifecycle_manager.check_docker().await {
        Ok(()) => {
           // println!("✓ Docker daemon is running and accessible");
           // We know alr!
        }
        Err(e) => {
            eprintln!("✗ Docker Error: {}", e);
            eprintln!("  Please ensure Docker Desktop is running and try again.");
            return;
        }
    }
    
    // Clone event_hub for lifecycle events
    let event_hub_lifecycle = event_hub.clone();
    let remote_sync_lifecycle = remote_sync.clone();
    
    // Spawn lifecycle event listener
    tokio::spawn(async move {
        while let Some(event) = lifecycle_rx.recv().await {
            if let container::lifecycle::LifecycleEvent::DockerConnected = event {
                // So many logs about the docker daemon
                // just end it from here and send nothing
            }else {
                tracing::info!("Container lifecycle event: {:?}", event);
            }
            
            // Send status updates to remote if enabled
            if let Some(ref sync) = remote_sync_lifecycle {
                match &event {
                    container::lifecycle::LifecycleEvent::Started(id) => {
                        sync.notify_status(id.clone(), "installing".to_string());
                    }
                    container::lifecycle::LifecycleEvent::CreatingContainer(id) => {
                        sync.notify_status(id.clone(), "installing".to_string());
                    }
                    container::lifecycle::LifecycleEvent::Ready(id) => {
                        sync.notify_status(id.clone(), "ready".to_string());
                    }
                    container::lifecycle::LifecycleEvent::Error(id, msg) => {
                        sync.notify_error(id.clone(), "failed".to_string(), Some(msg.clone()));
                    }
                    container::lifecycle::LifecycleEvent::ReinstallStarted(id) => {
                        sync.notify_status(id.clone(), "installing".to_string());
                    }
                    _ => {}
                }
            }
            
            // Broadcast relevant events to WebSocket clients
            match &event {
                container::lifecycle::LifecycleEvent::CreatingContainer(id) => {
                    websocket::notify_installing(&event_hub_lifecycle, id).await;
                }
                container::lifecycle::LifecycleEvent::Ready(id) => {
                    websocket::notify_installed(&event_hub_lifecycle, id).await;
                }
                container::lifecycle::LifecycleEvent::Error(id, msg) => {
                    event_hub_lifecycle.broadcast_daemon_message(id, &format!("Error: {}", msg)).await;
                }
                container::lifecycle::LifecycleEvent::ReinstallStarted(id) => {
                    websocket::notify_installing(&event_hub_lifecycle, id).await;
                }
                _ => {}
            }
        }
    });
    
    // Clone event_hub for power events
    let event_hub_power = event_hub.clone();
    
    // Spawn power event listener
    tokio::spawn(async move {
        while let Some(event) = power_rx.recv().await {
            tracing::info!("Container power event: {:?}", event);
            
            // Broadcast power events to WebSocket clients
            match &event {
                container::power::PowerEvent::Starting(id) => {
                    event_hub_power.broadcast_event(id, "starting").await;
                }
                container::power::PowerEvent::Started(id) => {
                    // Note: We don't broadcast 'running' here - that comes from pattern matching in logs
                    event_hub_power.broadcast_daemon_message(id, "Container started").await;
                }
                container::power::PowerEvent::Killing(id) => {
                    event_hub_power.broadcast_event(id, "stopping").await;
                }
                container::power::PowerEvent::Killed(id) => {
                    event_hub_power.broadcast_daemon_message(id, "Container stopped").await;
                }
                container::power::PowerEvent::Restarting(id) => {
                    event_hub_power.broadcast_event(id, "stopping").await;
                }
                container::power::PowerEvent::Restarted(id) => {
                    event_hub_power.broadcast_daemon_message(id, "Container restarted").await;
                }
                container::power::PowerEvent::Error(id, msg) => {
                    event_hub_power.broadcast_daemon_message(id, &format!("Power error: {}", msg)).await;
                }
            }
        }
    });
    
    // Spawn network rebinding event listener
    tokio::spawn(async move {
        while let Some(event) = network_rx.recv().await {
            tracing::info!("Container network event: {:?}", event);
        }
    });
    
    // Spawn container update event listener
    tokio::spawn(async move {
        while let Some(event) = update_rx.recv().await {
            tracing::info!("Container update event: {:?}", event);
        }
    });
    
    // Setup WebSocket state
    let ws_state = websocket::WebSocketState {
        manager: container_manager.clone(),
        power: power_manager.clone(),
        event_hub: event_hub.clone(),
        console_streamer,
        stats_collector,
        token_manager: token_manager.clone(),
    };
    
    // Setup routers
    let public_routes = router::public::public_router();
    let auth_routes = router::auth::auth_router(token_manager);
    let remote_routes = router::remote::remote_router();
    let firewall_routes = router::firewall::firewall_router(firewall_manager);
    let billing_routes = router::billing::billing_router(billing_tracker);
    
    // Create auth config for middleware
    let auth_config = Arc::new(auth::middleware::AuthConfig::from_config(&config));
    
    // Protected routes with auth middleware
    let filesystem_routes = router::filesystem::volume_router(volume_handler)
        .layer(middleware::from_fn_with_state(auth_config.clone(), auth::middleware::auth_middleware));
    let network_routes = router::network::network_router(network_pool.clone())
        .layer(middleware::from_fn_with_state(auth_config.clone(), auth::middleware::auth_middleware));
    let firewall_protected_routes = firewall_routes
        .layer(middleware::from_fn_with_state(auth_config.clone(), auth::middleware::auth_middleware));
    let billing_protected_routes = billing_routes
        .layer(middleware::from_fn_with_state(auth_config.clone(), auth::middleware::auth_middleware));
    let container_routes = router::container::container_router(container_manager, lifecycle_manager, power_manager, network_rebinder, network_pool)
        .layer(middleware::from_fn_with_state(auth_config.clone(), auth::middleware::auth_middleware));
    
    // WebSocket route
    let ws_routes = Router::new()
        .route("/ws/:id", get(websocket::ws_handler))
        .with_state(ws_state);
    
    // Combine routes with CORS
    let app = public_routes
        .merge(auth_routes)
        .merge(remote_routes)
        .merge(filesystem_routes)
        .merge(network_routes)
        .merge(firewall_protected_routes)
        .merge(billing_protected_routes)
        .merge(container_routes)
        .merge(ws_routes)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        );
    
    // Start server
    // as son as server starts we set startup time
    let elapsed = timer.stop().await;
    println!("Total startup time: {}ms\n", elapsed);
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await
        .expect("Failed to bind server");
    
   // println!("Server running on http://{}", addr);
    //println!("WebSocket endpoint: ws://{}/ws/<container_id>", addr);
    
    axum::serve(listener, app).await
        .expect("Server failed");
}
   

async fn run_system_mode(timer: Timer) {
    main_app(timer).await
}
