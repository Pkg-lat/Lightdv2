// Detailed File Listing Test
// Tests comprehensive file metadata API

const BASE_URL = 'http://localhost:8070';
const TOKEN = 'lightd_ad2f7fc49ed640429c450e14ed07c8d5';

const headers = {
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${TOKEN}`,
    'Accept': 'Application/vnd.pkglatv1+json'
};

async function testDetailedFileListing() {
    console.log('=== Detailed File Listing Test ===\n');

    try {
        // Step 1: Create a volume
        console.log('1. Creating volume...');
        const volumeRes = await fetch(`${BASE_URL}/volumes`, {
            method: 'POST',
            headers
        });
        const volumeData = await volumeRes.json();
        console.log('Volume created:', volumeData.id);
        const volumeId = volumeData.id;
        console.log('');

        // Step 2: Create various file types
        console.log('2. Creating test files...');
        
        // Text file
        await fetch(`${BASE_URL}/volumes/${volumeId}/write`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                filename: 'readme.txt',
                content: 'This is a plain text file\nWith multiple lines\n'
            })
        });
        console.log('  Created: readme.txt');

        // JSON file
        await fetch(`${BASE_URL}/volumes/${volumeId}/write`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                filename: 'config.json',
                content: JSON.stringify({ server: { port: 8080 } }, null, 2)
            })
        });
        console.log('  Created: config.json');

        // Shell script
        await fetch(`${BASE_URL}/volumes/${volumeId}/write`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                filename: 'start.sh',
                content: '#!/bin/bash\necho "Starting server..."\n'
            })
        });
        console.log('  Created: start.sh');

        // Python script
        await fetch(`${BASE_URL}/volumes/${volumeId}/write`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                filename: 'app.py',
                content: 'print("Hello, World!")\n'
            })
        });
        console.log('  Created: app.py');

        // Markdown file
        await fetch(`${BASE_URL}/volumes/${volumeId}/write`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                filename: 'NOTES.md',
                content: '# Notes\n\n## Section 1\n\nSome content here.\n'
            })
        });
        console.log('  Created: NOTES.md');

        // Log file
        await fetch(`${BASE_URL}/volumes/${volumeId}/write`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                filename: 'server.log',
                content: '[2024-01-15 10:30:00] Server started\n[2024-01-15 10:30:01] Listening on port 8080\n'
            })
        });
        console.log('  Created: server.log');

        console.log('');

        // Step 3: Create directories
        console.log('3. Creating directories...');
        
        await fetch(`${BASE_URL}/volumes/${volumeId}/create-folder`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                root: '/',
                name: 'config'
            })
        });
        console.log('  Created: config/');

        await fetch(`${BASE_URL}/volumes/${volumeId}/create-folder`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                root: '/',
                name: 'logs'
            })
        });
        console.log('  Created: logs/');

        await fetch(`${BASE_URL}/volumes/${volumeId}/create-folder`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                root: '/',
                name: 'data'
            })
        });
        console.log('  Created: data/');

        console.log('');

        // Step 4: Get basic file list
        console.log('4. Getting basic file list...');
        const basicRes = await fetch(`${BASE_URL}/volumes/${volumeId}/files`, {
            headers
        });
        const basicData = await basicRes.json();
        console.log('Basic files:', basicData.files);
        console.log('');

        // Step 5: Get detailed file list
        console.log('5. Getting detailed file list...');
        const detailedRes = await fetch(`${BASE_URL}/volumes/${volumeId}/files/detailed`, {
            headers
        });
        const detailedData = await detailedRes.json();
        console.log('Detailed files:');
        console.log(JSON.stringify(detailedData, null, 2));
        console.log('');

        // Step 6: Analyze file metadata
        console.log('6. Analyzing file metadata...');
        console.log('');
        
        detailedData.files.forEach(file => {
            const attrs = file.attributes;
            console.log(`File: ${attrs.name}`);
            console.log(`  Type: ${attrs.is_file ? 'File' : 'Directory'}`);
            console.log(`  Mode: ${attrs.mode} (${attrs.mode_bits})`);
            console.log(`  Size: ${attrs.size} bytes`);
            console.log(`  MIME: ${attrs.mimetype}`);
            console.log(`  Created: ${attrs.created_at}`);
            console.log(`  Modified: ${attrs.modified_at}`);
            console.log('');
        });

        // Step 7: Verify MIME types
        console.log('7. Verifying MIME type detection...');
        const mimeTypes = {};
        detailedData.files.forEach(file => {
            const name = file.attributes.name;
            const mime = file.attributes.mimetype;
            mimeTypes[name] = mime;
        });
        
        console.log('MIME Types:');
        console.log(JSON.stringify(mimeTypes, null, 2));
        console.log('');

        // Step 8: Check sorting (directories first)
        console.log('8. Verifying sort order (directories first)...');
        let directoriesFirst = true;
        let foundFile = false;
        
        for (const file of detailedData.files) {
            if (file.attributes.is_file) {
                foundFile = true;
            } else if (foundFile) {
                directoriesFirst = false;
                break;
            }
        }
        
        console.log(`Directories first: ${directoriesFirst ? '✓ PASS' : '✗ FAIL'}`);
        console.log('');

        // Step 9: Verify file count
        console.log('9. Verifying file count...');
        const fileCount = detailedData.files.filter(f => f.attributes.is_file).length;
        const dirCount = detailedData.files.filter(f => !f.attributes.is_file).length;
        console.log(`Files: ${fileCount}`);
        console.log(`Directories: ${dirCount}`);
        console.log(`Total: ${detailedData.files.length}`);
        console.log('');

        // Step 10: Test specific MIME types
        console.log('10. Testing specific MIME type detection...');
        const tests = [
            { name: 'readme.txt', expected: 'text/plain' },
            { name: 'config.json', expected: 'application/json' },
            { name: 'start.sh', expected: 'text/x-shellscript' },
            { name: 'app.py', expected: 'text/x-python' },
            { name: 'NOTES.md', expected: 'text/markdown' },
            { name: 'server.log', expected: 'text/plain' },
        ];
        
        tests.forEach(test => {
            const actual = mimeTypes[test.name];
            const pass = actual === test.expected;
            console.log(`  ${test.name}: ${actual} ${pass ? '✓' : '✗ (expected: ' + test.expected + ')'}`);
        });
        console.log('');

        // Step 11: Cleanup
        console.log('11. Cleaning up...');
        await fetch(`${BASE_URL}/volumes/${volumeId}`, {
            method: 'DELETE',
            headers
        });
        console.log('Volume deleted');
        console.log('');

        console.log('=== Test Complete ===');

    } catch (error) {
        console.error('Test failed:', error);
    }
}

// Run test
testDetailedFileListing();
