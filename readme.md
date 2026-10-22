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
# Create your local config (contains the API token — it is gitignored)
cp config.example.json config.json

# Generate a token and put it in config.json, or export it
export LIGHTD_API_TOKEN=lightd_...

# Build and run
make run

# Test SFTP
node tests/sftp.js
```

`config.json` is **not** committed and must not be: it holds the master API token,
which authenticates every route. A leaked token grants full control of the daemon
and all containers. Use `config.example.json` as the template, and rotate the token
with `./lightd --token reset` if one is ever exposed.

Test scripts read `LIGHTD_API_TOKEN` from the environment.

## Documentation

Docs available at - https://docs.pkg.lat/lightd/introduction
A mirror what is used to write that is in /docs

### Key Docs
- [SFTP Quick Start](docs/SFTP-QUICK-START.md) - Get file access working in 5 minutes
- [SFTP Full Guide](docs/sftp.md) - Complete SFTP documentation
- [Container Management](docs/containers.md)
- [Volume Management](docs/volumes.md)
- [API Reference](docs/AI-QUICK-REFERENCE.md)