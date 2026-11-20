#!/usr/bin/env bash
# Instalação do Arcadia (edição Tauri/Rust): dependências de sistema, build
# do front-end e do binário, config inicial e atalho no menu.
#
# Para USUÁRIO FINAL, o caminho recomendado é o release empacotado:
#   https://github.com/imperat-on/arcadia/releases
#   (AppImage, .deb, .rpm, setup.exe) — nada deste script é preciso.
# Este script é para instalar a partir do código-fonte.
#
# Duas formas de usar:
#   Local:  ./install.sh
#   Remoto: curl -fsSL https://raw.githubusercontent.com/imperat-on/arcadia/master/install.sh | bash
#
# Flags:
#   -y, --yes   Não pergunta nada (assume sim para dependências; mantém o resto
#               do comportamento). Para rodar sem TTY.
set -e

# --- helpers ---------------------------------------------------------------
ERRO() { echo "ERRO: $*" >&2; }

# Confirma com o usuário; devolve 0 (sim) ou 1 (não). Com -y ou sem TTY,
# responde sim automaticamente para não travar a instalação.
PERGUNTAR() {
    local msg="$1"
    if [ "${SEM_PERGUNTAS:-0}" = 1 ]; then return 0; fi
    if [ ! -t 0 ]; then return 0; fi
    local r
    read -rp "$msg [s/N] " r
    [[ "$r" =~ ^[sS] ]]
}

SEM_PERGUNTAS=0
if [ "${1:-}" = "-y" ] || [ "${1:-}" = "--yes" ]; then SEM_PERGUNTAS=1; fi

DESTINO="$HOME/.local/share/arcadia"

# --- Modo remoto (curl | bash) ----------------------------------------------
# Quando o script vem de stdin, "$0" não resolve para o repo. Detecta pelo
# marker do projeto (Cargo.toml do workspace); se ausente, clona para
# ~/.local/share/arcadia e re-executa de lá.
SELF="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd)"
if [ -z "$SELF" ] || [ ! -f "$SELF/Cargo.toml" ]; then
    REPO="https://github.com/imperat-on/arcadia.git"
    echo "==> Baixando o Arcadia para $DESTINO"
    if [ -d "$DESTINO/.git" ]; then
        git -C "$DESTINO" pull --ff-only
    elif [ -e "$DESTINO" ] && [ -n "$(ls -A "$DESTINO" 2>/dev/null)" ]; then
        echo "    $DESTINO já existe mas não é um clone válido — movendo para backup e clonando de novo."
        mv "$DESTINO" "$DESTINO.bak-$(date +%Y%m%d-%H%M%S)"
        git clone "$REPO" "$DESTINO"
    else
        git clone "$REPO" "$DESTINO"
    fi
    exec bash "$DESTINO/install.sh" "${@:-}"
fi
DIR="$SELF"

echo "==> Instalando o Arcadia a partir de $DIR"

# --- 0/4 Dependências de sistema -------------------------------------------
# O binário é Rust/Tauri: precisa do cargo e das libs WebKitGTK no build.
# Em runtime: python3 (worker de torrent com libtorrent), steam (nativa),
# dotnet (DepotDownloader), procps (pgrep/pkill), yt-dlp (trailers),
# ffmpeg (remux), tar (Proton/Wine), node/npm (front-end).
echo "==> 0/4 Verificando dependências de sistema"
FALTAM=()
for cmd in python3 steam dotnet pgrep du df yt-dlp ffmpeg tar find git node npm unzip; do
    command -v "$cmd" >/dev/null 2>&1 || FALTAM+=("$cmd")
done
# O cargo não é pacote de sistema padrão: checa também o rustup do usuário.
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
    FALTAM+=(cargo)
fi

if [ ${#FALTAM[@]} -gt 0 ]; then
    echo "    Faltando: ${FALTAM[*]}"
    declare -A PKG_ARCH=( [python3]=python [steam]=steam [dotnet]=dotnet-runtime [pgrep]=procps-ng [du]=coreutils [df]=coreutils [yt-dlp]=yt-dlp [ffmpeg]=ffmpeg [tar]=tar [find]=findutils [git]=git [node]=nodejs [npm]=npm [unzip]=unzip [cargo]=rust )
    declare -A PKG_DEB=(  [python3]=python3 [steam]=steam [dotnet]=dotnet-runtime-9.0 [pgrep]=procps [du]=coreutils [df]=coreutils [yt-dlp]=yt-dlp [ffmpeg]=ffmpeg [tar]=tar [find]=findutils [git]=git [node]=nodejs [npm]=npm [unzip]=unzip [cargo]=cargo )
    PKGS=()
    if command -v pacman >/dev/null 2>&1; then
        for c in "${FALTAM[@]}"; do PKGS+=("${PKG_ARCH[$c]}"); done
        echo "    Vou instalar via pacman (sudo): ${PKGS[*]}"
        if PERGUNTAR "    OK instalar essas dependências?"; then
            sudo pacman -S --needed --noconfirm "${PKGS[@]}"
        else
            echo "    Dependências não instaladas — o app pode não funcionar."
        fi
    elif command -v apt-get >/dev/null 2>&1; then
        for c in "${FALTAM[@]}"; do PKGS+=("${PKG_DEB[$c]}"); done
        # WebKitGTK e GStreamer: sem eles o Tauri não liga e o vídeo fica preto.
        PKGS+=(libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev pkg-config libssl-dev build-essential curl)
        echo "    Vou instalar via apt (sudo): ${PKGS[*]}"
        if PERGUNTAR "    OK instalar essas dependências?"; then
            sudo apt-get update && sudo apt-get install -y "${PKGS[@]}"
        else
            echo "    Dependências não instaladas — o app pode não funcionar."
        fi
    else
        echo "    AVISO: distro não reconhecida — instale manualmente: ${FALTAM[*]}"
        echo "    Docs de build: docs/DEPENDENCIAS.md"
    fi
else
    echo "    Tudo presente."
fi

command -v cargo >/dev/null 2>&1 || { ERRO "cargo não encontrado (instale em https://rustup.rs)"; exit 1; }

# Opcional: libtorrent para o worker de torrent.
if python3 -c "import libtorrent" >/dev/null 2>&1; then
    echo "    libtorrent: presente"
else
    echo "    libtorrent: ausente (torrents ficam desativados; downloads HTTP seguem)"
fi

# Opcional: o yt-dlp usa o Deno para resolver o desafio JS do YouTube, exigido
# só em vídeos com restrição de idade. Sem ele o resto dos trailers funciona,
# então nunca abortamos a instalação por causa disso.
if ! command -v deno >/dev/null 2>&1; then
    echo "    Deno (opcional, trailers com restrição de idade): não encontrado"
    if command -v pacman >/dev/null 2>&1; then
        sudo pacman -S --needed --noconfirm deno 2>/dev/null || echo "    (sem Deno — trailers +18 podem falhar)"
    else
        echo "    (instale manualmente se quiser trailers +18)"
    fi
fi

# --- 1/4 Build ---------------------------------------------------------------
echo "==> 1/4 Build do front-end e do binário"
cd "$DIR/ui"
if npm install; then
    echo "    npm install OK"
else
    ERRO "npm install falhou. Verifique sua conexão e o Node/npm."
    exit 1
fi
if npm run build; then
    echo "    Build do front-end OK"
else
    ERRO "build do front-end falhou."
    exit 1
fi

cd "$DIR"
echo "    Compilando o Arcadia (release)…"
if cargo build --release -p arcadia --features custom-protocol; then
    echo "    Compilação OK"
else
    ERRO "cargo build falhou. Veja docs/DEPENDENCIAS.md para as libs de sistema."
    exit 1
fi

BIN="$DIR/target/release/arcadia"
[ -x "$BIN" ] || { ERRO "binário não encontrado em $BIN"; exit 1; }

# --- 2/4 Config --------------------------------------------------------------
echo "==> 2/4 Configuração inicial"
mkdir -p "$DESTINO"
if [ ! -f "$DESTINO/config.json" ]; then
    cp "$DIR/config.example.json" "$DESTINO/config.json"
    echo "    config.json criado em $DESTINO — edite e cole suas chaves (Steam API / Hubcap)."
else
    echo "    config.json já existe (mantido)."
fi

# --- 3/4 Atalho no menu de aplicativos --------------------------------------
echo "==> 3/4 Atalho no menu de aplicativos"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/512x512/apps"
mkdir -p "$DESKTOP_DIR" "$ICON_DIR"
if [ -f "$DIR/src-tauri/icons/512x512.png" ]; then
    cp "$DIR/src-tauri/icons/512x512.png" "$ICON_DIR/arcadia.png"
elif compgen -G "$DIR"/src-tauri/icons/*.png >/dev/null 2>&1; then
    cp "$DIR"/src-tauri/icons/*.png "$ICON_DIR/arcadia.png"
else
    echo "    (ícone não encontrado — atalho sem ícone)"
fi
cat > "$DESKTOP_DIR/arcadia.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Arcadia
Comment=Launcher de jogos (Rust/Tauri)
Exec=$BIN
Icon=arcadia
Terminal=false
Categories=Game;
EOF
update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
echo "    Atalho criado: $DESKTOP_DIR/arcadia.desktop"

# --- 4/4 Resumo -------------------------------------------------------------
echo ""
echo "Pronto! O Arcadia está em:  $BIN"
echo "Pelo menu de aplicativos:   Arcadia"
echo ""
echo "Usuário final: prefira o release empacotado (AppImage/deb/setup.exe):"
echo "  https://github.com/imperat-on/arcadia/releases"
echo ""
echo "Opcionais dentro do app (botões): .NET local, SLSsteam, SLScheevo,"
echo "versões de Wine/Proton — tudo se instala pela interface."
echo ""
echo "Para desinstalar: ./uninstall.sh  (pergunta o que preservar)"
