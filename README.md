# Arcadia

A self-hosted game launcher for **Linux and Windows**. Library management,
store, downloads, achievements and retro catalog — with the entire main process
written in **Rust**.

```
Rust main process   ·  Tauri 2  ·  React renderer
Linux AppImage/deb/rpm  ·  Windows NSIS installer
```

Two interfaces ship in the same binary:

- **Desktop** — windowed library, store, downloads and settings.
- **Console (Big Picture)** — fullscreen, gamepad-first, TV-friendly.

## Why Rust

The original Electron main process was replaced by a Rust workspace. What that
buys, measured on this repository:

| | |
|---|---|
| Main-process code | **123k lines of Rust** in 19 crates |
| Renderer | 31k lines of TypeScript (React) |
| IPC surface | **203 invoke channels** + 22 event channels, all handled in Rust |
| Tests | **1547** across the workspace |
| Runtime JS | **none** — no Node, no `require('electron')` |

The renderer stays TypeScript because Tauri hosts a system webview
(WebKitGTK on Linux, WebView2 on Windows) — that is the architecture, not a
leftover.

## Features

- **Unified library** — store titles, downloaded games and custom entries, each
  with cover art and per-game details.
- **Store** — catalog search, per-title details, download queue. Sources are
  configured by you; providers can be swapped without touching the rest.
- **Retro catalog** — 28k+ titles across consoles, with paginated browsing,
  per-system filtering and a local cover cache.
- **Downloads** — serial queue with real progress in MiB, speed and ETA,
  pause/resume/cancel and cleanup of partial files. Torrent support via
  libtorrent, HTTP via the native client.
- **Per-game launch options** — Wine/Proton version, dedicated prefix,
  DXVK/NVAPI/VKD3D, Esync/Fsync, gamescope, gamemode, MangoHud, custom wrappers,
  environment variables, game arguments, pre/post scripts and verbose logs
  (Linux; Windows titles run natively).
- **Wine manager** — installs and manages GE-Proton and Wine-GE, detects the
  Protons Steam already ships.
- **Achievements** — real-time unlock tracking with native toast notifications,
  plus RetroAchievements integration and cross-machine sync.
- **Playtime** — hours read from your Steam installation, per account.
- **Trailers** — YouTube search and download through `yt-dlp`.
- **Discord Rich Presence** — optional, silent when disabled.
- **Accounts** — per-account libraries on the same machine.

## Requirements

### Linux

- x86_64, glibc **2.34+** (Ubuntu 22.04, Debian 12 or newer for the AppImage)
- `python3` (torrent worker and gamepad bridge)
- A native **Steam** installation
- **.NET 9+** — installed locally by the launcher if missing

### Windows

- Windows 10/11 x64
- A native **Steam** installation
- **.NET 9+** — installed automatically if missing
- WebView2 runtime (present on Windows 10/11 by default)

## Install

### Linux — AppImage (any distribution)

```bash
curl -fsSL -o Arcadia.AppImage \
  https://github.com/imperat-on/arcadia/releases/latest/download/Arcadia_amd64.AppImage
chmod +x Arcadia.AppImage
./Arcadia.AppImage
```

The AppImage bundles the GStreamer codecs the webview needs, so trailers play
without installing media plugins system-wide.

### Linux — package

`Arcadia_<version>_amd64.deb` and `Arcadia-<version>-1.x86_64.rpm` are published
with each release and declare their system dependencies.

### Linux — from source

```bash
git clone https://github.com/imperat-on/arcadia.git && cd arcadia
export PATH="$HOME/.cargo/bin:$PATH"
(cd ui && npm ci && npm run build)
cargo build --release -p arcadia --features custom-protocol
./target/release/arcadia
```

`--features custom-protocol` is **mandatory**: without it the release build
resolves the dev server and opens a blank window.

### Windows

Download from the [releases page](https://github.com/imperat-on/arcadia/releases):

- `Arcadia_<version>_x64-setup.exe` — NSIS installer.
- `arcadia.exe` — portable, no installation.

The installer is unsigned, so SmartScreen shows a warning on first run.

## Build

Two artifacts, each with a packaging script and a verifier:

```bash
./scripts/package-linux.sh --verify      # AppImage + deb + rpm
./scripts/package-windows.sh --verify    # arcadia.exe + NSIS installer
```

`--verify` runs the checks that catch a broken release before it ships: codec
presence, bundled GStreamer plugins, RPATH leakage, glibc ceiling, PE32+
architecture and external DLLs.

Guides: [docs/PACKAGING-LINUX.md](docs/PACKAGING-LINUX.md) ·
[docs/PACKAGING-WINDOWS.md](docs/PACKAGING-WINDOWS.md)

### glibc note

An AppImage built on a rolling-release host inherits that host's glibc floor.
Build on `ubuntu-22.04` for an artifact that runs on 2022+ distros — the CI
`package` job does exactly that, producing a binary that needs only **glibc
2.34**. The local verifier prints a warning when a local build is not
distributable.

## Architecture

```
src-tauri/                 Tauri shell: window, IPC bridge, boot
crates/
  arcadia-ipc              dispatcher for all 202 IPC channels
  arcadia-contracts        shared types and channel manifest
  arcadia-store            config, library, session persistence
  arcadia-api              backend client (HTTP, auth, DoH resolver)
  arcadia-download         download manager, torrent worker bridge
  arcadia-launch           process launch, Wine/Proton, env assembly
  arcadia-steam            Steam integration (hours, shortcuts, manifests)
  arcadia-catalog          catalog client with mirror and ETag cache
  arcadia-steamstore       Steam store client
  arcadia-retro            retro catalog (server client + local index)
  arcadia-achievements     achievement tracking and schemas
  arcadia-ra               RetroAchievements client
  arcadia-emulators        emulator registry and detection
  arcadia-media            trailer search and download
  arcadia-social           friends and community
  arcadia-sync             cross-machine sync engine
  arcadia-plugins          plugin registry
  arcadia-fixes            launch fix patches
  arcadia-updater          auto-update state machine
ui/                        React + TypeScript renderer
```

User data lives under `~/.local/share/arcadia/` on Linux and
`%LOCALAPPDATA%\arcadia` on Windows, and is never versioned.
`ARCADIA_DATA_DIR=/absolute/path` isolates it for development.

## Backend

The client talks to the Arcadia API over HTTPS. The server and migrations live
in a separate, private repository; installing the launcher does not require a
local PostgreSQL. Point the client at an authorized instance with
`ARCADIA_API_URL`.

Address failover, DNS resolution and response caching happen in the Rust client
(`arcadia-api`): the system resolver is tried first with a 500 ms budget, then
Cloudflare DoH, with a five-minute cache and preferred-IP ordering.

## Config

`config.json` lives in the data directory, is not versioned, and is created from
`config.example.json`:

| Key | Purpose |
|---|---|
| `steam_api_key` | Optional Steam metadata and integration data |
| `steam_id64` | Account used for the Steam integration |
| `hubcap_api_key` | Manifest provider key used by the store |
| `language` | UI language (`pt-BR`, `en-US`, `es-ES`) |
| `discord_rich_presence` | Enable Discord Rich Presence |
| `discord_client_id` | Discord application id |

Secrets are never written to logs or uploaded; the client redacts them before
any IPC response reaches the renderer.

## Testing

```bash
cargo test --workspace    # 1547 tests
cd ui && npx tsc --noEmit # renderer type check
```

The packaging contract is itself covered by tests
(`crates/arcadia-updater/tests/packaging_contract.rs`): version alignment across
`Cargo.toml`, `ui/package.json` and `tauri.conf.json`, bundle targets, and the
GStreamer dependencies the Linux bundle must declare.

## Legacy reference

The Electron app (`app/electron`, 37k lines of JS) has been **removed**. Every
module was ported to a Rust crate first — `steamstore.js` → `arcadia-steamstore`,
`downloadmanager.js` → `arcadia-download`, `retro-catalog.js` → `arcadia-retro`,
`emulator-registry.js` → `arcadia-emulators`, and so on. The last two holdouts,
the gamepad uinput wrapper and the `contracts/` helpers, are now
`arcadia-gamepad` and `arcadia-contracts`.

The tag `legacy-electron-final` keeps the full tree as a frozen reference if the
old IPC contract ever needs to be consulted.

`arcadia-tauri.sh` is the build entry point.

## Uninstall

### Linux

Remove the AppImage or the package, then delete the data directory:

```bash
rm -rf ~/.local/share/arcadia
```

Games downloaded through the store stay registered in your Steam installation;
remove them from the launcher first if you do not want them there.

### Windows

Use **Add/Remove Programs** for the installed version, or delete the extracted
folder for the portable one.

## License

[MIT](LICENSE) © 2026 Davi Kolansinsky.
