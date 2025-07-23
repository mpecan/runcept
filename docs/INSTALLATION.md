# Installation and Setup Guide

This guide covers everything you need to install, configure, and start using Runcept.

## Prerequisites

### System Requirements

- **Operating System**: Linux or macOS (Windows support in development)
- **Rust**: Version 1.70 or later
- **Memory**: 50MB+ available RAM
- **Disk**: 100MB+ available space

> **Note**: While the codebase includes some Windows compatibility code, full Windows support is not yet complete. The application currently relies on Unix-specific features like process groups and signal handling (SIGTERM/SIGKILL) that need Windows equivalents.

### Required Dependencies

**Rust Toolchain:**
```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Verify installation
rustc --version
cargo --version
```

**Git:**
```bash
# Ubuntu/Debian
sudo apt update && sudo apt install git

# macOS (with Homebrew)
brew install git

# Windows (if using WSL2)
sudo apt update && sudo apt install git
```

## Installation Methods

### Method 1: Install from Source (Recommended)

```bash
# Clone the repository
git clone https://github.com/mpecan/runcept.git
cd runcept

# Build the release binary
cargo build --release

# The binary will be available at target/release/runcept
```

### Method 2: Install to System PATH

```bash
# After building from source
# Linux/macOS
sudo cp target/release/runcept /usr/local/bin/
# or for user-only installation
cp target/release/runcept ~/.local/bin/

# Windows
# Copy target\release\runcept.exe to a directory in your PATH
```

### Method 3: Development Installation

```bash
# Clone and install in development mode
git clone https://github.com/mpecan/runcept.git
cd runcept

# Install with cargo (adds to ~/.cargo/bin)
cargo install --path .

# Verify installation
runcept --version
```

## Initial Setup

### 1. Verify Installation

```bash
# Check if runcept is working
runcept --help

# Check version
runcept --version

# Test daemon functionality
runcept daemon status
```

### 2. Create Your First Project

```bash
# Navigate to your project directory
cd /path/to/your/project

# Initialize runcept configuration
runcept init

# This creates a .runcept.toml file with example configuration
```

### 3. Configure Your Project

Edit the generated `.runcept.toml` file:

```toml
[project]
name = "my-project"
description = "My development environment"
inactivity_timeout = "30m"

[[process]]
name = "example"
command = "echo 'Hello from runcept!'"
working_dir = "."
auto_restart = false
```

### 4. Test Your Setup

```bash
# Activate your project environment
runcept activate

# Check status
runcept status

# View logs
runcept logs example

# Deactivate when done
runcept deactivate
```

## Platform-Specific Setup

### Linux

**Additional Dependencies (Ubuntu/Debian):**
```bash
sudo apt update
sudo apt install build-essential pkg-config libssl-dev
```

**Service Installation (Optional):**
```bash
# Create systemd service for runcept daemon
sudo tee /etc/systemd/system/runcept.service > /dev/null <<EOF
[Unit]
Description=Runcept Process Manager Daemon
After=network.target

[Service]
Type=simple
User=$USER
ExecStart=/usr/local/bin/runcept daemon start -f
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# Enable and start service
sudo systemctl enable runcept
sudo systemctl start runcept
```

### macOS

**Using Homebrew (if available):**
```bash
# Install dependencies
brew install rust git

# Follow standard installation steps above
```

**LaunchAgent Setup (Optional):**
```bash
# Create launch agent for automatic daemon startup
mkdir -p ~/Library/LaunchAgents

cat > ~/Library/LaunchAgents/com.runcept.daemon.plist <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.runcept.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/runcept</string>
        <string>daemon</string>
        <string>start</string>
        <string>-f</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
EOF

# Load the launch agent
launchctl load ~/Library/LaunchAgents/com.runcept.daemon.plist
```

### Windows

> **⚠️ Windows Support Status**: Windows support is currently **in development** and not yet fully functional. The application may compile but will likely fail at runtime due to Unix-specific dependencies.

**Current Windows Limitations:**
- Process group management uses Unix-specific APIs
- Signal handling (SIGTERM/SIGKILL) is not implemented for Windows
- Some IPC mechanisms may not work correctly
- Process termination and cleanup may not function properly

**For Windows Users:**
- Consider using WSL2 (Windows Subsystem for Linux) as a workaround
- Windows native support is planned for a future release
- Contributions to improve Windows compatibility are welcome

**WSL2 Installation (Recommended):**
```bash
# Install WSL2 and Ubuntu
wsl --install

# Then follow the Linux installation instructions within WSL2
```

## Configuration

### Global Configuration

Runcept stores global configuration in:
- Linux/macOS: `~/.config/runcept/config.toml`
- Windows: `%APPDATA%\runcept\config.toml`

Example global configuration:
```toml
[daemon]
socket_path = "/tmp/runcept.sock"  # Unix socket path (Linux/macOS)
log_level = "info"
max_log_files = 10
log_rotation_size = "10MB"

[mcp]
enabled = true
port = 3000
host = "127.0.0.1"

[process]
default_timeout = "30s"
max_restart_attempts = 3
restart_delay = "5s"
```

### Environment Variables

Runcept recognizes these environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `RUNCEPT_CONFIG_DIR` | Configuration directory | OS-specific |
| `RUNCEPT_LOG_LEVEL` | Log level (trace, debug, info, warn, error) | `info` |
| `RUNCEPT_SOCKET_PATH` | Daemon socket path | OS-specific |
| `RUNCEPT_DATABASE_PATH` | SQLite database path | `~/.config/runcept/runcept.db` |

## Verification

### Test Installation

```bash
# Basic functionality test
runcept --version
runcept daemon status

# Create test project
mkdir test-runcept && cd test-runcept
runcept init

# Edit .runcept.toml to add a simple process
cat >> .runcept.toml <<EOF
[[process]]
name = "test"
command = "echo 'Test successful!'"
working_dir = "."
EOF

# Test activation
runcept activate
runcept logs test
runcept deactivate
```

### Performance Test

```bash
# Test with multiple processes
cat > .runcept.toml <<EOF
[project]
name = "performance-test"

[[process]]
name = "proc1"
command = "sleep 10"
working_dir = "."

[[process]]
name = "proc2"
command = "sleep 10"
working_dir = "."
depends_on = ["proc1"]

[[process]]
name = "proc3"
command = "sleep 10"
working_dir = "."
depends_on = ["proc2"]
EOF

# Time the activation
time runcept activate
runcept status
runcept deactivate
```

## Troubleshooting Installation

### Common Issues

**"runcept: command not found"**
- Ensure the binary is in your PATH
- Try using the full path: `/path/to/target/release/runcept`

**Permission denied errors**
- Make sure the binary is executable: `chmod +x target/release/runcept`
- Check directory permissions for config and log directories

**Compilation errors**
- Update Rust: `rustup update`
- Install required system dependencies (see platform-specific sections)
- Clear cargo cache: `cargo clean`

**Daemon connection issues**
- Check if daemon is running: `runcept daemon status`
- Verify socket permissions and path
- Check firewall settings (for MCP server)

### Getting Help

If you encounter issues:

1. Check the logs: `runcept logs <process-name>`
2. Run with verbose output: `runcept --verbose <command>` or `runcept -v <command>`
3. List all processes: `runcept ps`
4. Check daemon status: `runcept daemon status`
5. Check environment status: `runcept status`
6. Review system requirements and dependencies

For additional support:
- GitHub Issues: https://github.com/mpecan/runcept/issues
- Main Documentation: See the [README.md](../README.md) for usage examples and configuration

## Next Steps

After successful installation:

1. Read the [main README](../README.md) for configuration examples and usage patterns
2. Try the examples in the README to get familiar with runcept
3. Set up your first project with `runcept init`
4. Explore the MCP integration for AI assistant support (see README for details)