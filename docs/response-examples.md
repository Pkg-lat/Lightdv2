# API Response Examples

Complete examples of all API responses for reference.

## Public Routes

### Health Check
**Request:** `GET /api/v1/public/ping`

**Response (200):**
```json
{
  "status": "ok",
  "message": "Lightd is running"
}
```

## Authentication

### Generate Token
**Request:** `POST /auth/tokens`
```json
{
  "ttl": "1h",
  "remove_on_use": false
}
```

**Response (200):**
```json
{
  "token": "lightd_f3a8b9c2d1e4f5a6b7c8d9e0f1a2b3c4",
  "expires_at": "2026-01-28T15:30:00Z"
}
```

## Volume Management

### Create Volume
**Request:** `POST /volumes`

**Response (201):**
```json
{
  "volume_id": "d6764075-c5f1-4045-9fb3-85315b85cb0f",
  "path": "/storage/volumes/d6764075-c5f1-4045-9fb3-85315b85cb0f"
}
```

### Write File
**Request:** `POST /volumes/d6764075/files`
```json
{
  "path": "config/server.properties",
  "content": "server-port=25565\nmax-players=20"
}
```

**Response (200):**
```json
{
  "message": "File written successfully",
  "path": "config/server.properties"
}
```

### Create Folder
**Request:** `POST /volumes/d6764075/folders`
```json
{
  "path": "plugins/MyPlugin"
}
```

**Response (200):**
```json
{
  "message": "Folder created successfully",
  "path": "plugins/MyPlugin"
}
```

### Copy File/Folder
**Request:** `POST /volumes/d6764075/copy`
```json
{
  "source": "plugins",
  "destination": "backup/plugins"
}
```

**Response (200):**
```json
{
  "message": "Copied successfully",
  "source": "plugins",
  "destination": "backup/plugins"
}
```

### List Files
**Request:** `GET /volumes/d6764075/files?path=plugins`

**Response (200):**
```json
{
  "files": [
    {
      "name": "MyPlugin",
      "type": "directory",
      "size": 0
    },
    {
      "name": "config.yml",
      "type": "file",
      "size": 1024
    }
  ]
}
```


### Compress Volume
**Request:** `POST /volumes/d6764075/compress`
```json
{
  "format": "tar.gz",
  "output_name": "backup"
}
```

**Response (200):**
```json
{
  "message": "Compression successful",
  "archive_path": "backup.tar.gz",
  "size": 2048576
}
```

### Decompress Archive
**Request:** `POST /volumes/d6764075/decompress`
```json
{
  "archive_path": "backup.tar.gz",
  "destination": "restored"
}
```

**Response (200):**
```json
{
  "message": "Decompression successful",
  "destination": "restored"
}
```

## Network Management

### Add Port
**Request:** `POST /network/ports`
```json
{
  "ip": "0.0.0.0",
  "port": 25565,
  "protocol": "tcp"
}
```

**Response (200):**
```json
{
  "message": "Port added to pool",
  "ip": "0.0.0.0",
  "port": 25565,
  "protocol": "tcp"
}
```

### Get Port
**Request:** `GET /network/ports/0.0.0.0/25565`

**Response (200):**
```json
{
  "ip": "0.0.0.0",
  "port": 25565,
  "protocol": "tcp",
  "in_use": false
}
```

### List All Ports
**Request:** `GET /network/ports`

**Response (200):**
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
    },
    {
      "ip": "0.0.0.0",
      "port": 19132,
      "protocol": "udp",
      "in_use": false
    }
  ]
}
```

### Get Random Available Port
**Request:** `GET /network/ports/random`

**Response (200):**
```json
{
  "ip": "0.0.0.0",
  "port": 25567,
  "protocol": "tcp",
  "in_use": false
}
```

### Mark Port In Use
**Request:** `PUT /network/ports/0.0.0.0/25565/use`
```json
{
  "in_use": true
}
```

**Response (200):**
```json
{
  "message": "Port usage updated",
  "ip": "0.0.0.0",
  "port": 25565,
  "in_use": true
}
```

### Delete Port
**Request:** `DELETE /network/ports/0.0.0.0/25565`

**Response (200):**
```json
{
  "message": "Port deleted from pool"
}
```

### Bulk Delete Ports
**Request:** `POST /network/ports/bulk-delete`
```json
{
  "ports": [
    {"ip": "0.0.0.0", "port": 25565},
    {"ip": "0.0.0.0", "port": 25566}
  ]
}
```

**Response (200):**
```json
{
  "message": "Deleted 2 ports from pool",
  "deleted_count": 2
}
```

## Container Management

### Create Container
**Request:** `POST /containers`
```json
{
  "internal_id": "minecraft-001",
  "image": "openjdk:17-slim",
  "volume_id": "d6764075-c5f1-4045-9fb3-85315b85cb0f",
  "startup_command": "java -Xmx1G -jar server.jar nogui",
  "start_pattern": "Done.*For help",
  "ports": [
    {
      "ip": "0.0.0.0",
      "port": 25565,
      "protocol": "tcp"
    }
  ],
  "limits": {
    "memory": 1073741824,
    "cpu": 2.0
  },
  "mount": {
    "/custom/path": "/host/path"
  },
  "install_script": "#!/bin/bash\nwget https://example.com/server.jar"
}
```

**Response (200):**
```json
{
  "message": "Container creation started",
  "internal_id": "minecraft-001",
  "state": "installing"
}
```

### Get Container State
**Request:** `GET /containers/minecraft-001`

**Response (200):**
```json
{
  "internal_id": "minecraft-001",
  "container_id": "a1b2c3d4e5f6789",
  "volume_id": "d6764075-c5f1-4045-9fb3-85315b85cb0f",
  "image": "openjdk:17-slim",
  "startup_command": "java -Xmx1G -jar server.jar nogui",
  "start_pattern": "Done.*For help",
  "install_state": "ready",
  "ports": [
    {
      "ip": "0.0.0.0",
      "port": 25565,
      "protocol": "tcp"
    }
  ],
  "limits": {
    "memory": 1073741824,
    "cpu": 2.0
  },
  "mount": {
    "/custom/path": "/host/path"
  },
  "created_at": 1706450000,
  "updated_at": 1706450100
}
```

### List All Containers
**Request:** `GET /containers`

**Response (200):**
```json
{
  "containers": [
    {
      "internal_id": "minecraft-001",
      "container_id": "a1b2c3d4e5f6789",
      "install_state": "ready",
      "image": "openjdk:17-slim",
      "ports": [
        {
          "ip": "0.0.0.0",
          "port": 25565,
          "protocol": "tcp"
        }
      ]
    },
    {
      "internal_id": "web-server-001",
      "container_id": "b2c3d4e5f6a7890",
      "install_state": "ready",
      "image": "nginx:latest",
      "ports": [
        {
          "ip": "0.0.0.0",
          "port": 8080,
          "protocol": "tcp"
        }
      ]
    }
  ]
}
```

### Delete Container
**Request:** `DELETE /containers/minecraft-001`

**Response (200):**
```json
{
  "message": "Container deleted successfully"
}
```

### Start Container
**Request:** `POST /containers/minecraft-001/start`

**Response (200):**
```json
{
  "message": "Container start initiated",
  "internal_id": "minecraft-001"
}
```

### Kill Container
**Request:** `POST /containers/minecraft-001/kill`

**Response (200):**
```json
{
  "message": "Container kill initiated",
  "internal_id": "minecraft-001"
}
```

### Restart Container
**Request:** `POST /containers/minecraft-001/restart`

**Response (200):**
```json
{
  "message": "Container restart initiated",
  "internal_id": "minecraft-001"
}
```

### Reinstall Container
**Request:** `POST /containers/minecraft-001/reinstall`
```json
{
  "image": "openjdk:17-slim",
  "install_script": "#!/bin/bash\nwget https://example.com/server.jar\necho 'eula=true' > eula.txt"
}
```

**Response (200):**
```json
{
  "message": "Container reinstall started",
  "internal_id": "minecraft-001"
}
```

### Rebind Network
**Request:** `POST /containers/minecraft-001/rebind-network`
```json
{
  "ports": [
    {
      "ip": "0.0.0.0",
      "port": 25566,
      "protocol": "tcp"
    }
  ]
}
```

**Response (200):**
```json
{
  "message": "Network rebinding started",
  "internal_id": "minecraft-001"
}
```

## Remote API

### Get Config
**Request:** `GET /remote/config`

**Response (200):**
```json
{
  "version": "0.1.0",
  "server": {
    "host": "0.0.0.0",
    "port": 8070
  },
  "authorization": {
    "enabled": true,
    "token": "lightd_ad2f7fc49ed640429c450e14ed07c8d5"
  },
  "docker": {
    "socket_path": "/var/run/docker.sock"
  },
  "storage": {
    "base_path": "/storage",
    "containers_path": "/storage/containers",
    "volumes_path": "/storage/volumes"
  },
  "remote": {
    "enabled": true,
    "url": "https://remote-api.com/api",
    "token": "lightd_ad2f7fc49ed640429c450e14ed07c8d5"
  }
}
```

### Reload Config
**Request:** `POST /remote/config/reload`

**Response (200):**
```json
{
  "message": "Configuration reloaded successfully"
}
```

## WebSocket Events

### Stats Event
```json
{
  "event": "stats",
  "cpu_usage": 25.5,
  "memory_usage": 536870912,
  "memory_limit": 1073741824,
  "network_rx": 1024000,
  "network_tx": 2048000,
  "block_read": 4096000,
  "block_write": 8192000
}
```

### Console Output
```json
{
  "event": "console",
  "data": "[Server] Starting Minecraft server version 1.20.4\n"
}
```

### Console Duplicate
```json
{
  "event": "console duplicate",
  "data": "[Server] Starting Minecraft server version 1.20.4\n"
}
```

### State Events
```json
{
  "event": "event",
  "data": "installing"
}
```

```json
{
  "event": "event",
  "data": "installed"
}
```

```json
{
  "event": "event",
  "data": "starting"
}
```

```json
{
  "event": "event",
  "data": "running"
}
```

```json
{
  "event": "event",
  "data": "stopping"
}
```

```json
{
  "event": "event",
  "data": "exit"
}
```

### Daemon Messages
```json
{
  "event": "daemon_message",
  "data": "Container started"
}
```

```json
{
  "event": "daemon_message",
  "data": "Server started"
}
```

```json
{
  "event": "daemon_message",
  "data": "Container stopped"
}
```

```json
{
  "event": "daemon_message",
  "data": "Error: Docker container creation failed"
}
```

### Historical Logs
```json
{
  "event": "logs",
  "data": "[10:30:15] Starting server...\n[10:30:16] Loading world...\n[10:30:20] Done! For help, type \"help\"\n"
}
```

## Error Responses

### Unauthorized (401)
```json
{
  "error": "Missing or invalid authorization token"
}
```

### Forbidden (403)
```json
{
  "error": "Invalid vendor header"
}
```

### Not Found (404)
```json
{
  "error": "Container not found"
}
```

```json
{
  "error": "Volume not found"
}
```

```json
{
  "error": "Port not found in pool"
}
```

### Bad Request (400)
```json
{
  "error": "Invalid path: path traversal detected"
}
```

```json
{
  "error": "Cannot rebind network while container is installing"
}
```

```json
{
  "error": "Port already exists in pool"
}
```

```json
{
  "error": "Invalid port binding: port not in network pool"
}
```

### Internal Server Error (500)
```json
{
  "error": "Failed to create Docker container: image not found"
}
```

```json
{
  "error": "Database operation failed"
}
```

```json
{
  "error": "Failed to write file: permission denied"
}
```
