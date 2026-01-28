# Authentication Guide

Lightd uses Bearer token authentication with additional security headers.

## Token Format

Tokens must follow this format:
```
lightd_<random-string>
```

Example: `lightd_ad2f7fc49ed640429c450e14ed07c8d5`

## Required Headers

All protected routes require:

```http
Authorization: Bearer lightd_<your-token>
Accept: Application/vnd.pkglatv1+json
```

## Token Management CLI

### View Current Token

```bash
./lightd --token what
```

**Output:**
```
Current Token: lightd_ad2f7fc49ed640429c450e14ed07c8d5
```

### Generate New Random Token

```bash
./lightd --token reset
```

**Output:**
```
✓ Token reset successfully!
New Token: lightd_f3a8b9c2d1e4f5a6b7c8d9e0f1a2b3c4

⚠️  Warning: The old token is now invalid.
   Update your applications with the new token.
```

### Set Custom Token

```bash
./lightd --token set
```

**Interactive prompt:**
```
Enter new token (must start with 'lightd_' and be at least 20 characters):
> lightd_my_custom_secure_token_here

✓ Token set successfully!
New Token: lightd_my_custom_secure_token_here
```


## Token Storage

Tokens are stored in `config.json`:

```json
{
  "authorization": {
    "enabled": true,
    "token": "lightd_ad2f7fc49ed640429c450e14ed07c8d5"
  }
}
```

## Creating API Tokens (Programmatic)

### Generate Token with TTL

**Endpoint:** `POST /auth/tokens`

**Headers:**
```http
Authorization: Bearer lightd_<master-token>
Accept: Application/vnd.pkglatv1+json
Content-Type: application/json
```

**Request Body:**
```json
{
  "ttl": "1h",
  "remove_on_use": false
}
```

**TTL Format:**
- `15m` - 15 minutes
- `1h` - 1 hour
- `30s` - 30 seconds
- `7d` - 7 days

**Response:**
```json
{
  "token": "lightd_generated_token_here",
  "expires_at": "2026-01-28T15:30:00Z"
}
```

### Single-Use Tokens

Set `remove_on_use: true` to create tokens that expire after first use:

```json
{
  "ttl": "1h",
  "remove_on_use": true
}
```

## Token Validation

Tokens are validated on every request:
- Must start with `lightd_`
- Must match token in config.json or exist in token database
- Must not be expired (for programmatic tokens)
- Automatically removed if `remove_on_use: true`

## Token Cleanup

Expired tokens are automatically cleaned up every 5 minutes by the daemon.

## Public Routes (No Auth Required)

These routes don't require authentication:

- `GET /api/v1/public/ping` - Health check

## Example Authenticated Request

```bash
curl -X GET http://localhost:8070/volumes \
  -H "Authorization: Bearer lightd_ad2f7fc49ed640429c450e14ed07c8d5" \
  -H "Accept: Application/vnd.pkglatv1+json"
```

## Security Notes

- Tokens are case-sensitive
- Store tokens securely (environment variables, secrets manager)
- Use short TTLs for temporary access
- Rotate master token regularly using `--token reset`
- For WebSocket connections, pass token as query parameter
