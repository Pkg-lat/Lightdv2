// SFTP Test Script for Lightd
// Tests SFTP credential generation and connection info

const API_BASE = 'http://localhost:8070';
const TOKEN = 'lightd_ad2f7fc49ed640429c450e14ed07c8d5';

const headers = {
    'Authorization': `Bearer ${TOKEN}`,
    'Accept': 'Application/vnd.pkglatv1+json',
    'Content-Type': 'application/json'
};

async function testSftpCredentials() {
    console.log('\n=== SFTP Credentials Test ===\n');
    
    // Step 1: Create a volume first
    console.log('Step 1: Creating volume...');
    const volumeResponse = await fetch(`${API_BASE}/volumes`, {
        method: 'POST',
        headers
    });
    
    if (!volumeResponse.ok) {
        console.error('Failed to create volume:', await volumeResponse.text());
        return;
    }
    
    const volume = await volumeResponse.json();
    console.log('✓ Volume created:', volume.id);
    
    // Step 2: Create a container
    console.log('\nStep 2: Creating test container...');
    const createResponse = await fetch(`${API_BASE}/containers`, {
        method: 'POST',
        headers,
        body: JSON.stringify({
            internal_id: 'sftp-test-001',
            volume_id: volume.id,
            image: 'alpine:latest',
            startup_command: 'sleep infinity',
            limits: {
                memory: 512000000,
                cpu: 1.0
            }
        })
    });
    
    if (!createResponse.ok) {
        console.error('Failed to create container:', await createResponse.text());
        return;
    }
    
    const container = await createResponse.json();
    console.log('✓ Container created:', container.internal_id);
    
    // Wait a bit for container to initialize
    await new Promise(resolve => setTimeout(resolve, 2000));
    
    // Step 3: Generate SFTP credentials (auto-generated)
    console.log('\nStep 3: Generating SFTP credentials (auto)...');
    const credsResponse = await fetch(`${API_BASE}/containers/sftp-test-001/sftp/credentials`, {
        method: 'POST',
        headers,
        body: JSON.stringify({})
    });
    
    if (!credsResponse.ok) {
        console.error('Failed to generate credentials:', await credsResponse.text());
        return;
    }
    
    const creds = await credsResponse.json();
    console.log('✓ SFTP Credentials generated:');
    console.log('  Username:', creds.username);
    console.log('  Password:', creds.password);
    console.log('  Host:', creds.host);
    console.log('  Port:', creds.port);
    console.log('  Volume Path:', creds.volume_path);
    
    // Step 4: Get SFTP info
    console.log('\nStep 4: Getting SFTP connection info...');
    const infoResponse = await fetch(`${API_BASE}/containers/sftp-test-001/sftp/info`, {
        method: 'GET',
        headers
    });
    
    if (!infoResponse.ok) {
        console.error('Failed to get SFTP info:', await infoResponse.text());
        return;
    }
    
    const info = await infoResponse.json();
    console.log('✓ SFTP Info:');
    console.log('  Username:', info.username);
    console.log('  Host:', info.host);
    console.log('  Port:', info.port);
    console.log('  Volume Path:', info.volume_path);
    console.log('  Created:', new Date(info.created_at * 1000).toISOString());
    
    // Step 5: Generate custom credentials
    console.log('\nStep 5: Generating custom SFTP credentials...');
    const customCredsResponse = await fetch(`${API_BASE}/containers/sftp-test-001/sftp/credentials`, {
        method: 'POST',
        headers,
        body: JSON.stringify({
            username: 'nadhi',
            password: 'nadhilove123'
        })
    });
    
    if (!customCredsResponse.ok) {
        console.error('Failed to generate custom credentials:', await customCredsResponse.text());
        return;
    }
    
    const customCreds = await customCredsResponse.json();
    console.log('✓ Custom SFTP Credentials:');
    console.log('  Username:', customCreds.username);
    console.log('  Password:', customCreds.password);
    
    // Step 6: Connection instructions
    console.log('\n=== SFTP Connection Instructions ===');
    console.log('\n⚠️  IMPORTANT: Remove old host key first:');
    console.log(`  ssh-keygen -R "[${creds.host}]:${creds.port}"`);
    console.log('\nUsing CLI:');
    console.log(`  sftp -P ${creds.port} ${customCreds.username}@${creds.host}`);
    console.log(`  Password: ${customCreds.password}`);
    
    console.log('\nUsing FileZilla:');
    console.log(`  Host: sftp://${creds.host}`);
    console.log(`  Port: ${creds.port}`);
    console.log(`  Username: ${customCreds.username}`);
    console.log(`  Password: ${customCreds.password}`);
    
    console.log('\n✓ SFTP test completed successfully!');
    console.log('\n📝 Next steps:');
    console.log(`  1. Run: ssh-keygen -R "[${creds.host}]:${creds.port}"`);
    console.log(`  2. Connect: sftp -P ${creds.port} ${customCreds.username}@${creds.host}`);
    console.log(`  3. Enter password when prompted: ${customCreds.password}`);
}

// Run the test
testSftpCredentials().catch(console.error);
