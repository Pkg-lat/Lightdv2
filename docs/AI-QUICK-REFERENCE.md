# Lightd API - AI Quick Reference

This guide provides essential information for AI agents to interact with Lightd.

## Authentication

**All requests require:**
```http
Authorization: Bearer lightd_<token>
Accept: Application/vnd.pkglatv1+json
```

**Get token from config.json or use CLI:**
```bash
./lightd --token what
```

## Common Workflow

### 1. Create Volume
```bash
curl -X POST http://localhost:8070/volumes \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json"
```
**Returns:** `{"volume_id": "uuid", "path": "/path/to/volume"}`

### 2. Write Files to Volume
```bash
curl -X POST http://localhost:8070/volumes/{volume_id}/files \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{"path": "config.yml", "content": "server-port: 25565"}'
```

### 3. Add Network Port
```bash
curl -X POST http://localhost:8070/network/ports \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{"ip": "0.0.0.0", "port": 25565, "protocol": "tcp"}'
```

### 4. Create Container
```bash
curl -X POST http://localhost:8070/containers \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{
    "internal_id": "server-001",
    "image": "ubuntu:22.04",
    "volume_id": "volume-uuid",
    "startup_command": "bash -c \"while true; do echo Ready; sleep 1; done\"",
    "start_pattern": "Ready",
    "ports": [{"ip": "0.0.0.0", "port": 25565, "protocol": "tcp"}],
    "limits": {"memory": 536870912, "cpu": 1.0}
  }'
```

### 5. Monitor via WebSocket
```javascript
const ws = new WebSocket('ws://localhost:8070/ws/server-001?token=lightd_token');
ws.onmessage = (e) => console.log(JSON.parse(e.data));
```

### 6. Start Container
```bash
curl -X POST http://localhost:8070/containers/server-001/start \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json"
```


## API Endpoints Summary

### Public (No Auth)
- `GET /api/v1/public/ping` - Health check

### Authentication
- `POST /auth/tokens` - Generate temporary token

### Volumes
- `POST /volumes` - Create volume
- `POST /volumes/:id/files` - Write file
- `POST /volumes/:id/folders` - Create folder
- `POST /volumes/:id/copy` - Copy file/folder
- `GET /volumes/:id/files` - List files
- `POST /volumes/:id/compress` - Compress to archive
- `POST /volumes/:id/decompress` - Extract archive

### Network
- `POST /network/ports` - Add port
- `GET /network/ports` - List all ports
- `GET /network/ports/:ip/:port` - Get specific port
- `GET /network/ports/random` - Get random available port
- `PUT /network/ports/:ip/:port/use` - Mark port in use
- `DELETE /network/ports/:ip/:port` - Delete port
- `POST /network/ports/bulk-delete` - Delete multiple ports

### Containers
- `POST /containers` - Create container
- `GET /containers` - List all containers
- `GET /containers/:id` - Get container state
- `DELETE /containers/:id` - Delete container
- `POST /containers/:id/start` - Start container
- `POST /containers/:id/kill` - Kill container
- `POST /containers/:id/restart` - Restart container
- `POST /containers/:id/reinstall` - Reinstall container
- `POST /containers/:id/rebind-network` - Change port bindings

### WebSocket
- `ws://host/ws/:id?token=<token>` - Connect to container

### Remote
- `GET /remote/config` - Get current config
- `POST /remote/config/reload` - Reload config

## Key Concepts

### Container States
- `installing` - Being created
- `ready` - Ready to start
- `failed` - Installation failed

### Container Lifecycle
1. Create container → `installing`
2. Docker container created
3. Install script runs (if provided)
4. Entrypoint configured
5. State → `ready`
6. Start container → `starting`
7. Pattern matches in logs → `running`

### Resource Limits
- **Memory:** Bytes (536870912 = 512MB)
- **CPU:** Cores (1.0 = 1 core, 0.5 = half core)

### Port Protocols
- `tcp` (default)
- `udp`

### Archive Formats
- `zip` - ZIP archive
- `tar` - TAR archive
- `tar.gz` - Gzipped TAR
- `tar.bz2` - Bzip2 TAR

## WebSocket Events

### Outbound (Server → Client)
- `stats` - CPU, memory, network, disk stats
- `console` - Console output
- `event` - State changes (installing, ready, starting, running, stopping, exit)
- `daemon_message` - System messages
- `logs` - Historical logs

### Inbound (Client → Server)
- `send_command` - Execute command
- `power` - Power action (start, kill, restart)
- `request_logs` - Request historical logs

## Error Responses

All errors return:
```json
{
  "error": "Error message"
}
```

Common errors:
- `401` - Missing/invalid token
- `403` - Invalid vendor header
- `404` - Resource not found
- `500` - Server error

## Important Notes

1. **Path Security:** All file paths are validated to prevent traversal attacks
2. **Async Operations:** Container operations are non-blocking (use WebSocket to monitor)
3. **Token Format:** Must start with `lightd_` and be at least 20 characters
4. **Vendor Header:** Required for all protected routes
5. **Port Management:** Ports must be added to pool before use in containers
6. **Volume Mounts:** Containers automatically mount volume to `/home/container`
7. **Start Pattern:** Use regex to detect when server is ready
8. **Install Scripts:** Optional bash scripts run during container creation
9. **Network Rebinding:** Recreates container with new ports (preserves volume data)
10. **Remote Sync:** Automatically sends status updates if enabled

## Example: Complete Server Setup

```bash
# 1. Create volume
VOLUME=$(curl -X POST http://localhost:8070/volumes \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" | jq -r '.volume_id')

# 2. Write config file
curl -X POST http://localhost:8070/volumes/$VOLUME/files \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{"path": "server.properties", "content": "server-port=25565"}'

# 3. Add port to pool
curl -X POST http://localhost:8070/network/ports \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d '{"ip": "0.0.0.0", "port": 25565, "protocol": "tcp"}'

# 4. Create container
curl -X POST http://localhost:8070/containers \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json" \
  -H "Content-Type: application/json" \
  -d "{
    \"internal_id\": \"minecraft-001\",
    \"image\": \"openjdk:17-slim\",
    \"volume_id\": \"$VOLUME\",
    \"startup_command\": \"java -Xmx1G -jar server.jar nogui\",
    \"start_pattern\": \"Done\",
    \"ports\": [{\"ip\": \"0.0.0.0\", \"port\": 25565, \"protocol\": \"tcp\"}],
    \"limits\": {\"memory\": 1073741824, \"cpu\": 2.0},
    \"install_script\": \"#!/bin/bash\\nwget https://example.com/server.jar\"
  }"

# 5. Wait for installation (monitor via WebSocket or poll state)
# 6. Start container
curl -X POST http://localhost:8070/containers/minecraft-001/start \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json"
```

## Debugging Tips

1. **Check container state:** `GET /containers/:id`
2. **Monitor WebSocket:** Connect and watch events
3. **View logs:** Send `request_logs` via WebSocket
4. **Check Docker:** Containers named `lightd-{internal_id}`
5. **Verify token:** Use `./lightd --token what`
6. **Test health:** `GET /api/v1/public/ping`

## Remote Sync Setup

**config.json:**
```json
{
  "remote": {
    "enabled": true,
    "url": "https://remote-api.com/api",
    "token": "lightd_same_token_as_lightd"
  }
}
```

**Remote must implement:**
- `GET /health` → `{"status": 200, "endpoint": "active"}`
- `POST /update` → Receive status/error updates

**Updates sent:**
- Container starts installing → `{"event": "update", "server": "id", "status": "installing"}`
- Container ready → `{"event": "update", "server": "id", "status": "ready"}`
- Container fails → `{"event": "update", "server": "id", "error": "failed", "data": "details"}`
