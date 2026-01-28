const BASE_URL = 'http://localhost:8070';

console.log('=== Full Integration Test ===\n');
console.log('Testing: Volume → Network → Container integration\n');

let testData = {};

// Step 1: Create a volume
fetch(`${BASE_URL}/volumes`, { method: 'POST' })
  .then(r => r.json())
  .then(vol => {
    console.log('✓ Step 1: Created volume:', vol.id);
    testData.volume = vol;
    
    // Create some files in the volume
    return fetch(`${BASE_URL}/volumes/${vol.id}/write`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ 
        filename: 'server.properties', 
        content: 'server-port=25565\nmax-players=20\n' 
      })
    }).then(r => r.json()).then(() => vol);
  })
  .then(vol => {
    console.log('✓ Step 2: Added server.properties to volume');
    
    // Step 2: Allocate network ports
    return Promise.all([
      fetch(`${BASE_URL}/network/ports`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ip: '0.0.0.0', port: 25565 })
      }).then(r => r.json()),
      
      fetch(`${BASE_URL}/network/ports`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ip: '0.0.0.0', port: 25575 })
      }).then(r => r.json())
    ]).then(ports => {
      console.log('✓ Step 3: Allocated network ports:');
      ports.forEach(p => console.log(`  - ${p.ip}:${p.port}`));
      testData.ports = ports;
      return vol;
    });
  })
  .then(vol => {
    // Step 3: Create container
    console.log('\n✓ Step 4: Creating container...');
    return fetch(`${BASE_URL}/containers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        internal_id: 'minecraft-server-001',
        volume_id: vol.id,
        startup_command: 'java -Xmx1024M -Xms1024M -jar server.jar nogui',
        image: 'openjdk:17-alpine',
        install_script: '#!/bin/sh\necho "Installing Minecraft server..."\nwget -O server.jar https://launcher.mojang.com/v1/objects/server.jar\necho "eula=true" > eula.txt\necho "Installation complete!"'
      })
    }).then(r => r.json()).then(container => {
      console.log('✓ Container installation started:', container.internal_id);
      testData.container = container;
      return container;
    });
  })
  .then(container => {
    // Mark ports as in use
    return Promise.all(
      testData.ports.map(port => 
        fetch(`${BASE_URL}/network/ports/${port.id}/use`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ in_use: true })
        }).then(r => r.json())
      )
    ).then(() => {
      console.log('✓ Step 5: Marked ports as in use');
      return container;
    });
  })
  .then(container => {
    // Wait for installation
    console.log('\n⏳ Waiting for container installation...\n');
    return new Promise(resolve => {
      setTimeout(() => resolve(container), 10000);
    });
  })
  .then(container => {
    // Check final status
    return Promise.all([
      fetch(`${BASE_URL}/containers/${container.internal_id}`).then(r => r.json()),
      fetch(`${BASE_URL}/volumes/${testData.volume.id}/files`).then(r => r.json()),
      fetch(`${BASE_URL}/network/ports`).then(r => r.json())
    ]).then(([containerStatus, volumeFiles, networkPorts]) => {
      console.log('=== Final Status ===\n');
      
      console.log('Container:');
      console.log(`  ID: ${containerStatus.internal_id}`);
      console.log(`  State: ${containerStatus.install_state}`);
      console.log(`  Installing: ${containerStatus.is_installing}`);
      console.log(`  Docker ID: ${containerStatus.container_id || 'pending'}`);
      
      console.log('\nVolume Files:', volumeFiles.files.length);
      volumeFiles.files.forEach(f => console.log(`  - ${f}`));
      
      console.log('\nNetwork Ports:');
      networkPorts.forEach(p => {
        console.log(`  - ${p.ip}:${p.port} (${p.in_use ? 'IN USE' : 'available'})`);
      });
      
      console.log('\n✅ Integration test complete!');
    });
  })
  .catch(e => console.error('❌ Error:', e));
