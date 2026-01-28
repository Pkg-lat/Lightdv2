const BASE_URL = 'http://localhost:8070';

// Test power actions
console.log('=== Power Actions Tests ===\n');

// Helper to wait
const wait = (ms) => new Promise(res => setTimeout(res, ms));

fetch(`${BASE_URL}/volumes`, { method: 'POST' })
  .then(r => r.json())
  .then(vol => {
    console.log('✓ Created volume:', vol.id);

    // Create a container for power testing
    return fetch(`${BASE_URL}/containers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        internal_id: 'power-test-001',
        volume_id: vol.id,
        startup_command: '/bin/sh -c "while true; do echo running; sleep 1; done"',
        image: 'alpine:latest'
      })
    }).then(r => r.json()).then(container => {
      console.log('✓ Container created:', container.internal_id);
      return { vol, container };
    });
  })
  .then(({ vol, container }) => {
    console.log('⏳ Waiting for container to start...');
    return wait(8000).then(() => ({ vol, container }));
  })
  .then(({ vol, container }) => {
    const id = container.internal_id;

    // 1. Restart
    console.log(`\n🔄 Testing Restart on ${id}...`);
    return fetch(`${BASE_URL}/containers/${id}/restart`, { method: 'POST' })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Restart response:', res);
        return wait(3000).then(() => ({ vol, container }));
      });
  })
  .then(({ vol, container }) => {
    const id = container.internal_id;

    // 2. Kill
    console.log(`\n🛑 Testing Kill on ${id}...`);
    return fetch(`${BASE_URL}/containers/${id}/kill`, { method: 'POST' })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Kill response:', res);
        return wait(1000).then(() => ({ vol, container }));
      });
  })
  .then(({ vol, container }) => {
    const id = container.internal_id;

    // 3. Start
    console.log(`\n▶️ Testing Start on ${id}...`);
    return fetch(`${BASE_URL}/containers/${id}/start`, { method: 'POST' })
      .then(r => r.json())
      .then(res => {
        console.log('✓ Start response:', res);
        return { vol, container };
      });
  })
  .then(() => {
    console.log('\n✅ Power actions tests completed successfully');
  })
  .catch(e => console.error('❌ Error:', e));
