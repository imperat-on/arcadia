# Arcadia

A self-hosted game launcher for **Linux and Windows**, built with Electron and
React. It manages your library, launches games through Steam, Proton/Wine
prefixes or local emulators, tracks achievements, and syncs playtime and
achievements across your machines through your own backend.

Two interfaces ship in the same app:

- **Desktop** — windowed library, store and downloads.
- **Console (Big Picture)** — fullscreen, gamepad-first, TV-friendly. The
  launcher opens straight into it when started with `./arcadia.sh`.

## Features

- **Unified library** — games added from the store, downloaded titles and
  custom entries, each with cover art and per-game details.
- **Store** — catalog search, per-title details and a download queue. Sources
  are configured by you in Settings; providers can be cut in and out without
  touching the rest of the app.
- **Downloads** — serial queue with real progress in MiB, speed and ETA,
  pause/resume/cancel and cleanup of partial files.
- **Per-game launch options** — Wine/Proton version, dedicated prefix,
  DXVK/NVAPI/VKD3D, Esync/Fsync, gamescope, gamemode, MangoHud, custom
  wrappers, environment variables, game arguments, pre/post scripts and
  verbose logs (Linux only; Windows titles run natively).
- **Wine manager** — installs and manages GE-Proton and Wine-GE, and detects
  the Protons that Steam already ships (Linux only).
- **Achievements** — real-time unlock tracking with native toast
  notifications, plus sync across machines.
- **Playtime** — hours read from your Steam installation, per account.
- **Trailers** — YouTube search and download through `yt-dlp`.
- **Accounts** — per-account libraries on the same machine; each account sees
  the titles assigned to it.

## Requirements

### Linux

- x86_64, `python3`, a native **Steam** installation, **.NET 9+**
  (installed locally by the launcher if missing).
- The compatible DepotDownloader runtime is fetched automatically on the first
  depot download, or from Settings → Integrations.

### Windows

- Windows 10/11 x64, a native **Steam** installation, **.NET 9+**
  (installed automatically if missing).

## Install

### Linux — script

```bash
curl -fsSL https://raw.githubusercontent.com/imperat-on/arcadia/master/install.sh | bash
```

### Linux — manual

```bash
git clone https://github.com/imperat-on/arcadia.git && cd arcadia
./install.sh                       # dependencies, npm install, config, desktop entry
cp config.example.json config.json # if install.sh did not create it
./arcadia-desktop.sh               # desktop mode
./arcadia.sh                       # console mode (fullscreen)
```

### Windows

Download the latest build from the
[releases page](https://github.com/imperat-on/arcadia/releases):

- `Arcadia-Setup-<version>-x64.exe` — NSIS installer.
- `Arcadia-<version>-x64.exe` — portable, no installation.
- `Arcadia-<version>-x64.zip` — plain unpacked folder.
- `Arcadia-<version>-x86_64.AppImage` — Linux AppImage, `chmod +x` and run.

On first run the launcher prepares the local data directory, restores the
session and uses the front-end that ships inside the package.

## Uninstall

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/imperat-on/arcadia/master/uninstall.sh | bash
```

Or, from inside the repo, `./uninstall.sh`. It removes the app, its desktop
entry/icons and `~/.local/share/arcadia/`, and asks before touching anything
that is not a cache or binary: installed games (`games/`) and Wine prefixes
(`prefixes/`, which may hold saves) are kept unless you opt out. Use
`-y`/`--yes` to skip the prompts. Games downloaded through the store stay
registered in your Steam installation; remove them from the store first if you
do not want them there.

### Windows

Use **Add/Remove Programs** for the installed version, or delete the extracted
folder for the portable one.

## Config

`config.json` lives in the data directory, is not versioned, and is created
from `config.example.json`:

| Key | Purpose |
|---|---|
| `steam_api_key` | Optional Steam metadata and integration data |
| `steam_id64` | Account used for the Steam integration |
| `hubcap_api_key` | Manifest provider key used by the store |
| `language` | UI language (`pt-BR`, `en-US`, `es-ES`) |

## Layout

```
app/src        # React front-end (desktop/ + console/)
app/electron   # Electron main process (main.js, downloadmanager, steamstore, winemanager)
arcadia.sh     # Linux console entry · arcadia-desktop.sh (Linux desktop entry)
install.sh     # Linux setup · uninstall.sh (Linux full removal)
```

User data (config, library, downloads, prefixes, artwork) lives under
`~/.local/share/arcadia/` on Linux and `%LOCALAPPDATA%\arcadia` on Windows, and
is never versioned. Set `ARCADIA_DATA_DIR=/absolute/path` to use an isolated
directory for development or testing; the Electron caches follow the same
directory.

## Architecture

- **`app/`** — the Electron launcher: React renderer in `app/src`, main
  process in `app/electron`.
- **Backend** — the client talks to the Arcadia API over HTTPS. The server
  code and migrations live in a separate, private repository.

Per-account sync works from `owned_games.json`: it decides which titles from
the local snapshot each account sees (a guest sees everything). Sensitive
values — provider keys and source caches — are never uploaded.

## Backend

The launcher uses the managed Arcadia API. Point the client at an authorized
instance with `ARCADIA_API_URL`; installing the launcher does not require a
local PostgreSQL.

## License

[MIT](LICENSE) © 2026 Davi Kolansinsky.
