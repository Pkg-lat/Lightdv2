# WebSocket API

Real-time container monitoring and control via WebSocket connection.

## Connection

**Endpoint:** `ws://localhost:8070/ws/:internal_id?token=<token>`

**Example:**
```javascript
const ws = new WebSocket('ws://localhost:8070/ws/my-server-001?token=lightd_your-token-here');
```

**Authentication:**
- Token passed as query parameter
- Token validated on connection
- Connection closed if token expires during session
- Single-use tokens (`remove_on_use: true`) are consumed on connection

## Outbound Events (Server → Client)

### Stats Event

Real-time container statistics with change detection.

**Event:**
```json
{
  "event": "stats",
  "cpu_usage": 25.5,
  "memory_usage": 134217728,
  "memory_limit": 536870912,
  "network_rx": 1024,
  "network_tx": 2048,
  "block_read": 4096,
  "block_write": 8192
}
```

**Fields:**
- `cpu_usage` - CPU percentage (0-100 per core)
- `memory_usage` - Memory used in bytes
- `memory_limit` - Memory limit in bytes
- `network_rx` - Network bytes received
- `network_tx` - Network bytes transmitted
- `block_read` - Disk bytes read
- `block_write` - Disk bytes written

**Note:** Only sent when values change (change detection enabled).

### Console Output

Real-time console output from container.

**Event:**
```json
{
  "event": "console",
  "data": "Hello mate\n"
}
```

### Console Duplicate

Duplicate of console output (for compatibility).

**Event:**
```json
{
  "event": "console duplicate",
  "data": "Hello mate\n"
}
```

### Container State Events

**Installing:**
```json
{
  "event": "event",
  "data": "installing"
}
```

**Installed (Ready):**
```json
{
  "event": "event",
  "data": "installed"
}
```

**Starting:**
```json
{
  "event": "event",
  "data": "starting"
}
```

**Running (Pattern Matched):**
```json
{
  "event": "event",
  "data": "running"
}
```

**Stopping:**
```json
{
  "event": "event",
  "data": "stopping"
}
```


**Exit (Container Stopped):**
```json
{
  "event": "event",
  "data": "exit"
}
```

### Daemon Messages

System messages from Lightd.

**Event:**
```json
{
  "event": "daemon_message",
  "data": "Server started"
}
```

**Common messages:**
- `"Container started"` - Container started successfully
- `"Server started"` - Server detected as running (pattern matched)
- `"Container stopped"` - Container stopped
- `"Container restarted"` - Container restarted
- `"Error: <message>"` - Error occurred

### Logs Event

Historical logs (when requested).

**Event:**
```json
{
  "event": "logs",
  "data": "Previous log line 1\nPrevious log line 2\n"
}
```

## Inbound Events (Client → Server)

### Send Command

Execute command in container.

**Event:**
```json
{
  "event": "send_command",
  "command": "echo 'Hello World'\n"
}
```

**Note:** Include `\n` at the end for command execution.

**Example:**
```javascript
ws.send(JSON.stringify({
  event: 'send_command',
  command: 'ls -la\n'
}));
```

### Power Actions

**Start Container:**
```json
{
  "event": "power",
  "action": "start"
}
```

**Kill Container:**
```json
{
  "event": "power",
  "action": "kill"
}
```

**Restart Container:**
```json
{
  "event": "power",
  "action": "restart"
}
```

### Request Logs

Request historical logs.

**Event:**
```json
{
  "event": "request_logs"
}
```

**Response:** Server sends `logs` event with historical data.

## Complete Example

```javascript
// Connect to WebSocket
const ws = new WebSocket('ws://localhost:8070/ws/my-server-001?token=lightd_token');

// Handle connection open
ws.onopen = () => {
  console.log('Connected to container');
  
  // Request historical logs
  ws.send(JSON.stringify({
    event: 'request_logs'
  }));
};

// Handle incoming messages
ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  
  switch(data.event) {
    case 'console':
      console.log('Console:', data.data);
      break;
      
    case 'stats':
      console.log('CPU:', data.cpu_usage + '%');
      console.log('Memory:', data.memory_usage, '/', data.memory_limit);
      break;
      
    case 'event':
      console.log('State:', data.data);
      break;
      
    case 'daemon_message':
      console.log('System:', data.data);
      break;
      
    case 'logs':
      console.log('Historical logs:', data.data);
      break;
  }
};

// Handle errors
ws.onerror = (error) => {
  console.error('WebSocket error:', error);
};

// Handle close
ws.onclose = () => {
  console.log('Disconnected from container');
};

// Send a command
function sendCommand(cmd) {
  ws.send(JSON.stringify({
    event: 'send_command',
    command: cmd + '\n'
  }));
}

// Power actions
function startContainer() {
  ws.send(JSON.stringify({
    event: 'power',
    action: 'start'
  }));
}

function stopContainer() {
  ws.send(JSON.stringify({
    event: 'power',
    action: 'kill'
  }));
}

function restartContainer() {
  ws.send(JSON.stringify({
    event: 'power',
    action: 'restart'
  }));
}

// Usage
sendCommand('echo "Hello from WebSocket"');
startContainer();
```

## Connection Lifecycle

1. **Connect** - Client connects with token
2. **Validate** - Server validates token
3. **Subscribe** - Client subscribed to container events
4. **Stream** - Real-time stats and console output
5. **Monitor** - Token validity checked periodically
6. **Disconnect** - Connection closed if token expires or client disconnects

## Error Handling

**Invalid Token:**
- Connection rejected with 401 status
- WebSocket closes immediately

**Token Expired:**
- Connection closes with close code
- Client should reconnect with new token

**Container Not Found:**
- Connection accepted but no events sent
- Client should verify container exists

## Performance Notes

- Stats sent only when values change (reduces bandwidth)
- Console output streamed in real-time
- Multiple clients can connect to same container
- Each client gets independent event stream

## Browser Example

```html
<!DOCTYPE html>
<html>
<head>
  <title>Container Monitor</title>
</head>
<body>
  <div id="console"></div>
  <input id="command" type="text" placeholder="Enter command">
  <button onclick="sendCmd()">Send</button>
  
  <script>
    const ws = new WebSocket('ws://localhost:8070/ws/my-server-001?token=lightd_token');
    const consoleDiv = document.getElementById('console');
    
    ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.event === 'console') {
        consoleDiv.innerHTML += data.data.replace(/\n/g, '<br>');
      }
    };
    
    function sendCmd() {
      const input = document.getElementById('command');
      ws.send(JSON.stringify({
        event: 'send_command',
        command: input.value + '\n'
      }));
      input.value = '';
    }
  </script>
</body>
</html>
```
