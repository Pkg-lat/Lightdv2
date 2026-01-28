const BASE_URL = 'http://localhost:8070';

console.log('=== Network Rebinding Test ===\n');

// Helper function to check container status
async function waitForInstallation(containerId, maxAttempts = 60) {
  for (let i = 0; i < maxAttempts; i++) {
    const response = await fetch(`${BASE_URL}/containers/${containerId}/status`);
    const status = await response.json();

    console.log(`⏳ Checking status (${i + 1}/${maxAttempts}): install_state=${status.install_state}, is_installing=${status.is_installing}`);

    if (!status.is_installing && status.install_state === 'ready') {
      console.log('✓ Installation complete!\n');
      return true;
    }

    if (status.install_state === 'failed') {
      console.log('❌ Installation failed:', status.error_message || 'Unknown error');
      throw new Error(`Container installation failed: ${status.error_message || 'Unknown error'}`);
    }

    await new Promise(resolve => setTimeout(resolve, 2000));
  }

  throw new Error('Installation timeout - container did not become ready');
}

// Helper function to wait for rebinding
async function waitForRebinding(containerId, maxAttempts = 30) {
  for (let i = 0; i < maxAttempts; i++) {
    const response = await fetch(`${BASE_URL}/containers/${containerId}`);
    const state = await response.json();

    console.log(`⏳ Checking rebind (${i + 1}/${maxAttempts}): Docker ID: ${state.container_id ? state.container_id.substring(0, 12) : 'null'}, Ports: ${state.ports.length}`);

    if (state.container_id && state.ports.length > 0) {
      console.log('✓ Rebinding complete!\n');
      return state;
    }

    await new Promise(resolve => setTimeout(resolve, 2000));
  }

  throw new Error('Rebinding timeout');
}

// Cleanup helper
async function cleanupContainer(containerId) {
  try {
    console.log(`🧹 Cleaning up container: ${containerId}`);
    await fetch(`${BASE_URL}/containers/${containerId}`, { method: 'DELETE' });
    console.log('✓ Container deleted\n');
  } catch (e) {
    console.log('⚠ Cleanup failed (may not exist):', e.message);
  }
}

// Main test flow
(async () => {
  const testId = 'network-test-001';

  try {
    // Cleanup any previous test container
    await cleanupContainer(testId);

    // Create volume
    console.log('📦 Creating volume...');
    const volResponse = await fetch(`${BASE_URL}/volumes`, { method: 'POST' });
    const vol = await volResponse.json();
    console.log('✓ Created volume:', vol.id, '\n');

    // Create container (alpine is a simple image that starts quickly)
    console.log('🐳 Creating container...');
    const containerResponse = await fetch(`${BASE_URL}/containers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        internal_id: testId,
        volume_id: vol.id,
        startup_command: '/bin/sh -c "echo Container running && while true; do sleep 5; done"',
        image: 'alpine:latest'
      })
    });

    if (!containerResponse.ok) {
      const errorText = await containerResponse.text();
      throw new Error(`Failed to create container: ${errorText}`);
    }

    const container = await containerResponse.json();
    console.log('✓ Container created');
    console.log('  Internal ID:', container.internal_id);
    console.log('  Volume ID:', container.volume_id);
    console.log('  Is Installing:', container.is_installing);
    console.log('');

    // Wait for installation to complete
    console.log('⏳ Waiting for installation...');
    await waitForInstallation(container.internal_id);

    // Get the container state to see the Docker ID
    const stateResponse = await fetch(`${BASE_URL}/containers/${testId}`);
    const stateData = await stateResponse.json();
    console.log('📋 Container state after installation:');
    console.log('  Docker Container ID:', stateData.container_id);
    console.log('  Install State:', stateData.install_state);
    console.log('');

    // Rebind network with new ports
    console.log('🔄 Rebinding network with new ports...');
    const rebindResponse = await fetch(`${BASE_URL}/containers/${testId}/rebind-network`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ports: [
          { ip: '0.0.0.0', port: 25565, protocol: 'tcp' },
          { ip: '0.0.0.0', port: 25565, protocol: 'udp' },
          { ip: '0.0.0.0', port: 8080, protocol: 'tcp' }
        ],
        image: 'alpine:latest'
      })
    });

    if (!rebindResponse.ok) {
      const errorText = await rebindResponse.text();
      throw new Error(`Failed to rebind network: ${errorText}`);
    }

    const rebindResult = await rebindResponse.json();
    console.log('✓ Rebind initiated:', rebindResult.message, '\n');

    // Wait for rebinding to complete
    const finalState = await waitForRebinding(testId);

    // Display final state
    console.log('=== Final Container State ===');
    console.log('Internal ID:', finalState.internal_id);
    console.log('Docker Container ID:', finalState.container_id);
    console.log('Install State:', finalState.install_state);
    console.log('Ports:', JSON.stringify(finalState.ports, null, 2));
    console.log('\n✅ Network rebinding test complete!');

  } catch (error) {
    console.error('❌ Error:', error.message);
    console.error('Stack:', error.stack);
  }
})();
