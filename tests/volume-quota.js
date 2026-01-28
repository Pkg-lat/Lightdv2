// Volume Quota Test
// Tests volume creation with disk quotas

const BASE_URL = 'http://localhost:8070';
const TOKEN = 'lightd_ad2f7fc49ed640429c450e14ed07c8d5';

const headers = {
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${TOKEN}`,
    'Accept': 'Application/vnd.pkglatv1+json'
};

async function testVolumeQuota() {
    console.log('=== Volume Quota Test ===\n');

    try {
        // Step 1: Create volume with 2GB quota
        console.log('1. Creating volume with 2GB (2048MB) quota...');
        const createRes = await fetch(`${BASE_URL}/volumes`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                size: 2048 // 2GB in MB
            })
        });
        const createData = await createRes.json();
        console.log('Volume created:', createData);
        const volumeId = createData.id;
        console.log('');

        // Step 2: Get quota usage
        console.log('2. Getting quota usage...');
        const quotaRes = await fetch(`${BASE_URL}/volumes/${volumeId}/quota`, {
            headers
        });
        const quotaData = await quotaRes.json();
        console.log('Quota usage:', quotaData);
        console.log('');

        // Step 3: Write some files to the volume
        console.log('3. Writing test files to volume...');
        for (let i = 1; i <= 5; i++) {
            const writeRes = await fetch(`${BASE_URL}/volumes/${volumeId}/write`, {
                method: 'POST',
                headers,
                body: JSON.stringify({
                    filename: `test-file-${i}.txt`,
                    content: `This is test file ${i}\n`.repeat(1000)
                })
            });
            const writeData = await writeRes.json();
            console.log(`  Written: test-file-${i}.txt`);
        }
        console.log('');

        // Step 4: Check quota usage after writes
        console.log('4. Checking quota usage after writes...');
        const quotaRes2 = await fetch(`${BASE_URL}/volumes/${volumeId}/quota`, {
            headers
        });
        const quotaData2 = await quotaRes2.json();
        console.log('Updated quota usage:', quotaData2);
        console.log('');

        // Step 5: Resize volume to 4GB
        console.log('5. Resizing volume to 4GB (4096MB)...');
        const resizeRes = await fetch(`${BASE_URL}/volumes/${volumeId}/resize`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                size: 4096 // 4GB in MB
            })
        });
        if (resizeRes.ok) {
            console.log('Volume resized successfully');
        } else {
            const error = await resizeRes.json();
            console.log('Resize result:', error);
        }
        console.log('');

        // Step 6: Check quota after resize
        console.log('6. Checking quota after resize...');
        const quotaRes3 = await fetch(`${BASE_URL}/volumes/${volumeId}/quota`, {
            headers
        });
        const quotaData3 = await quotaRes3.json();
        console.log('Quota after resize:', quotaData3);
        console.log('');

        // Step 7: List all volumes
        console.log('7. Listing all volumes...');
        const listRes = await fetch(`${BASE_URL}/volumes`, {
            headers
        });
        const listData = await listRes.json();
        console.log('Volumes:', JSON.stringify(listData, null, 2));
        console.log('');

        // Step 8: Delete volume
        console.log('8. Deleting volume...');
        const deleteRes = await fetch(`${BASE_URL}/volumes/${volumeId}`, {
            method: 'DELETE',
            headers
        });
        if (deleteRes.ok) {
            console.log('Volume deleted successfully');
        } else {
            const error = await deleteRes.json();
            console.log('Delete error:', error);
        }
        console.log('');

        console.log('=== Test Complete ===');

    } catch (error) {
        console.error('Test failed:', error);
    }
}

// Run test
testVolumeQuota();
