//! Timer mostly done to track how much time internal things take
//! not a serious bit of code for now
//!
//! Last updated (mon-jan-26-2026)


use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct Timer {
    start_time: Arc<Mutex<Option<Instant>>>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            start_time: Arc::new(Mutex::new(None)),
            handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start(&self) {
        let mut start_time = self.start_time.lock().await;
        *start_time = Some(Instant::now());

        let start_time_clone = self.start_time.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                let time = start_time_clone.lock().await;
                if let Some(start) = *time {
                    let elapsed = start.elapsed();
                    tracing::trace!("Timer running: {}ms", elapsed.as_millis());
                }
            }
        });

        let mut handle_lock = self.handle.lock().await;
        *handle_lock = Some(handle);
    }

    pub async fn stop(&self) -> u128 {
        let mut handle_lock = self.handle.lock().await;
        if let Some(handle) = handle_lock.take() {
            handle.abort();
        }

        let start_time = self.start_time.lock().await;
        if let Some(start) = *start_time {
            start.elapsed().as_millis()
        } else {
            0
        }
    }

    /* Dead code
    pub async fn elapsed(&self) -> u128 {
        let start_time = self.start_time.lock().await;
        if let Some(start) = *start_time {
            start.elapsed().as_millis()
        } else {
            0
        }
    } */
}
