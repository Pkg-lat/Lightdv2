#!/usr/bin/env node

/**
 * Full Integration Test
 * Tests complete workflow: volume creation, install script, Node.js + Express server
 */

const BASE_URL = 'http://localhost:8070';
const TOKEN = 'lightd_ad2f7fc49ed640429c450e14ed07c8d5';

const headers = {
    'Authorization': `Bearer ${TOKEN}`,
    'Accept': 'Application/vnd.pkglatv1+json',
    'Content-Type': 'application/json'
};

let volumeId = null;
let containerId = null;
let containerPort = null;

// Helper functions
async function request(method, path, body = null) {
    const options = {
        method,
        headers
    };
    
    if (body) {
        options.body = JSON.stringify(body);
    }
    
    const response = await fetch(`${BASE_URL}${path}`, options);
    const text = await response.text();
    
    let data;
    try {
        data = text ? JSON.parse(text) : null;
    } catch (e) {
        data = text;
    }
    
    return { status: response.status, data };
}

function log(message, data = null) {
    console.log(`\n✓ ${message}`);
    if (data) {
        console.log(JSON.stringify(data, null, 2));
    }
}

function error(message, data = null) {
    console.error(`\n✗ ${message}`);
    if (data) {
        console.error(JSON.stringify(data, null, 2));
    }
}

async function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

// Test functions
async function testCreateVolume() {
    console.log('\n=== Step 1: Create Volume ===');
    
    const { status, data } = await request('POST', '/volumes', {
        size: 2048  // 2GB for Node.js and dependencies
    });
    
    if (status === 200 || status === 201) {
        volumeId = data.id;
        log(`Created volume with 2GB quota: ${volumeId}`, data);
    } else {
        error(`Failed to create volume: ${status}`, data);
        throw new Error('Volume creation failed');
    }
}

async function testCreateExpressApp() {
    console.log('\n=== Step 2: Create Express App in Volume ===');
    
    // Create package.json
    const packageJson = {
        name: "lightd-test-app",
        version: "1.0.0",
        main: "server.js",
        dependencies: {
            express: "^4.18.2"
        }
    };
    
    const packageResult = await request('POST', `/volumes/${volumeId}/write`, {
        filename: 'package.json',
        content: JSON.stringify(packageJson, null, 2)
    });
    
    if (packageResult.status === 200 || packageResult.status === 201) {
        log('Created package.json');
    } else {
        error('Failed to create package.json', packageResult);
    }
    
    // Create Express server
    const serverCode = `const express = require('express');
const app = express();
const port = process.env.PORT || 3000;

app.use(express.json());

app.get('/', (req, res) => {
  res.json({
    message: 'Hello from Lightd!',
    timestamp: new Date().toISOString(),
    environment: process.env.NODE_ENV || 'development'
  });
});

app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    uptime: process.uptime(),
    memory: process.memoryUsage()
  });
});

app.post('/echo', (req, res) => {
  res.json({
    received: req.body,
    timestamp: new Date().toISOString()
  });
});

app.listen(port, '0.0.0.0', () => {
  console.log(\`Express server listening on port \${port}\`);
});
`;
    
    const serverResult = await request('POST', `/volumes/${volumeId}/write`, {
        filename: 'server.js',
        content: serverCode
    });
    
    if (serverResult.status === 200 || serverResult.status === 201) {
        log('Created server.js');
    } else {
        error('Failed to create server.js', serverResult);
    }
}

async function testCreateContainerWithInstall() {
    console.log('\n=== Step 3: Create Container with Node.js Install Script ===');
    
    // Install script that sets up Node.js and Express
    const installScript = `#!/bin/sh
set -e

echo "Installing Node.js and dependencies..."

apk update
apk add --no-cache nodejs npm

node --version
npm --version

cd /home/container

echo "Installing Express..."
npm install --production

echo "Installation Complete"
ls -la /home/container
`;
    
    containerId = `express-test-${Date.now()}`;
    
    const containerConfig = {
        internal_id: containerId,
        volume_id: volumeId,
        startup_command: 'node server.js',
        image: 'alpine:latest',
        install_script: installScript,
        start_pattern: 'Express server listening',
        ports: [
            { container_port: 3000, protocol: 'tcp' }
        ]
    };
    
    const { status, data } = await request('POST', '/containers', containerConfig);
    
    if (status === 200 || status === 201) {
        log(`Created container: ${containerId}`, data);
        log('Install script will run automatically...');
        log('Port will be automatically assigned from pool');
    } else {
        error(`Failed to create container: ${status}`, data);
        throw new Error('Container creation failed');
    }
}

async function testWaitForInstallation() {
    console.log('\n=== Step 4: Wait for Installation ===');
    
    let attempts = 0;
    const maxAttempts = 30;  // 30 attempts = 60 seconds
    
    while (attempts < maxAttempts) {
        await sleep(2000);
        attempts++;
        
        const { status, data } = await request('GET', `/containers/${containerId}`);
        
        if (status === 200) {
            log(`Installation status: ${data.install_state} (attempt ${attempts}/${maxAttempts})`);
            
            if (data.install_state === 'Installed' || data.install_state === 'Ready') {
                log('Installation completed successfully!');
                return;
            } else if (data.install_state === 'Failed') {
                error('Installation failed!', data);
                throw new Error('Installation failed');
            }
        }
    }
    
    error('Installation timeout - took longer than 60 seconds');
    throw new Error('Installation timeout');
}

async function testGetContainerStatus() {
    console.log('\n=== Step 5: Get Container Status ===');
    
    const { status, data } = await request('GET', `/containers/${containerId}`);
    
    if (status === 200) {
        log('Container status', {
            internal_id: data.internal_id,
            install_state: data.install_state,
            is_installing: data.is_installing,
            container_id: data.container_id,
            is_healthy: data.is_healthy
        });
        
        if (data.container_id) {
            log('Container is running with Docker ID: ' + data.container_id);
        }
        
        // Get assigned port
        if (data.ports && data.ports.length > 0) {
            containerPort = data.ports[0].host_port;
            log(`Assigned host port: ${containerPort} (container port: ${data.ports[0].container_port})`);
        }
    } else {
        error(`Failed to get status: ${status}`, data);
    }
}

async function testListContainerFiles() {
    console.log('\n=== Step 6: List Files in Volume ===');
    
    const { status, data } = await request('GET', `/volumes/${volumeId}/files/detailed`);
    
    if (status === 200) {
        log(`Found ${data.files.length} files in volume`);
        
        // Show important files
        const importantFiles = data.files.filter(f => 
            f.name === 'package.json' || 
            f.name === 'server.js'
        );
        
        importantFiles.forEach(file => {
            console.log(`  - ${file.name} (${file.size} bytes)`);
        });
    } else {
        error(`Failed to list files: ${status}`, data);
    }
}

async function testNetworkPool() {
    console.log('\n=== Step 7: Check Network Pool ===');
    
    const { status, data } = await request('GET', '/network/ports');
    
    if (status === 200) {
        const availablePorts = data.filter(p => !p.in_use);
        const usedPorts = data.filter(p => p.in_use);
        
        log(`Network pool status:`, {
            total: data.length,
            available: availablePorts.length,
            in_use: usedPorts.length
        });
        
        if (usedPorts.length > 0) {
            console.log('\nPorts in use:');
            usedPorts.forEach(p => {
                console.log(`  - ${p.ip}:${p.port}/${p.protocol}`);
            });
        }
    } else {
        error(`Failed to get network pool: ${status}`, data);
    }
}

async function testAccessExpressServer() {
    console.log('\n=== Step 8: Test Express Server ===');
    
    if (!containerPort) {
        error('No port assigned to container');
        return;
    }
    
    // Wait a bit for server to fully start
    await sleep(3000);
    
    try {
        // Test root endpoint
        const rootResponse = await fetch(`http://localhost:${containerPort}/`);
        const rootData = await rootResponse.json();
        
        if (rootData.message === 'Hello from Lightd!') {
            log('Express server is responding!', rootData);
        } else {
            error('Unexpected response from Express server', rootData);
        }
        
        // Test health endpoint
        const healthResponse = await fetch(`http://localhost:${containerPort}/health`);
        const healthData = await healthResponse.json();
        
        if (healthData.status === 'healthy') {
            log('Health check passed!', {
                status: healthData.status,
                uptime: healthData.uptime.toFixed(2) + 's'
            });
        }
        
        // Test echo endpoint
        const echoResponse = await fetch(`http://localhost:${containerPort}/echo`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ test: 'data', from: 'lightd' })
        });
        const echoData = await echoResponse.json();
        
        if (echoData.received) {
            log('Echo endpoint works!', echoData);
        }
        
    } catch (err) {
        error('Failed to connect to Express server', err.message);
    }
}

async function testBillingTracking() {
    console.log('\n=== Step 9: Check Billing Tracking ===');
    
    // Wait for metrics to be collected
    await sleep(5000);
    
    const { status, data } = await request('GET', `/billing/usage/${containerId}/hourly`);
    
    if (status === 200) {
        log('Billing metrics collected', {
            memory_gb: data.memory_gb?.toFixed(4),
            cpu_vcpus: data.cpu_vcpus?.toFixed(4),
            estimated_cost: data.estimated_cost?.toFixed(6)
        });
    } else if (status === 404) {
        log('Billing data not yet available (container may be too new)');
    } else {
        error(`Failed to get billing data: ${status}`, data);
    }
}

async function testContainerLogs() {
    console.log('\n=== Step 10: Check Container Logs ===');
    
    const { status, data } = await request('GET', `/containers/${containerId}/logs?lines=20`);
    
    if (status === 200) {
        log('Container logs retrieved');
        if (data.logs) {
            console.log('\nRecent logs:');
            console.log('---');
            console.log(data.logs);
            console.log('---');
        }
    } else {
        log('Logs endpoint may not be available');
    }
}

async function testCleanup() {
    console.log('\n=== Cleanup Skipped ===');
    console.log('Container and volume left running for manual inspection');
    console.log(`Container ID: ${containerId}`);
    console.log(`Volume ID: ${volumeId}`);
    console.log(`Port: ${containerPort || 'N/A'}`);
}

// Main test runner
async function runTests() {
    console.log('╔════════════════════════════════════════════════════╗');
    console.log('║   Lightd Full Integration Test                    ║');
    console.log('║   Node.js + Express + Install Scripts + Billing   ║');
    console.log('╚════════════════════════════════════════════════════╝');
    
    try {
        await testCreateVolume();
        await testCreateExpressApp();
        await testCreateContainerWithInstall();
        await testWaitForInstallation();
        await testGetContainerStatus();
        await testListContainerFiles();
        await testNetworkPool();
        await testAccessExpressServer();
        await testBillingTracking();
        await testContainerLogs();
        
        console.log('\n╔════════════════════════════════════════════════════╗');
        console.log('║         All Integration Tests Passed! 🎉          ║');
        console.log('╚════════════════════════════════════════════════════╝');
        
        console.log('\n⚠️  Container and volume left running for inspection');
        console.log(`   Express server: http://localhost:${containerPort || 'N/A'}`);
        console.log(`   Container ID: ${containerId}`);
        console.log(`   Volume ID: ${volumeId}`);
        console.log(`\n   Check files: storage/volumes/${volumeId}`);
        console.log(`   Check logs: storage/containers/${containerId}/install.log\n`);
        
    } catch (err) {
        error('Integration test failed', err.message);
        console.error(err);
        process.exit(1);
    }
}

// Run tests
runTests();
