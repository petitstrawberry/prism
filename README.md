# Prism – High-Fidelity macOS Virtual Audio Routing

Prism is a high‑performance, fail‑secure virtual audio device for macOS that enables per‑application routing of audio streams. It consists of two main components: a lightweight Core Audio HAL driver (`Prism.driver`) written in Rust, and a user‑space daemon (`prismd`) that manages routing logic and configuration.

## Installation

**Prerequisites:** macOS 13+, Xcode Command Line Tools, Rust.

### 1. Build & Install Daemon
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

Control is performed via the daemon (`prismd`). The rest of this README (build/install) remains applicable, but runtime routing is managed through `prismd` commands.

Start the daemon
- Run `prismd` in the background to open the interactive REPL:

```bash
prismd &
```

One‑shot CLI (scripting)
- Useful when you want to script a single routing change without entering the REPL:

```bash
# Send a routing update: prismd set <PID> <OFFSET>
prismd set 12345 2
```

Interactive REPL
- If you run `prismd` with no arguments it opens a small REPL. Key commands:
- `help` — show available commands
- `set <PID> <OFFSET>` — send a routing update for PID
- `list` — list the driver's custom properties (to inspect available selectors)
- `exit` / `quit` — quit the REPL

How routing updates work
- `prismd` sends a custom CoreAudio property (`'rout'`) containing a binary struct `{ pid: i32, channel_offset: u32 }` to the driver. The driver uses that information to map a source PID to a channel offset.

## Uninstall

## 1. Uninstall Deamon

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
