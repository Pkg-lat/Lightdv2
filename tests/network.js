const BASE_URL = 'http://localhost:8070';

console.log('=== Network Pool Tests ===\n');

// Add some ports to the pool
Promise.all([
  fetch(`${BASE_URL}/network/ports`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ip: '0.0.0.0', port: 25565 })
  }).then(r => r.json()),
  
  fetch(`${BASE_URL}/network/ports`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ip: '0.0.0.0', port: 25566 })
  }).then(r => r.json()),
  
  fetch(`${BASE_URL}/network/ports`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ip: '127.0.0.1', port: 8080 })
  }).then(r => r.json()),
  
  fetch(`${BASE_URL}/network/ports`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ip: '127.0.0.1', port: 8081 })
  }).then(r => r.json())
])
.then(ports => {
  console.log('✓ Added ports to pool:');
  ports.forEach(p => {
    console.log(`  - ${p.ip}:${p.port} (ID: ${p.id})`);
  });
  return ports;
})
.then(ports => {
  // Get all ports
  return fetch(`${BASE_URL}/network/ports`)
    .then(r => r.json())
    .then(allPorts => {
      console.log('\n✓ All ports in pool:', allPorts.length);
      allPorts.forEach(p => {
        console.log(`  - ${p.ip}:${p.port} - In use: ${p.in_use}`);
      });
      return ports;
    });
})
.then(ports => {
  // Mark first port as in use
  const firstPort = ports[0];
  return fetch(`${BASE_URL}/network/ports/${firstPort.id}/use`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ in_use: true })
  })
  .then(r => r.json())
  .then(updated => {
    console.log(`\n✓ Marked port ${updated.ip}:${updated.port} as in use`);
    return ports;
  });
})
.then(ports => {
  // Get a random available port
  return fetch(`${BASE_URL}/network/ports/random`)
    .then(r => r.json())
    .then(randomPort => {
      console.log(`\n✓ Random available port: ${randomPort.ip}:${randomPort.port}`);
      return ports;
    });
})
.then(ports => {
  // Get specific port
  const secondPort = ports[1];
  return fetch(`${BASE_URL}/network/ports/${secondPort.id}`)
    .then(r => r.json())
    .then(port => {
      console.log(`\n✓ Retrieved specific port: ${port.ip}:${port.port}`);
      return ports;
    });
})
.then(ports => {
  // Bulk delete last two ports
  const idsToDelete = [ports[2].id, ports[3].id];
  return fetch(`${BASE_URL}/network/ports/bulk-delete`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ids: idsToDelete })
  })
  .then(r => r.json())
  .then(result => {
    console.log(`\n✓ Bulk deleted ${result.deleted.length} ports`);
    return ports;
  });
})
.then(ports => {
  // Final list
  return fetch(`${BASE_URL}/network/ports`)
    .then(r => r.json())
    .then(finalPorts => {
      console.log('\n✓ Final port list:', finalPorts.length);
      finalPorts.forEach(p => {
        console.log(`  - ${p.ip}:${p.port} - In use: ${p.in_use}`);
      });
    });
})
.catch(e => console.error('❌ Error:', e));
