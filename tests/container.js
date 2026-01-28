const BASE_URL = 'http://localhost:8070';

// Test container management
console.log('=== Container Management Tests ===\n');

// First create a volume for the container
fetch(`${BASE_URL}/volumes`, { method: 'POST' })
  .then(r => r.json())
  .then(vol => {
    console.log('✓ Created volume:', vol.id);

    // Create a container
    return fetch(`${BASE_URL}/containers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        internal_id: 'test-server-001',
        volume_id: vol.id,
        startup_command: '/bin/sh -c "echo Starting server && sleep infinity"',
        image: 'alpine:latest',
        install_script: '#!/bin/sh\necho "Running install script"\napk add --no-cache curl\necho "Install complete"'
      })
    }).then(r => r.json()).then(container => {
      console.log('✓ Container creation started:', container);
      return { vol, container };
    });
  })
  .then(({ vol, container }) => {
    // Wait a bit for installation to progress
    console.log('\n⏳ Waiting for installation to complete...\n');
    return new Promise(resolve => {
      setTimeout(() => resolve({ vol, container }), 8000);
    });
  })
  .then(({ vol, container }) => {
    // Get container status
    return fetch(`${BASE_URL}/containers/${container.internal_id}`)
      .then(r => r.json())
      .then(status => {
        console.log('✓ Container status:', {
          internal_id: status.internal_id,
          install_state: status.install_state,
          is_installing: status.is_installing,
          container_id: status.container_id
        });
        return { vol, container };
      });
  })
  .then(({ vol, container }) => {
    // List all containers
    return fetch(`${BASE_URL}/containers`)
      .then(r => r.json())
      .then(containers => {
        console.log('\n✓ All containers:', containers.length);
        containers.forEach(c => {
          console.log(`  - ${c.internal_id}: ${c.install_state}`);
        });
        return { vol, container };
      });
  })
  .then(({ vol, container }) => {
    // Create another container without install script
    return fetch(`${BASE_URL}/containers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        internal_id: 'test-server-002',
        volume_id: vol.id,
        startup_command: 'echo "Hello from container 2"',
        image: 'alpine:latest'
      })
    }).then(r => r.json()).then(container2 => {
      console.log('\n✓ Second container created:', container2.internal_id);
      return { vol, container, container2 };
    });
  })
  .then(({ vol, container, container2 }) => {
    // Wait and check both containers
    return new Promise(resolve => {
      setTimeout(() => {
        fetch(`${BASE_URL}/containers`)
          .then(r => r.json())
          .then(containers => {
            console.log('\n✓ Final container list:');
            containers.forEach(c => {
              console.log(`  - ${c.internal_id}:`);
              console.log(`    State: ${c.install_state}`);
              console.log(`    Installing: ${c.is_installing}`);
              console.log(`    Container ID: ${c.container_id || 'pending'}`);
            });
            resolve({ vol, container, container2 });
          });
      }, 5000);
    });
  })
  .then(({ vol, container, container2 }) => {
    console.log('\n=== Testing Advanced Lifecycle (Reinstall/Repair) ===\n');
    const id = container.internal_id; // test-server-001

    // 1. Check Detailed Status
    return fetch(`${BASE_URL}/containers/${id}/status`)
      .then(r => r.json())
      .then(status => {
        console.log('✓ Detailed status:', status);
        return { vol, container, container2 };
      });
  })
  .then(({ vol, container, container2 }) => {
    const id = container.internal_id;
    // 2. Validate
    return fetch(`${BASE_URL}/containers/${id}/validate`)
      .then(r => r.json())
      .then(valid => {
        console.log('✓ Validation result:', valid);
        return { vol, container, container2 };
      });
  })
  .then(({ vol, container, container2 }) => {
    const id = container.internal_id;
    console.log(`\n🔄 Reinstalling container ${id}...`);

    // 3. Reinstall
    return fetch(`${BASE_URL}/containers/${id}/reinstall`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        image: 'alpine:latest',
        install_script: '#!/bin/sh\necho "Reinstalling..."\nsleep 2\necho "Reinstall complete"'
      })
    })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Reinstall requested:', res);
        return { vol, container, container2 };
      });
  })
  .then(({ vol, container, container2 }) => {
    // Wait for reinstall to finish
    console.log('⏳ Waiting 10s for reinstall to complete...');
    return new Promise(resolve => setTimeout(() => resolve({ vol, container, container2 }), 10000));
  })
  .then(({ vol, container, container2 }) => {
    // Check status again
    return fetch(`${BASE_URL}/containers/${container.internal_id}/status`)
      .then(r => r.json())
      .then(status => {
        console.log('✓ Post-reinstall status:', status.install_state);
        return { vol, container, container2 };
      });
  })
  .then(({ vol, container, container2 }) => {
    console.log('\n🔧 Testing Repair Endpoint...');
    const id = container.internal_id;
    return fetch(`${BASE_URL}/containers/${id}/repair`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ image: 'alpine:latest' })
    })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Repair response:', res);
        return { vol, container, container2 };
      });
  })
  .then(({ vol, container, container2 }) => {
    console.log('\n=== Testing Power Actions (on container.js) ===\n');
    const id = container.internal_id;

    // 1. Restart
    console.log(`🔄 Restarting container ${id}...`);
    return fetch(`${BASE_URL}/containers/${id}/restart`, { method: 'POST' })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Restart response:', res);
        // Wait for restart
        return new Promise(resolve => setTimeout(resolve, 2000));
      })
      .then(() => ({ vol, container, container2 }));
  })
  .then(({ vol, container, container2 }) => {
    const id = container.internal_id;
    // 2. Kill
    console.log(`🛑 Killing container ${id}...`);
    return fetch(`${BASE_URL}/containers/${id}/kill`, { method: 'POST' })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Kill response:', res);
        return { vol, container, container2 };
      });
  })
  .then(({ vol, container, container2 }) => {
    const id = container.internal_id;
    // 3. Start
    console.log(`▶️ Starting container ${id}...`);
    return fetch(`${BASE_URL}/containers/${id}/start`, { method: 'POST' })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Start response:', res);
        return { vol, container, container2 };
      });
  })
  .catch(e => console.error('❌ Error:', e));
