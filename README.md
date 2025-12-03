# Prism – macOS Virtual Audio Router

Prism is a virtual routing device for macOS. It exposes a 64-channel bus where every app can be pinned to its own stereo pair, so you can feed OBS, DAWs, and recorders with isolated tracks while preserving a separate monitor mix. The driver is written in Rust and uses Accelerate (vDSP) for its realtime mixing path.

## Highlights

- **Per-application routing** – send each process to its own stereo pair on the 64-channel bus.
- **Separable stems** – capture game audio, voice chat, music, and system alerts on independent channels for streaming or editing.
- **Instant muting on reroute** – Prism clears a slot the moment an app disconnects or moves, so recordings never pick up stale audio.
- **Automation-ready** – drive routing changes via the `prism` CLI or by scripting its Unix socket API.

## Use cases

- **Streaming with OBS/Streamlabs** – capture the game, voice chat, and music on isolated inputs so the mixer scene stays tidy.
- **Multitrack podcasting or post-production** – record each participant or app feed to its own track inside Logic, Reaper, or Audition.
- **Hybrid events** – send different mixes to Zoom, in-room PA, and a backup recorder without touching physical patch bays.
- **Creative monitoring** – audition reference audio or click tracks privately while the audience hears only the main program.

## Prerequisites

- macOS 13 or later.
- Xcode Command Line Tools (`xcode-select --install`).
- Rust toolchain (`rustup`), using the default stable channel.

## Build and Install

1. **Install the CLI and daemon**

```bash
cargo install --path .
```

`cargo install` places `prism` and `prismd` under `~/.cargo/bin/`; ensure that directory is on your `PATH`.

2. **Build the CoreAudio driver bundle**

```bash
./build_driver.sh
```

This script compiles the plugin binary, updates the bundle under `Prism.driver/`, and performs codesign stubs if required.

3. **Install the driver**

```bash
sudo ./install.sh
```

The installer copies `Prism.driver` into `/Library/Audio/Plug-Ins/HAL/` and refreshes permissions.

4. **Reboot**

Reboot macOS to allow the HAL plug-in to load. 

## Usage

1. **Launch the daemon**

```bash
prismd --daemonize
```

The `--daemonize` flag double-forks and detaches `prismd`. Omit it if you prefer to run in the foreground for logging.

2. **Manage routing with the CLI**

```bash
# List all currently attached clients and their channel offsets
prism clients

# Route PID 12345 to the stereo slot that starts at channel offset 4
prism set 12345 4
```

Routing requests are serialized as a custom `'rout'` property containing `{ pid: i32, channel_offset: u32 }`. The driver consumes the property, updates the slot table, and clears the corresponding loopback pair if the client moved.

Use `prism --help` to discover additional subcommands.

### Mixing model

- Channels 1/2 always carry the same full-system mix you hear through your speakers.
- Every pinned app gets its own stereo pair on the 64-channel bus, so you can record or stream it separately.
- Reroute apps on the fly without pops or stale audio—the mix updates instantly every cycle.
- Tuned for live workflows, keeping latency low even when lots of apps are active.
- Apple’s Accelerate framework (vDSP) handles the math under the hood, keeping CPU use light.

## Uninstall

1. Remove the CLI and daemon binaries (optional):

```bash
cargo uninstall prism
```

2. Remove the driver bundle:

```bash
sudo ./uninstall.sh
```

3. Reboot to unload the HAL plug-in.

## License

MIT License. See [LICENSE](LICENSE) for details.
