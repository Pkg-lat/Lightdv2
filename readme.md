# Lightd v2

Thanks to
**Ma4z for helping me out in debuggin**
rishi.surf for general pointers

A major revison of the original cluttered, cluster fuck of a system called Lightd.
This version aims for a simpler, stateless remote based arch.

## Features

- 🐳 **Container Management**: Docker-based container orchestration
- 📁 **Volume Management**: Isolated storage with quota support
- 🔒 **SFTP File Access**: Built-in SFTP server for secure file transfers
- 🌐 **Network Management**: Port allocation and firewall rules
- 📊 **Resource Monitoring**: Real-time stats and billing tracking
- 🔌 **WebSocket Support**: Live console and stats streaming
- 🔐 **Authentication**: Token-based API security

## Quick Start

```bash
# Build and run
make run

# Test SFTP
node tests/sftp.js
```

## Documentation

Docs available at - https://docs.pkg.lat/lightd/introduction
A mirror what is used to write that is in /docs

### Key Docs
- [SFTP Quick Start](docs/SFTP-QUICK-START.md) - Get file access working in 5 minutes
- [SFTP Full Guide](docs/sftp.md) - Complete SFTP documentation
- [Container Management](docs/containers.md)
- [Volume Management](docs/volumes.md)
- [API Reference](docs/AI-QUICK-REFERENCE.md)