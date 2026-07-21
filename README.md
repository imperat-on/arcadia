# Arcadia

A game launcher for Linux with two UIs: a **desktop** mode (windowed
library, store and downloads) and a **console** mode (fullscreen, gamepad,
PS5-inspired).

### Big Picture — boot

Cinematic boot into the fullscreen console UI. The launcher opens straight
into it when started with `./arcadia.sh`, ready for a gamepad.

<p align="center">
  <img src="assets/boot-do-modo-big-picture.gif" width="820" alt="Big Picture boot">
</p>

### Big Picture — games tab

The Games tab: hero of the selected title with its trailer as background,
horizontal rail of covers, and per-game details a click away.

<p align="center">
  <img src="assets/tour-pela-aba-jogos-do-big-picture.gif" width="820" alt="Big Picture games tab">
</p>

### Big Picture — store & downloads

The store laid out PS Store-style: showcase with a rotating hero and rails
per category, or open a category as a dense grid. Downloading a title
enqueues DepotDownloader and shows real progress, speed and ETA.

<p align="center">
  <img src="assets/tour-pela-loja-e-download-do-modo-big-picture.gif" width="820" alt="Big Picture store and downloads">
</p>

### Desktop mode

Windowed layout: library on the left, per-game overview on the right,
launch options in a dedicated pane. Same backend as the console mode — just
a different UI on top.

<p align="center">
  <img src="assets/tour-pelo-arcadia-desktop.gif" width="820" alt="Desktop mode tour">
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
index.py       # library indexer (scans installed sources → library.json)
arcadia.sh     # console entry · arcadia-desktop.sh (desktop entry)
```

User data (config, library, downloads, prefixes, artwork) lives under
`~/.local/share/arcadia/` and is **not** versioned.

## Credits

Arcadia leans on the work of these projects:

- [SLSsteam](https://github.com/AceSLS/SLSsteam) — the Steam plugin loader
  that makes injected apps appear as owned in the Steam client. Powers the
  "Add to Steam" flow and the launch pipeline for injected games.
- [luatools-moon](https://github.com/swwayps/luatools-moon/tree/millennium) —
  the Millennium-less LuaTools bridge (Lumen sidecar and wrapper) that
  Arcadia's SLSsteam setup ships alongside.
- [SLScheevo](https://github.com/xamionex/SLScheevo) — the achievement
  unlocker that Arcadia pairs with for the PS5-style toast pop-ups.

Huge thanks to their authors and contributors.

## License

[MIT](LICENSE) © 2026 Davi Kolansinsky.
