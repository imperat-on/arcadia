# Arcadia

Arcadia is a game launcher for **Linux and Windows**. The Desktop and
controller-first Big Picture interfaces run in the same native Rust
application, built with [iced](https://iced.rs/).

- Manage your library, add Steam titles and custom games, and browse the retro
  catalog.
- Queue downloads, configure sources and set per-game launch options.
- Run games with Wine, Proton or an emulator, depending on the title.
- View achievements and playtime. Sign in to synchronize your Steam store
  additions and removals across devices.

## Install

Download the native archive from a GitHub Actions build for your system. See
[the packaging guide](docs/NATIVE-PACKAGING.md) for the archive contents, system
dependencies and installation steps.

- **Linux** — extract `arcadia-native-<version>-linux-<arch>.tar.gz` and run
  `./arcadia-native` in the extracted directory. From a source checkout,
  `./install.sh` builds the app and adds it to the application menu.
- **Windows** — extract `arcadia-native-<version>-windows-x86_64.zip` and run
  `run-arcadia-native.cmd` when the archive bundles GStreamer. For an archive
  without the runtime, install GStreamer and run `arcadia-native.exe`.

Existing users can open the native app with their current Arcadia data. The
default data directories are `~/.local/share/arcadia` on Linux and
`%LOCALAPPDATA%\arcadia` on Windows. Set `ARCADIA_DATA_DIR` to an absolute path
to use a separate directory, for example when testing.

## Build and test

Install a Rust toolchain and the
[platform build dependencies](docs/NATIVE-PACKAGING.md#dependências-de-build).
From the repository root:

```sh
cargo run --locked -p arcadia-native
cargo test --locked --workspace
```

The native packaging scripts build the release binary and produce an archive
with its SHA-256 checksum:

| Platform | Command | Output |
|---|---|---|
| Linux | `./scripts/package-native-linux.sh` | `dist/native-linux/` |
| Windows | `pwsh -File ./scripts/package-native-windows.ps1` | `dist/native-windows/` |

## Architecture

| Crate | Role |
|---|---|
| `arcadia-native` | Desktop and Big Picture, navigation and display |
| `arcadia-native-media`, `arcadia-native-input` | GStreamer playback and controller navigation |
| `arcadia-ipc` | Application commands, called directly from the native interface |
| `arcadia-contracts` | Shared data and synchronization contracts |
| `arcadia-api`, `arcadia-sync`, `arcadia-store` | Backend requests, synchronization and local data |
| `arcadia-steamstore`, `arcadia-retro`, `arcadia-catalog` | Store and retro catalog |
| `arcadia-download`, `arcadia-launch`, `arcadia-emulators` | Downloads and launching games |

On Linux the app can open a separate WebKitGTK window to verify download
sources that need an interactive browser check. The Desktop and Big Picture
interfaces use iced.

## Backend and configuration

The [Arcadia server](https://github.com/imperat-on/arcadia-server) is developed
in a separate repository. The launcher connects over HTTPS. Set
`ARCADIA_API_URL` to use your own instance.

Settings and optional integrations are configured in the app. See
[`config.example.json`](config.example.json) for the local configuration format
and [the runtime dependencies guide](docs/DEPENDENCIAS.md) for optional tools.
Account data and credentials belong in the data directory.

## License

[MIT](LICENSE) © 2026 Davi Kolansinsky.
