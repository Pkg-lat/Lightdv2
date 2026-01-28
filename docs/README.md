# Lightd v2 API Documentation

Lightd is a container management daemon that provides a REST API and WebSocket interface for managing Docker containers, volumes, and network resources.

## Table of Contents

1. [Authentication](#authentication)
2. [Public Routes](#public-routes)
3. [Volume Management](#volume-management)
4. [Network Pool Management](#network-pool-management)
5. [Container Management](#container-management)
6. [WebSocket Interface](#websocket-interface)
7. [Remote API Sync](#remote-api-sync)
8. [Error Handling](#error-handling)

## Base URL

Default: `http://localhost:8070`

## Documentation Files

- [Authentication Guide](./authentication.md) - Token management and auth middleware
- [Volume API](./volumes.md) - File system and volume operations
- [Network API](./network.md) - Network pool and port management
- [Container API](./containers.md) - Container lifecycle and management
- [WebSocket API](./websocket.md) - Real-time container monitoring
- [Remote Sync API](./remote.md) - Remote server synchronization

## Quick Start

### 1. Generate an API Token

```bash
# View current token
./lightd --token what

# Generate new random token
./lightd --token reset

# Set custom token
./lightd --token set
```

### 2. Make Your First Request

```bash
curl -X GET http://localhost:8070/api/v1/public/ping \
  -H "Accept: Application/vnd.pkglatv1+json"
```

### 3. Create a Volume

```bash
curl -X POST http://localhost:8070/volumes \
  -H "Authorization: Bearer lightd_your-token-here" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json"
```

### 4. Create a Container

```bash
curl -X POST http://localhost:8070/containers \
  -H "Authorization: Bearer lightd_your-token-here" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{
    "internal_id": "my-server-001",
    "image": "ubuntu:22.04",
    "volume_id": "your-volume-id",
    "startup_command": "bash -c \"while true; do echo Hello; sleep 1; done\"",
    "ports": [],
    "limits": {
      "memory": 536870912,
      "cpu": 1.0
    }
  }'
```

### 5. Connect via WebSocket

```javascript
const ws = new WebSocket('ws://localhost:8070/ws/my-server-001?token=lightd_your-token-here');

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log('Event:', data.event, data);
};

// Send a command
ws.send(JSON.stringify({
  event: 'send_command',
  command: 'echo "Hello from WebSocket"\n'
}));
```

## Common Headers

All protected routes require these headers:

```
Authorization: Bearer lightd_<your-token>
Accept: Application/vnd.pkglatv1+json
```

## Response Format

### Success Response

```json
{
  "success": true,
  "data": { ... }
}
```

### Error Response

```json
{
  "error": "Error message description"
}
```

## Status Codes

- `200 OK` - Request succeeded
- `201 Created` - Resource created successfully
- `400 Bad Request` - Invalid request parameters
- `401 Unauthorized` - Missing or invalid authentication
- `403 Forbidden` - Invalid vendor header or origin
- `404 Not Found` - Resource not found
- `500 Internal Server Error` - Server error

## Next Steps

- Read the [Authentication Guide](./authentication.md) to understand token management
- Explore [Container API](./containers.md) for full container lifecycle management
- Check [WebSocket API](./websocket.md) for real-time monitoring
- Review [Remote Sync API](./remote.md) for multi-server setups
