#!/usr/bin/env bash
# Empacotamento AppImage do Arcadia Native (Rust + iced).
#
# O AppImage embute o GStreamer usado pelo player (core + plugins + codecs),
# para o vídeo não depender da versão da distro do usuário. GTK/WebKit ficam
# no host: são usados apenas pelo helper de verificação de fontes, e embutir a
# pilha gráfica no AppImage conflita com o compositor/driver do host.
#
# Uso:
#   ./scripts/package-native-appimage.sh                 # compila e empacota
#   ./scripts/package-native-appimage.sh --skip-build    # usa binários prontos
#   ./scripts/package-native-appimage.sh --output /tmp/x # muda a saída
#
# Variáveis:
#   ARCADIA_NATIVE_BIN        binário alternativo
#   ARCADIA_GAMEPAD_BIN       wrapper de gamepad alternativo
#   ARCADIA_SOURCE_HELPER_BIN helper de verificação alternativo
#   ARCADIA_NATIVE_VERSION    sobrescreve a versão do workspace
#   ARCADIA_NATIVE_ICON       sobrescreve o ícone
#   APPIMAGETOOL              caminho do appimagetool (senão baixa 1.9.1)
#   CARGO_TARGET_DIR          respeitado no build
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

SKIP_BUILD=0
OUTPUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --skip-build) SKIP_BUILD=1 ;;
        --output)
            [ $# -ge 2 ] || { echo "erro: --output exige um diretório" >&2; exit 2; }
            OUTPUT="$2"
            shift
            ;;
        --output=*) OUTPUT="${1#--output=}" ;;
        -h|--help)
            sed -n '2,/^set -euo pipefail$/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "erro: argumento desconhecido: $1" >&2; exit 2 ;;
    esac
    shift
done

# --- Versão e diretórios ----------------------------------------------------

VERSION="${ARCADIA_NATIVE_VERSION:-}"
if [ -z "$VERSION" ]; then
    VERSION="$(awk '
        /^\[workspace\.package\]/ { section = 1; next }
        /^\[/ { section = 0 }
        section && /^version[[:space:]]*=/ {
            gsub(/["[:space:]]/, "", $3)
            print $3
            exit
        }
    ' "$ROOT/Cargo.toml")"
fi
[ -n "$VERSION" ] || {
    echo "erro: não foi possível ler a versão em $ROOT/Cargo.toml" >&2
    exit 1
}

ARCH="$(uname -m)"
PKG="arcadia-native-$VERSION-linux-$ARCH"
if [ -z "$OUTPUT" ]; then
    OUTPUT="$ROOT/dist/native-linux/appimage"
elif [[ "$OUTPUT" != /* ]]; then
    OUTPUT="$ROOT/$OUTPUT"
fi
APPDIR="$OUTPUT/Arcadia.AppDir"
APPIMAGE="$OUTPUT/$PKG.AppImage"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in
    /*) ;;
    *) TARGET_DIR="$ROOT/$TARGET_DIR" ;;
esac

BIN="${ARCADIA_NATIVE_BIN:-$TARGET_DIR/release/arcadia-native}"
case "$BIN" in /*) ;; *) BIN="$ROOT/$BIN" ;; esac
GAMEPAD="${ARCADIA_GAMEPAD_BIN:-$TARGET_DIR/release/arcadia-gamepad}"
case "$GAMEPAD" in /*) ;; *) GAMEPAD="$ROOT/$GAMEPAD" ;; esac
HELPER="${ARCADIA_SOURCE_HELPER_BIN:-$TARGET_DIR/release/arcadia-source-helper}"
case "$HELPER" in /*) ;; *) HELPER="$ROOT/$HELPER" ;; esac

# --- Build ------------------------------------------------------------------

if [ "$SKIP_BUILD" -eq 0 ]; then
    command -v cargo >/dev/null 2>&1 || {
        echo "erro: cargo não encontrado no PATH" >&2
        exit 1
    }
    echo "==> cargo build --locked --release -p arcadia-native -p arcadia-gamepad -p arcadia-source-helper"
    ( cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" cargo build --locked --release \
        -p arcadia-native -p arcadia-gamepad -p arcadia-source-helper )
fi

for file in "$BIN" "$GAMEPAD" "$HELPER"; do
    [ -f "$file" ] && [ -x "$file" ] || {
        echo "erro: binário ausente ou não executável: $file" >&2
        exit 1
    }
done

# --- GStreamer do host ------------------------------------------------------

command -v pkg-config >/dev/null 2>&1 || {
    echo "erro: pkg-config é necessário para localizar o GStreamer" >&2
    exit 1
}
GST_VERSION="$(pkg-config --modversion gstreamer-1.0)"
GST_PLUGINS="$(pkg-config --variable=pluginsdir gstreamer-1.0)"
[ -d "$GST_PLUGINS" ] || {
    echo "erro: diretório de plugins GStreamer não encontrado: $GST_PLUGINS" >&2
    exit 1
}

SCANNER=""
for candidate in \
    "$GST_PLUGINS/gst-plugin-scanner" \
    "$(dirname "$GST_PLUGINS")/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner" \
    /usr/libexec/gstreamer-1.0/gst-plugin-scanner \
    /usr/lib/gstreamer-1.0/gst-plugin-scanner
do
    if [ -x "$candidate" ]; then
        SCANNER="$candidate"
        break
    fi
done
if [ -z "$SCANNER" ]; then
    SCANNER="$(find "$(dirname "$GST_PLUGINS")" -maxdepth 5 -name gst-plugin-scanner -type f 2>/dev/null | head -n1 || true)"
fi
[ -n "$SCANNER" ] || {
    echo "erro: gst-plugin-scanner não encontrado (instale gstreamer1.0-tools)" >&2
    exit 1
}

# Plugins necessários ao player. Se algum sumir do host, o pacote falha em vez
# de gerar um AppImage que não toca vídeo.
REQUIRED_PLUGINS=(
    libgstplayback.so
    libgstcoreelements.so
    libgstapp.so
    libgsttypefindfunctions.so
    libgstvideoconvertscale.so
    libgstaudioconvert.so
    libgstaudioresample.so
    libgstaudioparsers.so
    libgstvideoparsersbad.so
    libgstisomp4.so
    libgsthls.so
    libgstsoup.so
    libgstopenh264.so
    libgstvpx.so
    libgstmatroska.so
)
OPTIONAL_PLUGINS=(
    libgstfaad.so
    libgstmpg123.so
    libgstvorbis.so
    libgstopus.so
    libgstogg.so
    libgsttheora.so
    libgstvolume.so
)

missing=()
for plugin in "${REQUIRED_PLUGINS[@]}"; do
    [ -f "$GST_PLUGINS/$plugin" ] || missing+=("$plugin")
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "erro: plugins GStreamer obrigatórios ausentes: ${missing[*]}" >&2
    echo "  instale gstreamer1.0-plugins-base, good, bad e libav (ou equivalentes)" >&2
    exit 1
fi

BUNDLE="$APPDIR/usr/lib/arcadia-gst"
PLUGIN_DIR="$BUNDLE/plugins"
STAGE="$APPDIR/usr/bin"
mkdir -p "$STAGE" "$PLUGIN_DIR" "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/512x512/apps" "$STAGE/resources/torrent_rpc"

is_excluded() {
    local name="$1"
    case "$name" in
        libc.so*|libm.so*|libdl.so*|libpthread.so*|librt.so*|libresolv.so*|libnsl.so*|\
        libutil.so*|libcrypt.so*|ld-linux*|linux-vdso*|libgcc_s.so*|libstdc++.so*|libatomic.so*)
            return 0 ;;
        libglib-2.0.so*|libgobject-2.0.so*|libgio-2.0.so*|libgmodule-2.0.so*|libgthread-2.0.so*)
            return 0 ;;
        libffi.so*|libpcre2-8.so*|libmount.so*|libselinux.so*|libblkid.so*|libuuid.so*|\
        libsystemd.so*|libudev.so*|libz.so*|libexpat.so*|libxml2.so*|libicu*.so*)
            return 0 ;;
        libGL.so*|libEGL.so*|libGLX.so*|libGLdispatch.so*|libOpenGL.so*|libgbm.so*|\
        libdrm.so*|libvulkan.so*|libX11*.so*|libxcb*.so*|libwayland*.so*|libxkbcommon*.so*)
            return 0 ;;
        libgtk-3.so*|libgdk-3.so*|libwebkit2gtk*.so*|libjavascriptcoregtk*.so*|libgdk_pixbuf*.so*)
            return 0 ;;
        libva.so*|libva-*.so*|libvdpau*.so*|libnvidia*.so*|libcuda*.so*)
            return 0 ;;
        libasound.so*|libpulse*.so*|libpipewire*.so*|libjack*.so*)
            return 0 ;;
    esac
    return 1
}

# Fecha a árvore de dependências de um conjunto de bibliotecas/plugins.
# Tudo que não é sistema/gráfico entra no bundle, inclusive codecs.
copy_dependency_closure() {
    local -a queue=("$@")
    local -a next=()
    local file real base line dep
    while [ ${#queue[@]} -gt 0 ]; do
        next=()
        for file in "${queue[@]}"; do
            real="$(readlink -f "$file" 2>/dev/null || true)"
            [ -n "$real" ] && [ -f "$real" ] || continue
            while IFS= read -r line; do
                case "$line" in
                    *"=> not found"*)
                        echo "erro: dependência não encontrada: $line" >&2
                        exit 1
                        ;;
                    *"=> /"*)
                        dep="${line#*=> }"
                        dep="${dep%% (*}"
                        ;;
                    /*)
                        dep="${line%% (*}"
                        ;;
                    *)
                        continue
                        ;;
                esac
                base="$(basename "$dep")"
                is_excluded "$base" && continue
                if [ ! -f "$BUNDLE/$base" ]; then
                    cp -L "$dep" "$BUNDLE/$base"
                    next+=("$BUNDLE/$base")
                fi
            done < <(ldd "$real" 2>/dev/null || true)
        done
        queue=("${next[@]}")
    done
}

# Plugin seeds: o próprio .so vai para plugins/, e o closure de ldd traz as
# libs do GStreamer e os codecs para BUNDLE/.
seed_plugins=()
for plugin in "${REQUIRED_PLUGINS[@]}" "${OPTIONAL_PLUGINS[@]}"; do
    source="$GST_PLUGINS/$plugin"
    [ -f "$source" ] || continue
    cp -L "$source" "$PLUGIN_DIR/$plugin"
    seed_plugins+=("$source")
done
cp -L "$SCANNER" "$BUNDLE/gst-plugin-scanner"
seed_plugins+=("$SCANNER")
# O binário principal também é seed: traz libdav1d (AVIF) e libudev (gamepad)
# para dentro do bundle, sem depender da distro do usuário.
seed_plugins+=("$BIN")

# As libs core do GStreamer são dependências dos plugins; o closure as copia.
copy_dependency_closure "${seed_plugins[@]}"

# --- Validação do bundle ----------------------------------------------------

command -v gst-inspect-1.0 >/dev/null 2>&1 || {
    echo "erro: gst-inspect-1.0 não encontrado; instale gstreamer1.0-tools" >&2
    exit 1
}
REGISTRY_DIR="$(mktemp -d)"
trap 'rm -rf "$REGISTRY_DIR"' EXIT
export GST_REGISTRY="$REGISTRY_DIR/registry.bin"
export GST_PLUGIN_SYSTEM_PATH_1_0="$PLUGIN_DIR"
export GST_PLUGIN_PATH_1_0="$PLUGIN_DIR"
export GST_PLUGIN_SCANNER="$BUNDLE/gst-plugin-scanner"
export LD_LIBRARY_PATH="$BUNDLE${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
unset GST_PLUGIN_SYSTEM_PATH GST_PLUGIN_PATH

verify_elements=(
    playbin appsink fakesink videoconvert videoscale
    hlsdemux souphttpsrc qtdemux h264parse aacparse
    openh264dec matroskademux vp9dec
)
for element in "${verify_elements[@]}"; do
    if ! gst-inspect-1.0 "$element" >/dev/null 2>&1; then
        echo "erro: elemento GStreamer ausente no bundle: $element" >&2
        exit 1
    fi
done

if command -v gst-launch-1.0 >/dev/null 2>&1 && [ -f "$ROOT/boot.mp4" ]; then
    if ! timeout 30 gst-launch-1.0 -q filesrc location="$ROOT/boot.mp4" \
        ! decodebin ! fakesink sync=false num-buffers=5 >/dev/null 2>&1
    then
        echo "erro: o bundle não conseguiu decodificar o boot.mp4" >&2
        exit 1
    fi
    echo "==> decodificação de vídeo validada com boot.mp4"
fi

echo "==> GStreamer $GST_VERSION embutido: $(find "$BUNDLE" -maxdepth 1 -name '*.so*' | wc -l) libs, $(find "$PLUGIN_DIR" -name '*.so' | wc -l) plugins"

# --- Staging ----------------------------------------------------------------

install -m 0755 "$BIN" "$STAGE/arcadia-native"
install -m 0755 "$GAMEPAD" "$STAGE/arcadia-gamepad"
install -m 0755 "$HELPER" "$STAGE/arcadia-source-helper"

BOOT="$ROOT/boot.mp4"
if [ -f "$BOOT" ]; then
    install -m 0644 "$BOOT" "$STAGE/boot.mp4"
else
    echo "aviso: boot.mp4 não encontrado; o app abre sem a tela de abertura" >&2
fi
install -m 0644 "$ROOT/resources/torrent_rpc/main.py" \
    "$ROOT/resources/torrent_rpc/torrent_downloader.py" "$STAGE/resources/torrent_rpc/"

ICON="${ARCADIA_NATIVE_ICON:-$ROOT/crates/arcadia-native/assets/icons/icon.png}"
install -m 0644 "$ICON" "$APPDIR/arcadia-native.png"
install -m 0644 "$ICON" "$APPDIR/usr/share/icons/hicolor/512x512/apps/arcadia-native.png"
ln -sf arcadia-native.png "$APPDIR/.DirIcon"

cat > "$APPDIR/arcadia-native.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Arcadia
GenericName=Game Library
Comment=Biblioteca e launcher de jogos do Arcadia
Exec=arcadia-native
Icon=arcadia-native
Terminal=false
Categories=Game;
StartupWMClass=arcadia-native
X-AppImage-Version=$VERSION
EOF
cp "$APPDIR/arcadia-native.desktop" "$APPDIR/usr/share/applications/arcadia-native.desktop"

cat > "$APPDIR/AppRun" <<EOF
#!/bin/sh
set -e
HERE="\$(dirname "\$(readlink -f "\$0")")"
GST="\$HERE/usr/lib/arcadia-gst"
export ARCADIA_BUNDLED_GST=1
export LD_LIBRARY_PATH="\$GST\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
export GST_PLUGIN_SYSTEM_PATH_1_0="\$GST/plugins"
export GST_PLUGIN_PATH_1_0="\$GST/plugins"
export GST_PLUGIN_SCANNER="\$GST/gst-plugin-scanner"
unset GST_PLUGIN_SYSTEM_PATH GST_PLUGIN_PATH
CACHE="\${XDG_CACHE_HOME:-\$HOME/.cache}/arcadia"
mkdir -p "\$CACHE"
export GST_REGISTRY="\$CACHE/gst-registry-$VERSION.bin"
exec "\$HERE/usr/bin/arcadia-native" "\$@"
EOF
chmod 0755 "$APPDIR/AppRun"

# --- appimagetool -----------------------------------------------------------

APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
APPIMAGE_RUNTIME_SHA256="617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/arcadia"
TOOL="${APPIMAGETOOL:-}"
if [ -z "$TOOL" ]; then
    CACHE_TOOL="$CACHE_DIR/appimagetool-1.9.1-x86_64.AppImage"
    if [ ! -x "$CACHE_TOOL" ]; then
        mkdir -p "$CACHE_DIR"
        echo "==> baixando appimagetool 1.9.1"
        curl -fsSL --retry 3 -o "$CACHE_TOOL.tmp" \
            "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
        actual="$(sha256sum "$CACHE_TOOL.tmp" | awk '{print $1}')"
        if [ "$actual" != "$APPIMAGETOOL_SHA256" ]; then
            rm -f "$CACHE_TOOL.tmp"
            echo "erro: SHA-256 do appimagetool não confere: $actual" >&2
            exit 1
        fi
        mv "$CACHE_TOOL.tmp" "$CACHE_TOOL"
        chmod 0755 "$CACHE_TOOL"
    fi
    TOOL="$CACHE_TOOL"
fi
[ -x "$TOOL" ] || {
    echo "erro: appimagetool não executável: $TOOL" >&2
    exit 1
}

RUNTIME="${APPIMAGE_RUNTIME:-}"
if [ -z "$RUNTIME" ]; then
    CACHE_RUNTIME="$CACHE_DIR/runtime-x86_64"
    if [ ! -x "$CACHE_RUNTIME" ]; then
        mkdir -p "$CACHE_DIR"
        echo "==> baixando runtime do AppImage"
        curl -fsSL --retry 3 -o "$CACHE_RUNTIME.tmp" \
            "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64"
        actual="$(sha256sum "$CACHE_RUNTIME.tmp" | awk '{print $1}')"
        if [ "$actual" != "$APPIMAGE_RUNTIME_SHA256" ]; then
            rm -f "$CACHE_RUNTIME.tmp"
            echo "erro: SHA-256 do runtime não confere: $actual" >&2
            exit 1
        fi
        mv "$CACHE_RUNTIME.tmp" "$CACHE_RUNTIME"
        chmod 0755 "$CACHE_RUNTIME"
    fi
    RUNTIME="$CACHE_RUNTIME"
fi
[ -f "$RUNTIME" ] || {
    echo "erro: runtime do AppImage não encontrado: $RUNTIME" >&2
    exit 1
}

mkdir -p "$OUTPUT"
rm -f "$APPIMAGE"
echo "==> gerando $APPIMAGE"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$TOOL" --no-appstream \
    --runtime-file "$RUNTIME" "$APPDIR" "$APPIMAGE"

( cd "$OUTPUT" && sha256sum "$(basename "$APPIMAGE")" > SHA256SUMS )

echo
echo "==> artefatos"
ls -lh "$APPIMAGE" "$OUTPUT/SHA256SUMS" | awk '{print "    "$5"\t"$9}'
