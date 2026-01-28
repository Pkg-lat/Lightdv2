# Network Pool API

The network pool manages IP:port combinations for container port bindings.

## Base Path

All network routes are prefixed with `/network`

## Add Port to Pool

**Endpoint:** `POST /network/ports`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Accept: Application/vnd.pkglatv1+json
Content-Type: application/json
```

**Request Body:**
```json
{
  "ip": "0.0.0.0",
  "port": 25565,
  "protocol": "tcp"
}
```

**Protocol Options:**
- `tcp` (default)
- `udp`

**Response:**
```json
{
  "message": "Port added to pool",
  "ip": "0.0.0.0",
  "port": 25565,
  "protocol": "tcp"
}
```

## Get Specific Port

**Endpoint:** `GET /network/ports/:ip/:port`

**Example:**
```bash
curl -X GET http://localhost:8070/network/ports/0.0.0.0/25565 \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json"
```

**Response:**
```json
{
  "ip": "0.0.0.0",
  "port": 25565,
  "protocol": "tcp",
  "in_use": false
}
```

## List All Ports

**Endpoint:** `GET /network/ports`

**Response:**
```json
{
  "ports": [
    {
      "ip": "0.0.0.0",
      "port": 25565,
      "protocol": "tcp",
      "in_use": false
    },
    {
      "ip": "0.0.0.0",
      "port": 25566,
      "protocol": "tcp",
      "in_use": true
    }
  ]
}
```


## Get Random Available Port

**Endpoint:** `GET /network/ports/random`

**Response:**
```json
{
  "ip": "0.0.0.0",
  "port": 25567,
  "protocol": "tcp",
  "in_use": false
}
```

**Note:** This does NOT mark the port as in use. Use this to find available ports.

## Mark Port as In Use

**Endpoint:** `PUT /network/ports/:ip/:port/use`

**Request Body:**
```json
{
  "in_use": true
}
```

**Response:**
```json
{
  "message": "Port usage updated",
  "ip": "0.0.0.0",
  "port": 25565,
  "in_use": true
}
```

## Delete Port

**Endpoint:** `DELETE /network/ports/:ip/:port`

**Response:**
```json
{
  "message": "Port deleted from pool"
}
```

## Bulk Delete Ports

**Endpoint:** `POST /network/ports/bulk-delete`

**Request Body:**
```json
{
  "ports": [
    {"ip": "0.0.0.0", "port": 25565},
    {"ip": "0.0.0.0", "port": 25566}
  ]
}
```

**Response:**
```json
{
  "message": "Deleted 2 ports from pool",
  "deleted_count": 2
}
```

## iptables Integration (Unix Only)

On Unix systems, Lightd automatically manages iptables rules when ports are added:

```bash
# TCP port
iptables -A INPUT -p tcp --dport 25565 -j ACCEPT

# UDP port
iptables -A INPUT -p udp --dport 25566 -j ACCEPT
```

**Note:** Requires root/sudo privileges for iptables commands.

## Port Binding Workflow

1. **Add ports to pool:**
```bash
curl -X POST http://localhost:8070/network/ports \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{"ip": "0.0.0.0", "port": 25565, "protocol": "tcp"}'
```

2. **Create container with port bindings:**
```bash
curl -X POST http://localhost:8070/containers \
  -d '{
    "internal_id": "server-001",
    "ports": [
      {"ip": "0.0.0.0", "port": 25565, "protocol": "tcp"}
    ]
  }'
```

3. **Port is automatically marked as in_use: true**

4. **Rebind network if needed:**
```bash
curl -X POST http://localhost:8070/containers/server-001/rebind-network \
  -d '{
    "ports": [
      {"ip": "0.0.0.0", "port": 25566, "protocol": "tcp"}
    ]
  }'
```

## Error Responses

**Port Already Exists:**
```json
{
  "error": "Port already exists in pool"
}
```

**Port Not Found:**
```json
{
  "error": "Port not found in pool"
}
```

**No Available Ports:**
```json
{
  "error": "No available ports in pool"
}
```
