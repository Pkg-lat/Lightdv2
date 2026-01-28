# Volume Management API

Volumes provide persistent storage for containers. Each volume is identified by a UUID.

## Base Path

All volume routes are prefixed with `/volumes`

## Create Volume

**Endpoint:** `POST /volumes`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Accept: Application/vnd.pkglatv1+json
```

**Response:**
```json
{
  "volume_id": "d6764075-c5f1-4045-9fb3-85315b85cb0f",
  "path": "/Users/nadhi/Desktop/Lightd-v2/storage/volumes/d6764075-c5f1-4045-9fb3-85315b85cb0f"
}
```

## Write File to Volume

**Endpoint:** `POST /volumes/:volume_id/files`

**Headers:**
```http
Authorization: Bearer lightd_<token>
Accept: Application/vnd.pkglatv1+json
Content-Type: application/json
```

**Request Body:**
```json
{
  "path": "config/server.properties",
  "content": "server-port=25565\nmax-players=20"
}
```

**Response:**
```json
{
  "message": "File written successfully",
  "path": "config/server.properties"
}
```

**Security:** Path traversal is prevented. Paths like `../../../etc/passwd` are rejected.

## Create Folder

**Endpoint:** `POST /volumes/:volume_id/folders`

**Request Body:**
```json
{
  "path": "plugins/MyPlugin"
}
```

**Response:**
```json
{
  "message": "Folder created successfully",
  "path": "plugins/MyPlugin"
}
```


## Copy File or Folder

**Endpoint:** `POST /volumes/:volume_id/copy`

**Request Body:**
```json
{
  "source": "plugins/MyPlugin",
  "destination": "backup/plugins/MyPlugin"
}
```

**Response:**
```json
{
  "message": "Copied successfully",
  "source": "plugins/MyPlugin",
  "destination": "backup/plugins/MyPlugin"
}
```

## List Files

**Endpoint:** `GET /volumes/:volume_id/files`

**Query Parameters:**
- `path` (optional) - Subdirectory to list (default: root)

**Example:**
```bash
curl -X GET "http://localhost:8070/volumes/d6764075/files?path=plugins" \
  -H "Authorization: Bearer lightd_token" \
  -H "Accept: Application/vnd.pkglatv1+json"
```

**Response:**
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

## Compress Volume

**Endpoint:** `POST /volumes/:volume_id/compress`

**Request Body:**
```json
{
  "format": "tar.gz",
  "output_name": "backup"
}
```

**Supported Formats:**
- `zip` - ZIP archive
- `tar` - TAR archive
- `tar.gz` - Gzipped TAR
- `tar.bz2` - Bzip2 compressed TAR

**Response:**
```json
{
  "message": "Compression successful",
  "archive_path": "backup.tar.gz",
  "size": 2048576
}
```

## Decompress Archive

**Endpoint:** `POST /volumes/:volume_id/decompress`

**Request Body:**
```json
{
  "archive_path": "backup.tar.gz",
  "destination": "restored"
}
```

**Response:**
```json
{
  "message": "Decompression successful",
  "destination": "restored"
}
```

## Volume Path Structure

Volumes are stored at:
```
{storage.volumes_path}/{volume_id}/
```

Example:
```
/storage/volumes/d6764075-c5f1-4045-9fb3-85315b85cb0f/
├── config/
│   └── server.properties
├── plugins/
│   └── MyPlugin/
└── backup.tar.gz
```

## Security Features

- **Path Traversal Protection:** All paths are validated to prevent escaping volume boundaries
- **Symlink Validation:** Symlinks are resolved and checked to ensure they stay within volume
- **Rejected Patterns:** `..`, absolute paths (`/`, `C:`), empty paths

## Error Responses

**Invalid Path:**
```json
{
  "error": "Invalid path: path traversal detected"
}
```

**Volume Not Found:**
```json
{
  "error": "Volume not found"
}
```

**File Already Exists:**
```json
{
  "error": "File already exists at path: config.yml"
}
```
