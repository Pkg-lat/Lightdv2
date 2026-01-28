# Container Management API

Containers are Docker containers managed by Lightd with lifecycle tracking and state management.

## Base Path

All container routes are prefixed with `/containers`

## Container States

- `installing` - Container is being created and installed
- `ready` - Container is ready to start
- `failed` - Installation or operation failed

## Create Container

**Endpoint:** `POST /containers`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Accept: Application/vnd.pkglatv1+json
Content-Type: application/json
```

**Request Body:**
```json
{
  "internal_id": "my-server-001",
  "image": "ubuntu:22.04",
  "volume_id": "d6764075-c5f1-4045-9fb3-85315b85cb0f",
  "startup_command": "bash -c 'while true; do echo Hello; sleep 1; done'",
  "start_pattern": "Hello",
  "ports": [
    {
      "ip": "0.0.0.0",
      "port": 25565,
      "protocol": "tcp"
    }
  ],
  "limits": {
    "memory": 536870912,
    "cpu": 1.0
  },
  "mount": {
    "/custom/path": "/host/path"
  },
  "install_script": "#!/bin/bash\napt-get update\napt-get install -y curl"
}
```

**Field Descriptions:**
- `internal_id` (required) - Unique identifier for the container
- `image` (required) - Docker image (e.g., `ubuntu:22.04`, `node:18`)
- `volume_id` (required) - Volume UUID for persistent storage
- `startup_command` (required) - Command to run when container starts
- `start_pattern` (optional) - Regex pattern to detect when server is ready
- `ports` (optional) - Array of port bindings
- `limits` (optional) - Resource limits
  - `memory` - Memory in bytes (e.g., 536870912 = 512MB)
  - `cpu` - CPU cores (e.g., 1.0 = 1 core, 0.5 = half core)
- `mount` (optional) - Custom volume mounts
- `install_script` (optional) - Script to run during installation

**Response:**
```json
{
  "message": "Container creation started",
  "internal_id": "my-server-001",
  "state": "installing"
}
```


## Get Container State

**Endpoint:** `GET /containers/:internal_id`

**Response:**
```json
{
  "internal_id": "my-server-001",
  "container_id": "a1b2c3d4e5f6",
  "volume_id": "d6764075-c5f1-4045-9fb3-85315b85cb0f",
  "image": "ubuntu:22.04",
  "startup_command": "bash -c 'while true; do echo Hello; sleep 1; done'",
  "start_pattern": "Hello",
  "install_state": "ready",
  "ports": [
    {
      "ip": "0.0.0.0",
      "port": 25565,
      "protocol": "tcp"
    }
  ],
  "limits": {
    "memory": 536870912,
    "cpu": 1.0
  },
  "mount": {},
  "created_at": 1706450000,
  "updated_at": 1706450100
}
```

## List All Containers

**Endpoint:** `GET /containers`

**Response:**
```json
{
  "containers": [
    {
      "internal_id": "my-server-001",
      "container_id": "a1b2c3d4e5f6",
      "install_state": "ready",
      "image": "ubuntu:22.04"
    }
  ]
}
```

## Delete Container

**Endpoint:** `DELETE /containers/:internal_id`

**Response:**
```json
{
  "message": "Container deleted successfully"
}
```

**Note:** This removes the container from Docker and deletes the database entry.

## Power Actions

### Start Container

**Endpoint:** `POST /containers/:internal_id/start`

**Response:**
```json
{
  "message": "Container start initiated",
  "internal_id": "my-server-001"
}
```

**Events:** Broadcasts `starting` event via WebSocket. When `start_pattern` matches in logs, broadcasts `running` event.

### Kill Container

**Endpoint:** `POST /containers/:internal_id/kill`

**Response:**
```json
{
  "message": "Container kill initiated",
  "internal_id": "my-server-001"
}
```

**Note:** Uses SIGKILL for instant termination.

### Restart Container

**Endpoint:** `POST /containers/:internal_id/restart`

**Response:**
```json
{
  "message": "Container restart initiated",
  "internal_id": "my-server-001"
}
```

## Reinstall Container

**Endpoint:** `POST /containers/:internal_id/reinstall`

**Request Body:**
```json
{
  "image": "ubuntu:22.04",
  "install_script": "#!/bin/bash\napt-get update"
}
```

**Response:**
```json
{
  "message": "Container reinstall started",
  "internal_id": "my-server-001"
}
```

**Note:** Removes old Docker container and creates new one. Volume data is preserved.

## Rebind Network

**Endpoint:** `POST /containers/:internal_id/rebind-network`

**Request Body:**
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

**Response:**
```json
{
  "message": "Network rebinding started",
  "internal_id": "my-server-001"
}
```

**Process:**
1. Validates new port bindings
2. Removes old Docker container
3. Creates new container with new ports
4. Updates database
5. Old ports marked as available, new ports marked as in use

**Timeout Settings:**
- Database operations: 5 seconds
- Container removal: 30 seconds
- Container creation: 60 seconds

## Container Lifecycle Events

Monitor via WebSocket (see [WebSocket API](./websocket.md)):

1. `Started` - Installation started
2. `CreatingContainer` - Creating Docker container
3. `ContainerCreated` - Docker container created
4. `RunningInstallScript` - Running install script (if provided)
5. `InstallScriptComplete` - Install script finished
6. `SettingUpEntrypoint` - Setting up entrypoint.sh
7. `Ready` - Container ready to start
8. `Error` - Installation failed

## Volume Mounts

Every container has these mounts:
- `/home/container` → Volume storage (persistent)
- `/app/data` → Container data directory (entrypoint.sh, install.sh)

Custom mounts can be added via the `mount` field.

## Startup Pattern Detection

The `start_pattern` field accepts regex patterns to detect when a server is ready:

**Example patterns:**
- `"Server started"` - Exact match
- `"Hello"` - Simple substring
- `"Server.*ready"` - Regex pattern
- `"Listening on port \\d+"` - Port detection

When the pattern matches in console output, container state transitions from `starting` to `running`.

## Resource Limits

**Memory:**
- Specified in bytes
- Example: `536870912` = 512MB, `1073741824` = 1GB

**CPU:**
- Specified as number of cores
- Example: `1.0` = 1 core, `0.5` = half core, `2.0` = 2 cores

## Error Responses

**Container Not Found:**
```json
{
  "error": "Container not found"
}
```

**Container Installing:**
```json
{
  "error": "Cannot rebind network while container is installing"
}
```

**Invalid Port Binding:**
```json
{
  "error": "Port not found in network pool"
}
```
