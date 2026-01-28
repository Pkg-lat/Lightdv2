# Remote API Sync

Lightd can sync container status and events to a remote management server.

## Configuration

Enable remote sync in `config.json`:

```json
{
  "remote": {
    "enabled": true,
    "url": "https://your-remote-api.com/api",
    "token": "lightd_ad2f7fc49ed640429c450e14ed07c8d5"
  }
}
```

**Fields:**
- `enabled` - Enable/disable remote sync
- `url` - Remote API base URL
- `token` - Authentication token (must match remote server)

**Important:** Remote and Lightd must use the same token for authentication.

## Remote Server Requirements

Your remote server must implement these endpoints:

### Health Check

**Endpoint:** `GET /health`

**Headers:**
```http
Authorization: Bearer lightd_<token>
```

**Expected Response:**
```json
{
  "status": 200,
  "endpoint": "active"
}
```

**Purpose:** Lightd checks health every 30 seconds to verify remote is accessible.

### Receive Updates

**Endpoint:** `POST /update`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Content-Type: application/json
```

**Request Body (Status Update):**
```json
{
  "event": "update",
  "server": "my-server-001",
  "status": "installing"
}
```

**Request Body (Error Update):**
```json
{
  "event": "update",
  "server": "my-server-001",
  "error": "failed",
  "data": "Docker container creation failed: image not found"
}
```

**Status Values:**
- `installing` - Container is being installed
- `ready` - Container installation complete and ready
- `failed` - Container installation or operation failed

**Error Values:**
- `failed` - General failure
- `timeout` - Operation timed out
- Custom error messages

**Expected Response:**
```json
{
  "success": true
}
```


### Get Config (Optional)

**Endpoint:** `GET /config`

**Headers:**
```http
Authorization: Bearer lightd_<token>
```

**Expected Response:**
```json
{
  "containers": [...],
  "settings": {...}
}
```

**Purpose:** Allows remote to send configuration to Lightd.

### Send Config (Optional)

**Endpoint:** `POST /config`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Content-Type: application/json
```

**Request Body:**
```json
{
  "containers": [...],
  "settings": {...}
}
```

**Purpose:** Allows Lightd to push configuration to remote.

## Event Flow

### Container Installation

```
1. User creates container via Lightd API
2. Lightd → Remote: {"event": "update", "server": "id", "status": "installing"}
3. Container installs...
4. Lightd → Remote: {"event": "update", "server": "id", "status": "ready"}
```

### Container Error

```
1. Container installation fails
2. Lightd → Remote: {"event": "update", "server": "id", "error": "failed", "data": "error details"}
```

### Container Reinstall

```
1. User triggers reinstall
2. Lightd → Remote: {"event": "update", "server": "id", "status": "installing"}
3. Container reinstalls...
4. Lightd → Remote: {"event": "update", "server": "id", "status": "ready"}
```

## Lifecycle Events Synced

These container lifecycle events trigger remote updates:

| Lifecycle Event | Remote Status | Notes |
|----------------|---------------|-------|
| Started | `installing` | Installation started |
| CreatingContainer | `installing` | Docker container being created |
| Ready | `ready` | Container ready to start |
| Error | `failed` | Installation failed |
| ReinstallStarted | `installing` | Reinstall triggered |

## Health Check Loop

Lightd performs health checks every 30 seconds:

```
1. Lightd → Remote: GET /health
2. Remote → Lightd: {"status": 200, "endpoint": "active"}
3. If success: Log "Remote health check: OK"
4. If failure: Log "Remote health check: Failed"
5. Wait 30 seconds
6. Repeat
```

## Error Handling

**Remote Unreachable:**
- Lightd logs error but continues operating
- Updates are lost (no retry queue)
- Health check continues attempting connection

**Authentication Failure:**
- Lightd logs error
- Verify tokens match on both sides

**Timeout:**
- HTTP requests timeout after 10 seconds
- Lightd logs timeout and continues

## Local API Routes

Lightd provides routes to manage remote config:

### Get Current Config

**Endpoint:** `GET /remote/config`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Accept: Application/vnd.pkglatv1+json
```

**Response:**
```json
{
  "version": "0.1.0",
  "server": {...},
  "remote": {
    "enabled": true,
    "url": "https://remote.com/api",
    "token": "lightd_token"
  }
}
```

### Reload Config

**Endpoint:** `POST /remote/config/reload`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Accept: Application/vnd.pkglatv1+json
```

**Response:**
```json
{
  "message": "Configuration reloaded successfully"
}
```

**Note:** Currently reloads from file but doesn't update running application. Restart required for changes.

## Implementation Example (Remote Server)

```javascript
// Express.js example
const express = require('express');
const app = express();

app.use(express.json());

// Middleware to verify token
function verifyToken(req, res, next) {
  const auth = req.headers.authorization;
  if (!auth || !auth.startsWith('Bearer lightd_')) {
    return res.status(401).json({ error: 'Unauthorized' });
  }
  
  const token = auth.substring(7);
  if (token !== process.env.LIGHTD_TOKEN) {
    return res.status(401).json({ error: 'Invalid token' });
  }
  
  next();
}

// Health check
app.get('/health', verifyToken, (req, res) => {
  res.json({
    status: 200,
    endpoint: 'active'
  });
});

// Receive updates
app.post('/update', verifyToken, (req, res) => {
  const { event, server, status, error, data } = req.body;
  
  console.log(`Update from ${server}:`, { status, error, data });
  
  // Update your database
  if (status) {
    updateContainerStatus(server, status);
  }
  
  if (error) {
    logContainerError(server, error, data);
  }
  
  res.json({ success: true });
});

app.listen(3000, () => {
  console.log('Remote API listening on port 3000');
});
```

## Security Considerations

- Use HTTPS for remote URL in production
- Keep tokens secure and rotate regularly
- Validate all incoming data on remote server
- Implement rate limiting on remote endpoints
- Log all sync attempts for audit trail

## Troubleshooting

**Remote not receiving updates:**
1. Check `remote.enabled` is `true` in config.json
2. Verify remote URL is correct
3. Ensure tokens match on both sides
4. Check remote server logs for errors
5. Verify remote `/health` endpoint returns correct response

**Health check failing:**
1. Verify remote server is running
2. Check network connectivity
3. Verify token authentication
4. Check remote server logs

**Updates delayed:**
- Updates are sent immediately (non-blocking)
- Check network latency
- Verify remote server is processing requests quickly
