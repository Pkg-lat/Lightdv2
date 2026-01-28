// Firewall API Test
// Tests container firewall with DDoS protection

const BASE_URL = 'http://localhost:8070';
const TOKEN = 'lightd_ad2f7fc49ed640429c450e14ed07c8d5';

const headers = {
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${TOKEN}`,
    'Accept': 'Application/vnd.pkglatv1+json'
};

async function testFirewall() {
    console.log('=== Firewall API Test ===\n');

    const containerId = 'firewall-test-001';

    try {
        // Step 1: Create isolated network for container
        console.log('1. Creating isolated network...');
        const networkRes = await fetch(`${BASE_URL}/firewall/networks/${containerId}`, {
            method: 'POST',
            headers
        });
        const networkData = await networkRes.json();
        console.log('Network created:', networkData);
        console.log('');

        // Step 2: Add firewall rule - Block specific IP
        console.log('2. Adding firewall rule - Block IP 192.168.1.100...');
        const blockIpRes = await fetch(`${BASE_URL}/firewall/rules`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                container_id: containerId,
                source_ip: '192.168.1.100',
                protocol: 'all',
                action: 'drop',
                description: 'Block malicious IP'
            })
        });
        const blockIpData = await blockIpRes.json();
        console.log('Rule created:', blockIpData);
        const ruleId1 = blockIpData.rule.id;
        console.log('');

        // Step 3: Add firewall rule - Rate limit on port 80
        console.log('3. Adding firewall rule - Rate limit port 80...');
        const rateLimitRes = await fetch(`${BASE_URL}/firewall/rules`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                container_id: containerId,
                dest_port: 80,
                protocol: 'tcp',
                action: 'accept',
                rate_limit: {
                    requests: 100,
                    per_seconds: 60
                },
                description: 'Rate limit HTTP traffic'
            })
        });
        const rateLimitData = await rateLimitRes.json();
        console.log('Rule created:', rateLimitData);
        const ruleId2 = rateLimitData.rule.id;
        console.log('');

        // Step 4: Add firewall rule - Allow SSH from specific IP
        console.log('4. Adding firewall rule - Allow SSH from 10.0.0.0/8...');
        const allowSshRes = await fetch(`${BASE_URL}/firewall/rules`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                container_id: containerId,
                source_ip: '10.0.0.0/8',
                dest_port: 22,
                protocol: 'tcp',
                action: 'accept',
                description: 'Allow SSH from internal network'
            })
        });
        const allowSshData = await allowSshRes.json();
        console.log('Rule created:', allowSshData);
        const ruleId3 = allowSshData.rule.id;
        console.log('');

        // Step 5: Get all rules for container
        console.log('5. Getting all rules for container...');
        const rulesRes = await fetch(`${BASE_URL}/firewall/rules/container/${containerId}`, {
            headers
        });
        const rulesData = await rulesRes.json();
        console.log('Container rules:', JSON.stringify(rulesData, null, 2));
        console.log('');

        // Step 6: Enable DDoS protection
        console.log('6. Enabling DDoS protection...');
        const ddosRes = await fetch(`${BASE_URL}/firewall/ddos/${containerId}`, {
            method: 'POST',
            headers,
            body: JSON.stringify({
                enabled: true,
                syn_flood_protection: true,
                connection_limit: 100,
                rate_limit: {
                    requests: 1000,
                    per_seconds: 60
                }
            })
        });
        const ddosData = await ddosRes.json();
        console.log('DDoS protection:', ddosData);
        console.log('');

        // Step 7: Disable a rule
        console.log('7. Disabling rule 1...');
        const toggleRes = await fetch(`${BASE_URL}/firewall/rules/${ruleId1}/toggle`, {
            method: 'PUT',
            headers,
            body: JSON.stringify({
                enabled: false
            })
        });
        const toggleData = await toggleRes.json();
        console.log('Rule toggled:', toggleData);
        console.log('');

        // Step 8: Delete a rule
        console.log('8. Deleting rule 2...');
        const deleteRes = await fetch(`${BASE_URL}/firewall/rules/${ruleId2}`, {
            method: 'DELETE',
            headers
        });
        const deleteData = await deleteRes.json();
        console.log('Rule deleted:', deleteData);
        console.log('');

        // Step 9: Get updated rules
        console.log('9. Getting updated rules...');
        const updatedRulesRes = await fetch(`${BASE_URL}/firewall/rules/container/${containerId}`, {
            headers
        });
        const updatedRulesData = await updatedRulesRes.json();
        console.log('Updated rules:', JSON.stringify(updatedRulesData, null, 2));
        console.log('');

        // Step 10: Cleanup all rules and network
        console.log('10. Cleaning up container firewall...');
        const cleanupRes = await fetch(`${BASE_URL}/firewall/cleanup/${containerId}`, {
            method: 'DELETE',
            headers
        });
        const cleanupData = await cleanupRes.json();
        console.log('Cleanup result:', cleanupData);
        console.log('');

        console.log('=== Test Complete ===');

    } catch (error) {
        console.error('Test failed:', error);
    }
}

// Run test
testFirewall();
