#!/usr/bin/env bash
# Compila e instala o Arcadia para o usuário atual no Linux.
# Uso: ./install.sh [--yes]
set -euo pipefail

YES=0
case "${1:-}" in
    -y|--yes) YES=1 ;;
    "") ;;
    *) echo "Uso: $0 [--yes]" >&2; exit 2 ;;
esac

confirm_dependencies() {
    if [ "$YES" -eq 1 ] || [ ! -t 0 ]; then
        return 0
    fi
    local answer
    read -rp 'Instalar as dependências com sudo? [s/N] ' answer
    [[ "$answer" =~ ^[sS] ]]
}

DATA_DIR="${ARCADIA_DATA_DIR:-$HOME/.local/share/arcadia}"
case "$DATA_DIR" in
    /*) ;;
    *) DATA_DIR="$PWD/$DATA_DIR" ;;
esac
SOURCE="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
if [ ! -f "$SOURCE/crates/arcadia-native/Cargo.toml" ]; then
    echo 'Execute ./install.sh a partir de uma cópia do código-fonte do Arcadia.' >&2
    exit 1
fi

export PATH="$HOME/.cargo/bin:$PATH"
have_build_dependencies() {
    command -v cc >/dev/null 2>&1 && command -v pkg-config >/dev/null 2>&1 &&
        pkg-config --exists gstreamer-1.0 gstreamer-app-1.0 gstreamer-video-1.0 \
            libudev gtk+-3.0 webkit2gtk-4.1 'dav1d >= 1.3.0'
}
if ! have_build_dependencies; then
    echo 'O Arcadia precisa de GStreamer, libudev, dav1d 1.3+, GTK 3 e WebKitGTK 4.1 para compilar.'
    if { command -v pacman >/dev/null 2>&1 || command -v apt-get >/dev/null 2>&1; } && confirm_dependencies; then
        if command -v pacman >/dev/null 2>&1; then
            sudo pacman -S --needed --noconfirm base-devel pkgconf gstreamer \
                gst-plugins-base-libs gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav \
                systemd-libs dav1d gtk3 webkit2gtk-4.1
        else
            sudo apt-get update
            sudo apt-get install -y build-essential pkg-config \
                libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
                gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav \
                libudev-dev libdav1d-dev libgtk-3-dev libwebkit2gtk-4.1-dev
        fi
    fi
fi

command -v cargo >/dev/null 2>&1 || {
    echo 'Cargo não encontrado. Instale o Rust em https://rustup.rs.' >&2
    exit 1
}
if ! have_build_dependencies; then
    echo "Dependências de compilação ausentes. Consulte $SOURCE/docs/NATIVE-PACKAGING.md." >&2
    exit 1
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$SOURCE/target}"
case "$TARGET_DIR" in
    /*) ;;
    *) TARGET_DIR="$SOURCE/$TARGET_DIR" ;;
esac
echo '==> Compilando o Arcadia'
( cd "$SOURCE" && CARGO_TARGET_DIR="$TARGET_DIR" cargo build --locked --release -p arcadia-native -p arcadia-gamepad )

BIN_DIR="$DATA_DIR/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/512x512/apps"
mkdir -p "$BIN_DIR/resources/torrent_rpc" "$DESKTOP_DIR" "$ICON_DIR"
install -m 0755 "$TARGET_DIR/release/arcadia-native" "$TARGET_DIR/release/arcadia-gamepad" "$BIN_DIR/"
install -m 0644 "$SOURCE/boot.mp4" "$BIN_DIR/"
install -m 0644 "$SOURCE/resources/torrent_rpc/main.py" \
    "$SOURCE/resources/torrent_rpc/torrent_downloader.py" "$BIN_DIR/resources/torrent_rpc/"
install -m 0644 "$SOURCE/crates/arcadia-native/assets/icons/icon.png" "$ICON_DIR/arcadia.png"
if [ ! -f "$DATA_DIR/config.json" ]; then
    install -m 0644 "$SOURCE/config.example.json" "$DATA_DIR/config.json"
fi

# O campo Exec do Desktop Entry exige aspas para caminhos com espaços.
desktop_argument() {
    local value="$1"
    value="${value//\\/\\\\\\\\}"
    value="${value//\"/\\\\\"}"
    value="${value//\$/\\\\\$}"
    value="${value//\`/\\\\\`}"
    value="${value//%/%%}"
    printf '"%s"' "$value"
}
EXEC="$(desktop_argument "$BIN_DIR/arcadia-native")"
DATA_ENV="$(desktop_argument "ARCADIA_DATA_DIR=$DATA_DIR")"
cat > "$DESKTOP_DIR/arcadia.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Arcadia
Comment=Biblioteca e launcher de jogos
Exec=env $DATA_ENV $EXEC
Icon=arcadia
Terminal=false
Categories=Game;
StartupWMClass=arcadia-native
DESKTOP
update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true

echo "Arcadia instalado em $BIN_DIR/arcadia-native. Abra pelo menu de aplicativos."
