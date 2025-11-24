# Prism – High-Fidelity macOS Virtual Audio Routing

Prism is a high‑performance, fail‑secure virtual audio device for macOS that enables per‑application routing of audio streams. It consists of two main components: a lightweight Core Audio HAL driver (`Prism.driver`) written in Rust, and a user‑space daemon (`prismd`) that manages routing logic and configuration.

## Installation

**Prerequisites:** macOS 13+, Xcode Command Line Tools, Rust.

### 1. Build & Install Daemon and CLI Tool
```bash
cargo install --path .
```

### 2. Build & Install Driver
```bash
./build_driver.sh
./install.sh
```

### 3. Reboot
**Reboot your Mac** to load the driver.
*(Note: Manual `coreaudiod` restart is unsafe and not performed.)*

## Usage

Start the daemon
- Run `prismd` in the background. It stays resident and listens for client list changes from the driver:

```bash
prismd &
```

Use the CLI
- The `prism` command sends requests to `prismd`, which performs all driver operations:

```bash
# Show active Prism clients
prism clients

# Send a routing update: prism set <PID> <OFFSET>
prism set 12345 2

# Explore interactively
prism repl
```

The interactive REPL mirrors the standalone commands (`set`, `list`, `clients`, `help`, `exit`). Ensure `prismd` is running before invoking the CLI.

How routing updates work
- The CLI sends a custom CoreAudio property (`'rout'`) containing a binary struct `{ pid: i32, channel_offset: u32 }`. The driver uses that information to map a source PID to a channel offset.

## Uninstall

## 1. Uninstall Deamon and CLI tool

```bash
# Remove the installed crate (package name: "prism")
cargo uninstall prism
```

## 2. Uninstall Driver

```bash
./uninstall.sh
```

Reboot to finish.

## License

MIT License. See [LICENSE](LICENSE) for details.
