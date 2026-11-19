#!/usr/bin/env bash
# Empacotamento Linux do Arcadia (shell Tauri/Rust).
#
# Gera .deb e AppImage com o ambiente de mídia correto.
#
# O PROBLEMA QUE ESTE SCRIPT RESOLVE
# ----------------------------------
# O `<video>` da UI usa WebKitGTK, que no Linux delega toda decodificação ao
# GStreamer do sistema. O `bundleMediaFramework` do Tauri embarca os plugins
# básicos (~194), mas deixa de fora justamente os patenteados:
#
#   libgstopenh264.so   decoder H.264       <- trailers da Steam
#   libgsthls.so        demuxer HLS         <- todo trailer moderno
#   libgstlibav.so      decoder via ffmpeg  (alternativa, ~130 libs)
#
# Sem eles o trailer é uma tela preta silenciosa. Este script injeta
# openh264 + hls + libopenh264 dentro do AppDir e reempacota. openh264 vem
# no lugar do libav de propósito: cobre H.264 com 143 KB em vez de arrastar
# o ffmpeg inteiro.
#
# USO
#   ./scripts/package-linux.sh              # deb + appimage + rpm
#   ./scripts/package-linux.sh --deb        # só .deb
#   ./scripts/package-linux.sh --appimage   # só AppImage
#   ./scripts/package-linux.sh --rpm        # só .rpm
#   ./scripts/package-linux.sh --verify     # empacota os três e valida
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI="$ROOT/ui/node_modules/.bin/tauri"
BUNDLE="$ROOT/target/release/bundle"
APPDIR="$BUNDLE/appimage/Arcadia.AppDir"

# Plugin GStreamer que precisa ser embarcado: nome do .so no AppDir.
GST_PLUGINS=(libgstopenh264.so libgsthls.so)
# Bibliotecas de sistema que esses plugins exigem, quando não estiverem lá.
GST_LIBS=(libopenh264.so.8)

DO_DEB=0
DO_APPIMAGE=0
DO_RPM=0
DO_VERIFY=0
for arg in "$@"; do
    case "$arg" in
        --deb) DO_DEB=1 ;;
        --appimage) DO_APPIMAGE=1 ;;
        --rpm) DO_RPM=1 ;;
        --verify) DO_VERIFY=1; DO_DEB=1; DO_APPIMAGE=1; DO_RPM=1 ;;
        -h|--help) sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "arg desconhecido: $arg" >&2; exit 2 ;;
    esac
done
if [ $DO_DEB -eq 0 ] && [ $DO_APPIMAGE -eq 0 ] && [ $DO_RPM -eq 0 ]; then
    DO_DEB=1; DO_APPIMAGE=1; DO_RPM=1
fi

# --- Pré-requisitos ---------------------------------------------------------

if [ ! -x "$TAURI" ]; then
    echo "erro: Tauri CLI não encontrado em $TAURI" >&2
    echo "  instale com: npm --prefix ui i -D @tauri-apps/cli@2" >&2
    exit 1
fi

export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "erro: cargo fora do PATH" >&2; exit 1; }

# patchelf é exigido pelo bundler Linux. Arch não traz por padrão; aceita uma
# cópia local extraída sem root.
if ! command -v patchelf >/dev/null 2>&1; then
    if [ -x "$ROOT/.tools/patchelf" ]; then
        export PATH="$ROOT/.tools:$PATH"
    else
        echo "erro: patchelf ausente (necessário para deb/appimage)" >&2
        echo "  Arch:  sudo pacman -S patchelf" >&2
        exit 1
    fi
fi

# --- Localiza os plugins de codec no host -----------------------------------

# Onde procurar: os plugins extraídos pelo usuário primeiro, depois o sistema.
gst_dir_candidates=(
    "$HOME/.local/lib/gstreamer-1.0"
    "/usr/lib/gstreamer-1.0"
)

find_gst_plugin() {
    local name="$1" dir
    for dir in "${gst_dir_candidates[@]}"; do
        [ -e "$dir/$name" ] && { echo "$dir/$name"; return 0; }
    done
    return 1
}

missing=()
for p in "${GST_PLUGINS[@]}"; do
    find_gst_plugin "$p" >/dev/null || missing+=("$p")
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "erro: plugins GStreamer ausentes no host: ${missing[*]}" >&2
    echo "  Arch: sudo pacman -S gst-plugins-bad" >&2
    exit 1
fi

# --- Build + bundle --------------------------------------------------------

bundles=()
[ $DO_DEB -eq 1 ] && bundles+=(deb)
[ $DO_APPIMAGE -eq 1 ] && bundles+=(appimage)
[ $DO_RPM -eq 1 ] && bundles+=(rpm)

echo "==> compilando e empacotando: ${bundles[*]}"
cd "$ROOT/src-tauri"
"$TAURI" build --bundles "$(IFS=,; echo "${bundles[*]}")"

# --- Injeta os codecs no AppDir --------------------------------------------

inject_appimage_codecs() {
    [ -d "$APPDIR" ] || { echo "aviso: AppDir ausente, pulando injeção"; return 0; }

    local dst="$APPDIR/usr/lib/gstreamer-1.0"
    mkdir -p "$dst"
    local injected=0

    for p in "${GST_PLUGINS[@]}"; do
        local src; src="$(find_gst_plugin "$p")"
        cp -f "$src" "$dst/$p"
        # Fixa as dependências de runtime do plugin (ex.: libopenh264) para que
        # o loader do AppRun as resolva de dentro do pacote.
        if command -v patchelf >/dev/null 2>&1; then
            patchelf --set-rpath '$ORIGIN/..' "$dst/$p" 2>/dev/null || true
        fi
        echo "    + $p"
        injected=$((injected + 1))
    done

    for lib in "${GST_LIBS[@]}"; do
        local real; real="$(readlink -f "/usr/lib/$lib" 2>/dev/null || true)"
        [ -n "$real" ] && [ -e "$real" ] || continue
        cp -fL "$real" "$APPDIR/usr/lib/$lib"
        echo "    + $lib"
        injected=$((injected + 1))
    done

    echo "==> $injected arquivo(s) de codec injetado(s) no AppDir"

    # Reempacota o AppDir no AppImage final.
    local runtime="$HOME/.cache/tauri/AppRun-x86_64"
    local mkapp="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"
    if [ ! -x "$runtime" ] || [ ! -x "$mkapp" ]; then
        echo "aviso: runtime do AppImage ausente no cache do Tauri —" >&2
        echo "       o AppImage publicado NÃO conterá os codecs." >&2
        return 0
    fi

    local out; out="$(ls -t "$BUNDLE/appimage"/*.AppImage | head -1)"
    echo "==> reempacotando $out"
    # O plugin standalone lê ARCH e VERSION do ambiente e escreve o AppImage
    # ao lado do AppDir.
    ARCH=x86_64 VERSION="$(node -p "require('$ROOT/ui/package.json').version" 2>/dev/null || echo 1.5.5)" \
        OUTPUT="$BUNDLE/appimage/$(basename "$out")" \
        "$mkapp" --appdir="$APPDIR" 2>&1 | tail -5
}

if [ $DO_APPIMAGE -eq 1 ]; then
    inject_appimage_codecs
fi

# --- Relatório -------------------------------------------------------------

echo
echo "==> artefatos"
ls -lh "$BUNDLE"/deb/*.deb "$BUNDLE"/appimage/*.AppImage "$BUNDLE"/rpm/*.rpm 2>/dev/null \
    | awk '{print "    "$5"\t"$9}'

if [ $DO_VERIFY -eq 1 ]; then
    echo
    exec "$ROOT/scripts/verify-package.sh"
fi
