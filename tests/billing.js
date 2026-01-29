#!/usr/bin/env node

/**
 * Billing System Test Suite
 * Tests resource tracking, cost estimation, and billing endpoints
 */

const BASE_URL = 'http://localhost:8070';
const TOKEN = 'lightd_ad2f7fc49ed640429c450e14ed07c8d5';

const headers = {
    'Authorization': `Bearer ${TOKEN}`,
    'Accept': 'Application/vnd.pkglatv1+json',
    'Content-Type': 'application/json'
};

let testContainerId = null;

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
async function testGetBillingRates() {
    console.log('\n=== Test: Get Billing Rates ===');
    
    const { status, data } = await request('GET', '/billing/rates');
    
    if (status === 200) {
        log('Retrieved billing rates', data);
        
        // Validate rate structure
        if (data.memory_per_gb_hour && data.cpu_per_vcpu_hour && 
            data.storage_per_gb_hour && data.egress_per_gb) {
            log('Rate structure is valid');
        } else {
            error('Invalid rate structure', data);
        }
    } else {
        error(`Failed to get rates: ${status}`, data);
    }
}

async function testEstimateContainer() {
    console.log('\n=== Test: Estimate Container Costs ===');
    
    const config = {
        memory_gb: 0.5,  // 512MB = 0.5GB
        cpu_vcpus: 1.0,
        storage_gb: 10.0,
        egress_gb_per_month: 100.0
    };
    
    const { status, data } = await request('POST', '/billing/estimate', config);
    
    if (status === 200) {
        log('Container cost estimate', data);
        
        // Validate estimate structure
        if (data.hourly && data.daily && data.monthly && data.breakdown) {
            log(`Hourly: $${data.hourly.toFixed(4)}`);
            log(`Daily: $${data.daily.toFixed(4)}`);
            log(`Monthly: $${data.monthly.toFixed(2)}`);
            
            // Check breakdown
            if (data.breakdown.memory && data.breakdown.cpu && 
                data.breakdown.storage && data.breakdown.egress) {
                log('Cost breakdown is complete');
            }
        } else {
            error('Invalid estimate structure', data);
        }
    } else {
        error(`Failed to estimate container: ${status}`, data);
    }
}

async function testEstimateVolume() {
    console.log('\n=== Test: Estimate Volume Costs ===');
    
    const config = {
        size_gb: 50.0
    };
    
    const { status, data } = await request('POST', '/billing/estimate/volume', config);
    
    if (status === 200) {
        log('Volume cost estimate', data);
        
        if (data.hourly && data.daily && data.monthly) {
            log(`Hourly: $${data.hourly.toFixed(6)}`);
            log(`Daily: $${data.daily.toFixed(4)}`);
            log(`Monthly: $${data.monthly.toFixed(2)}`);
        } else {
            error('Invalid estimate structure', data);
        }
    } else {
        error(`Failed to estimate volume: ${status}`, data);
    }
}

async function testCreateContainerForBilling() {
    console.log('\n=== Test: Create Container for Billing Tracking ===');
    
    // First create a volume
    const volumeResponse = await request('POST', '/volumes');
    
    if (volumeResponse.status !== 200 && volumeResponse.status !== 201) {
        error('Failed to create volume', volumeResponse);
        return;
    }
    
    const volumeId = volumeResponse.data.id;
    log(`Created volume: ${volumeId}`);
    
    // Now create container with proper fields
    const containerConfig = {
        internal_id: `billing-test-${Date.now()}`,
        volume_id: volumeId,
        startup_command: 'sh -c "while true; do echo billing test; sleep 5; done"',
        image: 'alpine:latest'
    };
    
    const { status, data } = await request('POST', '/containers', containerConfig);
    
    if (status === 200 || status === 201) {
        testContainerId = containerConfig.internal_id;
        log(`Created container for billing: ${testContainerId}`, data);
        
        // Wait for container to start and generate metrics
        log('Waiting for container to start and generate metrics...');
        await sleep(8000);
    } else {
        error(`Failed to create container: ${status}`, data);
    }
}

async function testGetContainerUsage() {
    console.log('\n=== Test: Get Container Usage (Hourly) ===');
    
    if (!testContainerId) {
        error('No test container available');
        return;
    }
    
    // Wait a bit for metrics to be collected
    await sleep(5000);
    
    const { status, data } = await request('GET', `/billing/usage/${testContainerId}/hourly`);
    
    if (status === 200) {
        log('Container usage data', data);
        
        if (data.memory_gb !== undefined && data.cpu_vcpus !== undefined && 
            data.storage_gb !== undefined && data.egress_gb !== undefined) {
            log('Usage snapshot is valid');
            log(`Memory: ${data.memory_gb.toFixed(4)} GB`);
            log(`CPU: ${data.cpu_vcpus.toFixed(4)} vCPUs`);
            log(`Storage: ${data.storage_gb.toFixed(4)} GB`);
            log(`Egress: ${data.egress_gb.toFixed(4)} GB`);
            log(`Estimated Cost: $${data.estimated_cost.toFixed(6)}`);
        } else {
            error('Invalid usage structure', data);
        }
    } else if (status === 404) {
        log('No usage data yet (container may be too new)', data);
    } else {
        error(`Failed to get usage: ${status}`, data);
    }
}

async function testGetContainerCost() {
    console.log('\n=== Test: Get Container Cost (Daily) ===');
    
    if (!testContainerId) {
        error('No test container available');
        return;
    }
    
    const { status, data } = await request('GET', `/billing/usage/${testContainerId}/daily`);
    
    if (status === 200) {
        log('Container daily cost', data);
        
        if (data.estimated_cost !== undefined) {
            log(`Daily cost: $${data.estimated_cost.toFixed(6)}`);
            log('Cost calculated successfully');
        } else {
            error('Invalid cost structure', data);
        }
    } else if (status === 404) {
        log('No cost data yet (container may be too new)', data);
    } else {
        error(`Failed to get cost: ${status}`, data);
    }
}

async function testGetTrackedContainers() {
    console.log('\n=== Test: Get Tracked Containers ===');
    
    const { status, data } = await request('GET', '/billing/containers');
    
    if (status === 200) {
        log('Tracked containers', data);
        
        if (Array.isArray(data)) {
            log(`Tracking ${data.length} container(s)`);
            
            if (testContainerId && data.includes(testContainerId)) {
                log(`Test container ${testContainerId} is being tracked`);
            }
        } else {
            error('Invalid tracked containers structure', data);
        }
    } else {
        error(`Failed to get tracked containers: ${status}`, data);
    }
}

async function testCompareEstimates() {
    console.log('\n=== Test: Compare Cost Estimates ===');
    
    // Small container
    const small = {
        memory_gb: 0.25,  // 256MB
        cpu_vcpus: 0.5,
        storage_gb: 5.0,
        egress_gb_per_month: 50.0
    };
    
    // Large container
    const large = {
        memory_gb: 2.0,  // 2GB
        cpu_vcpus: 2.0,
        storage_gb: 50.0,
        egress_gb_per_month: 500.0
    };
    
    const smallEstimate = await request('POST', '/billing/estimate', small);
    const largeEstimate = await request('POST', '/billing/estimate', large);
    
    if (smallEstimate.status === 200 && largeEstimate.status === 200) {
        log('Small container estimate', smallEstimate.data);
        log('Large container estimate', largeEstimate.data);
        
        const difference = largeEstimate.data.monthly - smallEstimate.data.monthly;
        const percentage = ((difference / smallEstimate.data.monthly) * 100).toFixed(2);
        
        log(`Cost difference: $${difference.toFixed(2)}/month (${percentage}% increase)`);
    } else {
        error('Failed to compare estimates');
    }
}

async function testMultipleTimeRanges() {
    console.log('\n=== Test: Multiple Time Ranges ===');
    
    if (!testContainerId) {
        error('No test container available');
        return;
    }
    
    const endpoints = [
        { name: 'hourly', path: 'hourly' },
        { name: 'daily', path: 'daily' },
        { name: 'monthly', path: 'monthly' }
    ];
    
    for (const endpoint of endpoints) {
        const { status, data } = await request('GET', `/billing/usage/${testContainerId}/${endpoint.path}`);
        
        if (status === 200) {
            log(`${endpoint.name} usage`, {
                memory_gb: data.memory_gb?.toFixed(4),
                cpu_vcpus: data.cpu_vcpus?.toFixed(4),
                duration_hours: data.duration_hours,
                estimated_cost: data.estimated_cost?.toFixed(6)
            });
        } else if (status === 404) {
            log(`No ${endpoint.name} data (expected for new containers)`);
        }
    }
}

async function testInvalidRequests() {
    console.log('\n=== Test: Invalid Requests ===');
    
    // Non-existent container usage
    const invalid = await request('GET', '/billing/usage/nonexistent-container-id/hourly');
    
    if (invalid.status === 404) {
        log('Correctly returned 404 for non-existent container');
    } else {
        error('Should return 404 for non-existent container', invalid);
    }
    
    // Test with zero values (should still work, just return 0 cost)
    const zeroEstimate = await request('POST', '/billing/estimate', {
        memory_gb: 0,
        cpu_vcpus: 0,
        storage_gb: 0,
        egress_gb_per_month: 0
    });
    
    if (zeroEstimate.status === 200) {
        log('Zero estimate returned successfully', {
            monthly: zeroEstimate.data.monthly
        });
    }
}

async function testCleanup() {
    console.log('\n=== Test: Cleanup ===');
    
    if (testContainerId) {
        // Stop container
        const stop = await request('POST', `/containers/${testContainerId}/stop`);
        if (stop.status === 200) {
            log(`Stopped container ${testContainerId}`);
        }
        
        await sleep(2000);
        
        // Delete container
        const del = await request('DELETE', `/containers/${testContainerId}`);
        if (del.status === 200) {
            log(`Deleted container ${testContainerId}`);
        }
    }
}

// Main test runner
async function runTests() {
    console.log('╔════════════════════════════════════════╗');
    console.log('║   Lightd Billing System Test Suite    ║');
    console.log('╚════════════════════════════════════════╝');
    
    try {
        // Basic billing endpoints
        await testGetBillingRates();
        await testEstimateContainer();
        await testEstimateVolume();
        await testCompareEstimates();
        
        // Container tracking tests
        await testCreateContainerForBilling();
        await testGetTrackedContainers();
        await testGetContainerUsage();
        await testGetContainerCost();
        await testMultipleTimeRanges();
        
        // Validation tests
        await testInvalidRequests();
        
        // Cleanup
        await testCleanup();
        
        console.log('\n╔════════════════════════════════════════╗');
        console.log('║         All Tests Completed!           ║');
        console.log('╚════════════════════════════════════════╝\n');
        
    } catch (err) {
        error('Test suite failed', err.message);
        console.error(err);
        process.exit(1);
    }
}

// Run tests
runTests();
