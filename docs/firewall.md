# Container Firewall System

The firewall system provides DDoS protection and security rules for containers using Docker's custom bridge networks and iptables. Each container gets its own isolated network, completely separate from the host network.

## Features

- **Isolated Networks**: Each container runs in its own Docker bridge network
- **Custom Firewall Rules**: Block/allow traffic based on IP, port, protocol
- **Rate Limiting**: Prevent abuse with configurable rate limits
- **DDoS Protection**: Built-in SYN flood protection and connection limits
- **Protocol Support**: TCP, UDP, ICMP, or all protocols
- **Rule Management**: Enable/disable rules without deletion

## Security

- **Host Network Isolation**: Container networks are completely isolated from host
- **No Host Abuse**: Firewall rules only apply to container traffic
- **iptables Integration**: Uses Linux kernel firewall for maximum security
- **Persistent Rules**: All rules stored in database and survive restarts

## API Endpoints

All endpoints require authentication with Bearer token and vendor header.

### Create Container Network

Creates an isolated Docker bridge network for a container.

```http
POST /firewall/networks/:container_id
```

**Response:**
```json
{
  "network_name": "lightd-net-firewall-test-001"
}
```

### Delete Container Network

Removes the container's isolated network.

```http
DELETE /firewall/networks/:container_id
```

### Create Firewall Rule

Add a new firewall rule for a container.

```http
POST /firewall/rules
```

**Request Body:**
```json
{
  "container_id": "my-container",
  "source_ip": "192.168.1.100",
  "source_port": 8080,
  "dest_port": 80,
  "protocol": "tcp",
  "action": "drop",
  "rate_limit": {
    "requests": 100,
    "per_seconds": 60
  },
  "description": "Block malicious IP"
}
```

**Fields:**
- `container_id` (required): Container internal ID
- `source_ip` (optional): Source IP or CIDR (e.g., "10.0.0.0/8")
- `source_port` (optional): Source port number
- `dest_port` (optional): Destination port number
- `protocol` (required): "tcp", "udp", "icmp", or "all"
- `action` (required): "accept", "drop", or "reject"
- `rate_limit` (optional): Rate limiting configuration
- `description` (optional): Human-readable description

**Response:**
```json
{
  "rule": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "container_id": "my-container",
    "source_ip": "192.168.1.100",
    "dest_port": 80,
    "protocol": "tcp",
    "action": "drop",
    "rate_limit": {
      "requests": 100,
      "per_seconds": 60
    },
    "description": "Block malicious IP",
    "enabled": true
  }
}
```

### Delete Firewall Rule

Remove a firewall rule.

```http
DELETE /firewall/rules/:rule_id
```

### Toggle Firewall Rule

Enable or disable a rule without deleting it.

```http
PUT /firewall/rules/:rule_id/toggle
```

**Request Body:**
```json
{
  "enabled": false
}
```

### Get Container Rules

Get all firewall rules for a specific container.

```http
GET /firewall/rules/container/:container_id
```

**Response:**
```json
{
  "rules": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "container_id": "my-container",
      "source_ip": "192.168.1.100",
      "protocol": "tcp",
      "action": "drop",
      "enabled": true
    }
  ]
}
```

### Enable DDoS Protection

Configure DDoS protection for a container.

```http
POST /firewall/ddos/:container_id
```

**Request Body:**
```json
{
  "enabled": true,
  "syn_flood_protection": true,
  "connection_limit": 100,
  "rate_limit": {
    "requests": 1000,
    "per_seconds": 60
  }
}
```

**Fields:**
- `enabled` (required): Enable/disable DDoS protection
- `syn_flood_protection` (required): Protect against SYN flood attacks
- `connection_limit` (optional): Maximum concurrent connections
- `rate_limit` (optional): Global rate limit for all traffic

**DDoS Protection Features:**
- **SYN Flood Protection**: Limits SYN packets to 10/s with burst of 20
- **Connection Limiting**: Rejects connections exceeding the limit
- **Rate Limiting**: Drops packets exceeding the rate limit

### Cleanup Container Firewall

Remove all firewall rules and network for a container.

```http
DELETE /firewall/cleanup/:container_id
```

## Usage Examples

### Block Specific IP

```bash
curl -X POST http://localhost:8070/firewall/rules \
  -H "Authorization: Bearer lightd_your_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{
    "container_id": "my-container",
    "source_ip": "192.168.1.100",
    "protocol": "all",
    "action": "drop",
    "description": "Block malicious IP"
  }'
```

### Rate Limit HTTP Traffic

```bash
curl -X POST http://localhost:8070/firewall/rules \
  -H "Authorization: Bearer lightd_your_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{
    "container_id": "my-container",
    "dest_port": 80,
    "protocol": "tcp",
    "action": "accept",
    "rate_limit": {
      "requests": 100,
      "per_seconds": 60
    },
    "description": "Rate limit HTTP to 100 req/min"
  }'
```

### Allow SSH from Internal Network

```bash
curl -X POST http://localhost:8070/firewall/rules \
  -H "Authorization: Bearer lightd_your_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{
    "container_id": "my-container",
    "source_ip": "10.0.0.0/8",
    "dest_port": 22,
    "protocol": "tcp",
    "action": "accept",
    "description": "Allow SSH from internal network"
  }'
```

### Enable Full DDoS Protection

```bash
curl -X POST http://localhost:8070/firewall/ddos/my-container \
  -H "Authorization: Bearer lightd_your_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{
    "enabled": true,
    "syn_flood_protection": true,
    "connection_limit": 100,
    "rate_limit": {
      "requests": 1000,
      "per_seconds": 60
    }
  }'
```

## Rule Actions

- **accept**: Allow the traffic through
- **drop**: Silently drop the traffic (no response)
- **reject**: Reject the traffic with TCP reset or ICMP error

## Protocol Types

- **tcp**: TCP protocol only
- **udp**: UDP protocol only
- **icmp**: ICMP protocol (ping, etc.)
- **all**: All protocols

## Rate Limiting

Rate limits can be applied to individual rules or globally via DDoS protection.

Format:
```json
{
  "requests": 100,
  "per_seconds": 60
}
```

This allows 100 requests per 60 seconds. Packets exceeding this rate are dropped.

## Best Practices

1. **Create Network First**: Always create the isolated network before adding rules
2. **Default Deny**: Start with restrictive rules and open up as needed
3. **Use CIDR Notation**: For IP ranges, use CIDR (e.g., "10.0.0.0/8")
4. **Enable DDoS Protection**: Always enable for public-facing containers
5. **Monitor Rules**: Regularly review and update firewall rules
6. **Cleanup on Delete**: Always cleanup firewall when removing containers

## Testing

Run the firewall test:

```bash
node tests/firewall.js
```

This will:
1. Create an isolated network
2. Add various firewall rules
3. Enable DDoS protection
4. Toggle and delete rules
5. Cleanup everything

## Technical Details

### Network Isolation

Each container gets a Docker bridge network named `lightd-net-{container_id}`. This network is completely isolated from:
- Host network interfaces
- Other container networks
- Default Docker bridge

### iptables Chains

Firewall rules are organized in iptables chains:
- `LIGHTD-{CONTAINER_ID}`: Main chain for container rules
- `LIGHTD-SYN-{NETWORK}`: SYN flood protection chain
- `LIGHTD-CONN-{NETWORK}`: Connection limiting chain
- `LIGHTD-RATE-{NETWORK}`: Rate limiting chain

### Persistence

All firewall rules and DDoS configurations are stored in `storage/firewall.db` using sled database. Rules are automatically reloaded on daemon restart.

## Limitations

- Requires Linux with iptables support
- Requires Docker with bridge network support
- Root/sudo access may be required for iptables commands
- Rules apply at network level, not application level

## Security Considerations

- Firewall rules are enforced by the Linux kernel
- Container cannot bypass its own firewall rules
- Host network is completely isolated from container networks
- All iptables commands are validated before execution
- No direct host network access is possible
