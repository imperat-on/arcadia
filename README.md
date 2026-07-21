# Arcadia

A game launcher for Linux with two UIs: a **desktop** mode (Heroic-style
library, store and downloads) and a **console** mode (fullscreen, gamepad,
PS5-inspired).

<p align="center">
  <img src="assets/ui-1.gif" width="820" alt="Arcadia UI">
</p>

<p align="center">
  <img src="assets/ui-2.gif" width="820" alt="Arcadia UI">
</p>

<p align="center">
  <img src="assets/ui-3.gif" width="820" alt="Arcadia UI">
</p>

<p align="center">
  <img src="assets/ui-4.gif" width="820" alt="Arcadia UI">
</p>

## Features

- **Unified library** — Steam, Epic (via Legendary), and custom games (`.exe`
  through Wine) with covers, categories, per-game details.
- **Steam store** — search, download via **DepotDownloader** into your Steam
  libraries (multi-drive), or add to Steam via **SLSsteam**. Manifests are
  fetched from four providers with a mixed-source cascade so downloads keep
  working when a single source is incomplete.
- **Per-game launch options** — Wine/Proton version, per-game prefix,
  DXVK/NVAPI/VKD3D, Esync/Fsync, gamescope, gamemode, MangoHud, custom
  wrappers, env vars, game args, pre/post scripts and verbose logs.
- **Wine manager** — installs and manages GE-Proton and Wine-GE; Steam-shipped
  Protons are detected automatically.
- **Downloads** — serial queue for Epic (Legendary) and Steam
  (DepotDownloader), with real progress in MiB, speed and ETA;
  pause/resume/cancel with cleanup of partials.
- **Achievements** — PS5-style toasts and SLScheevo integration.
- **Trailers** — YouTube search and download via `yt-dlp`.

## Requirements

- Linux x86_64, `python3`, native **Steam**, **.NET 9+** (auto-installed
  locally if missing)
- For Epic titles: **Legendary** login (the binary is fetched by the app)
- For the Steam store: a **Hubcap API key** (free, community) and
  **SLSsteam** (installable from Settings → Integrations)

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/imperat-on/arcadia/master/install.sh | bash
```

Or manual:

```bash
git clone https://github.com/imperat-on/arcadia.git && cd arcadia
./install.sh                        # deps + npm + config + desktop entry
cp config.example.json config.json  # if install.sh didn't create it
./arcadia-desktop.sh                # desktop mode
./arcadia.sh                        # console mode (fullscreen)
```

First run indexes your library (`index.py`) and builds the front-end
(`npm run build`).

## Config

`config.json` (not versioned — see `config.example.json`):

| Key | Purpose |
|---|---|
| `steam_api_key` | Full Steam library (owned games) |
| `hubcap_api_key` | Manifest search and download for the store |

## Layout

```
app/src        # React front-end (desktop/ + ps5-launcher/)
app/electron   # Electron main process (main.js, downloadmanager, steamstore, winemanager)
index.py       # library indexer (Steam/Heroic/Lutris → library.json)
arcadia.sh     # console entry · arcadia-desktop.sh (desktop entry)
```

User data (config, library, downloads, prefixes, artwork) lives under
`~/.local/share/arcadia/` and is **not** versioned.

## Disclaimer

Personal interoperability project. Respect Valve, Epic and other platforms'
terms of service. Use at your own risk.
