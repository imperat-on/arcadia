# Dependências de runtime — Linux e Windows

O que precisa estar na máquina do usuário para cada recurso funcionar, separado
por plataforma. Existe porque a falha dessas dependências é silenciosa na UI.

## Resumo — o que instalar

### Linux

| Dependência | Para quê | Obrigatória? |
|---|---|---|
| `webkit2gtk-4.1`, `gtk3`, `libsoup3`, `librsvg` | abrir a janela | **sim** — o `.deb` declara |
| `gst-plugins-base/good/bad` + `gst-libav` | vídeo do trailer | **sim** — sem isso, tela preta |
| `libayatana-appindicator` | ícone na bandeja | não — só o tray |
| `python3` + `libtorrent` | download por torrent | só para torrent |
| `yt-dlp` | trailers do YouTube | só para trailer |
| `dotnet` (runtime 9+) | DepotDownloader | só para depot |

### Windows

| Dependência | Para quê | Obrigatória? |
|---|---|---|
| WebView2 Runtime | desenhar a UI | **já vem no Win10/11 atualizado**; o setup baixa se faltar |
| Python + `libtorrent` | download por torrent | só para torrent |
| `yt-dlp` | trailers do YouTube | só para trailer |
| `dotnet` (runtime 9+) | DepotDownloader | só para depot |

**Diferença importante:** no Windows **não existe problema de codec**. O WebView2
é Chromium e traz H.264/HLS embutido. Toda a injeção de GStreamer do AppImage
não tem equivalente lá.

## O que o app baixa sozinho

Não precisam de instalação manual — o app busca na primeira necessidade:

| Item | Quando |
|---|---|
| DepotDownloader | primeiro download de depot (`ensure_depotdownloader`) |
| runtime .NET | se `dotnet` não estiver compatível (`ensure_dotnet`) |
| `aria2c.exe` | engine de torrent no Windows (cópia local em `bin/deps`) |

## Como conferir

Menu → Configurações → Diagnóstico. O bloco:

    "dependencies": {
      "python3": true,
      "libtorrent": false,
      "torrent_pronto": false,
      "yt_dlp": false,
      "dotnet": false
    }

`torrent_pronto` é `python3 && libtorrent`.

## Torrent — `python3` + `libtorrent` (binding Python)

O worker é `resources/torrent_rpc/main.py`, JSON por linha no stdio.

### A armadilha que já custou tempo

O app dizia `sudo pacman -S libtorrent-rasterbar`. **Não resolve.** Conferido:

    $ pacman -Ql libtorrent-rasterbar | grep '\.py$'     # nada
    $ pacman -Si libtorrent-rasterbar | grep depends
    Depends On : boost-libs glibc libgcc libstdc++ openssl

É a biblioteca C++ (qBittorrent, rtorrent). Não instala `import libtorrent`.
O binding Python vem do **PyPI**, como wheel `libtorrent`.

### Limite de versão do Python

Wheels do PyPI: cp39 a cp313. **Python 3.14 não tem wheel** — `pip install`
tentaria compilar com Boost e toolchain C++, o que não é instalação de um
comando.

### Interpretador dedicado — o caminho que funciona

Como o `python3` do sistema pode ser 3.14, o app procura o binding em ordem:

1. `ARCADIA_PYTHON` — caminho explícito.
2. venv dedicado (layout por plataforma, abaixo).
3. `python` / `python3` do PATH.

Vence o primeiro que importar `libtorrent`. Se nenhum importar, cai no
interpretador do sistema com `libtorrent_available = false`, e a UI mostra a
mensagem em vez de o worker morrer no spawn.

| Plataforma | Caminho do venv | Executável do sistema |
|---|---|---|
| Linux | `~/.local/share/arcadia-torrent/bin/python` | `python3` |
| Windows | `%USERPROFILE%\.local\share\arcadia-torrent\Scripts\python.exe` | `python` |

### Criar o venv

**Linux** (sem root, com uv):

    uv python install 3.13
    uv venv --python 3.13 ~/.local/share/arcadia-torrent
    uv pip install --python ~/.local/share/arcadia-torrent/bin/python libtorrent

**Windows** (PowerShell, com Python 3.13 instalado):

    py -3.13 -m venv "$env:USERPROFILE\.local\share\arcadia-torrent"
    & "$env:USERPROFILE\.local\share\arcadia-torrent\Scripts\python.exe" -m pip install libtorrent

Conferir:

    # Linux
    ~/.local/share/arcadia-torrent/bin/python -c "import libtorrent; print(libtorrent.__version__)"

    # Windows
    & "$env:USERPROFILE\.local\share\arcadia-torrent\Scripts\python.exe" -c "import libtorrent; print(libtorrent.__version__)"

### Sessão do worker

O worker chama `lt.session({...})` com dict de settings. Conferido no binding
2.1.1: `lt.session`, `lt.torrent_flags` e `lt.parse_magnet_uri` existem;
`lt.settings_pack` **não**. `lt.session(dict)` funciona e é o que o código usa.

## Trailers — `yt-dlp`

O app procura em `YTDLP_PATH` primeiro, depois no PATH.

    # Linux, sem root
    curl -fsSL -o ~/.local/bin/yt-dlp \
      https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp
    chmod +x ~/.local/bin/yt-dlp

No Windows, baixar `yt-dlp.exe` e pôr numa pasta do PATH — ou
`winget install yt-dlp`.

## DepotDownloader — `dotnet`

O `.deb` declara `dotnet-runtime`; o AppImage não embarca. Na primeira vez que um
download de depot roda, o app verifica o runtime e, se incompatível, baixa o
próprio runtime para `bin/dotnet`.

## O que cada pacote embarca

| Item | AppImage | .deb / .rpm | Windows (exe/setup) |
|---|---|---|---|
| WebKitGTK | embarcado | dependência declarada | — (WebView2 do sistema) |
| WebView2Loader.dll | — | — | **embarcado** |
| Codecs H.264/HLS | **embarcados** (openh264 + hls) | dependência declarada | nativo no WebView2 |
| worker Python do torrent | **embarcado** (2 .py) | **embarcado** | **embarcado** |
| `boot.mp4` | embarcado | embarcado | embarcado |
| `python3` / `libtorrent` | não | não | não |
| `yt-dlp` | não | não | não |

Nenhuma das três últimas é embarcável com sensatez: `python3` e `libtorrent` são
específicos da distro e da versão do interpretador, e embarcar um interpretador
próprio cria um segundo Python no sistema. O caminho é detectar e informar — que
é o que o diagnóstico faz agora.
