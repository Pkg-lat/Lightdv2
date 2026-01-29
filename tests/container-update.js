// Container Update Test
// Tests dynamic container configuration updates

const BASE_URL = 'http://localhost:8070';
const TOKEN = 'lightd_ad2f7fc49ed640429c450e14ed07c8d5';

const headers = {
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${TOKEN}`,
    'Accept': 'Application/vnd.pkglatv1+json'
};

async function testContainerUpdate() {
    console.log('=== Container Update Test ===\n');

    const containerId = 'update-test-001';

    try {
        // Step 1: Create a volume
        console.log('1. Creating volume...');
        const volumeRes = await fetch(`${BASE_URL}/volumes`, {
            method: 'POST',
            headers,
            body: JSON.stringify({ size: 2048 })
        });
        const volumeData = await volumeRes.json();
        console.log('Volume created:', volumeData.id);
        const volumeId = volumeData.id;
        console.log('');

        // Step 2: Create container
        console.log('2. Creating container...');
        const createRes = await fetch(`${BASE_URL}/containers`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                internal_id: containerId,
                volume_id: volumeId,
                startup_command: 'whoami && id && while true; do echo "Running as $(whoami) ($(id -u):$(id -g))"; sleep 5; done',
                image: 'alpine:latest',
                start_pattern: 'Running'
            })
        });
        const createData = await createRes.json();
        console.log('Container created:', createData);
        console.log('');

        // Wait for container to be ready
        console.log('3. Waiting for container to be ready...');
        await new Promise(resolve => setTimeout(resolve, 10000));
        console.log('');

        // Step 4: Get current resources
        console.log('4. Getting current resource limits...');
        const getResourcesRes = await fetch(`${BASE_URL}/containers/${containerId}/resources`, {
            headers
        });
        const currentResources = await getResourcesRes.json();
        console.log('Current resources:', JSON.stringify(currentResources, null, 2));
        console.log('');

        // Step 5: Update memory limit to 512MB
        console.log('5. Updating memory limit to 512MB...');
        const updateMemoryRes = await fetch(`${BASE_URL}/containers/${containerId}/resources`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                memory: 512 * 1024 * 1024, // 512MB in bytes
                memory_swap: 1024 * 1024 * 1024 // 1GB swap
            })
        });
        const updateMemoryData = await updateMemoryRes.json();
        console.log('Memory update:', updateMemoryData);
        console.log('');

        // Wait for update to apply
        await new Promise(resolve => setTimeout(resolve, 2000));

        // Step 6: Update CPU limits
        console.log('6. Updating CPU limits...');
        const updateCpuRes = await fetch(`${BASE_URL}/containers/${containerId}/resources`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                cpu_shares: 512, // 50% of default
                cpu_quota: 50000, // 50% of one CPU
                cpu_period: 100000
            })
        });
        const updateCpuData = await updateCpuRes.json();
        console.log('CPU update:', updateCpuData);
        console.log('');

        // Wait for update to apply
        await new Promise(resolve => setTimeout(resolve, 2000));

        // Step 7: Get updated resources
        console.log('7. Getting updated resource limits...');
        const getResourcesRes2 = await fetch(`${BASE_URL}/containers/${containerId}/resources`, {
            headers
        });
        const updatedResources = await getResourcesRes2.json();
        console.log('Updated resources:', JSON.stringify(updatedResources, null, 2));
        console.log('');

        // Step 8: Update volumes
        console.log('8. Updating volumes...');
        const updateVolumesRes = await fetch(`${BASE_URL}/containers/${containerId}/volumes`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                volumes: {
                    '/app/config': '/tmp/config',
                    '/app/logs': '/tmp/logs'
                }
            })
        });
        const updateVolumesData = await updateVolumesRes.json();
        console.log('Volumes update:', updateVolumesData);
        console.log('');

        // Step 9: Get container state
        console.log('9. Getting container state...');
        const stateRes = await fetch(`${BASE_URL}/containers/${containerId}`, {
            headers
        });
        const stateData = await stateRes.json();
        console.log('Container state:', JSON.stringify(stateData, null, 2));
        console.log('');

        // Step 10: Update with combined limits
        console.log('10. Updating combined resource limits...');
        const updateCombinedRes = await fetch(`${BASE_URL}/containers/${containerId}/resources`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                memory: 1024 * 1024 * 1024, // 1GB
                memory_reservation: 512 * 1024 * 1024, // 512MB soft limit
                cpu_shares: 1024 // 100% of default
                // Note: blkio_weight removed as it's Linux-only
            })
        });
        const updateCombinedData = await updateCombinedRes.json();
        console.log('Combined update:', updateCombinedData);
        console.log('');

        // Wait for update
        await new Promise(resolve => setTimeout(resolve, 2000));

        // Step 11: Verify final resources
        console.log('11. Verifying final resource limits...');
        const finalResourcesRes = await fetch(`${BASE_URL}/containers/${containerId}/resources`, {
            headers
        });
        const finalResources = await finalResourcesRes.json();
        console.log('Final resources:', JSON.stringify(finalResources, null, 2));
        console.log('');
/*
        // Step 12: Kill container
        console.log('12. Stopping container...');
        const killRes = await fetch(`${BASE_URL}/containers/${containerId}/kill`, {
            method: 'POST',
            headers
        });
        if (killRes.ok) {
            console.log('Container stopped');
        }
        console.log('');

        
        console.log('13. Cleaning up...');
        await fetch(`${BASE_URL}/containers/${containerId}`, {
            method: 'DELETE',
            headers
        });
        await fetch(`${BASE_URL}/volumes/${volumeId}`, {
            method: 'DELETE',
            headers
        });
        console.log('Cleanup complete');
        console.log('');

        console.log('=== Test Complete ===');*/

    } catch (error) {
        console.error('Test failed:', error);
    }
}

// Run test
testContainerUpdate();
