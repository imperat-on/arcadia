#!/usr/bin/env bash
# Arcadia (shell Tauri/Rust) — entrada Linux.
# Uso:
#   ./arcadia-tauri.sh              -> modo console (tela cheia), padrão do sh
#   ./arcadia-tauri.sh --desktop    -> janela
#   ARCADIA_MODE=console|desktop ./arcadia-tauri.sh
set -e

DIR="$(cd "$(dirname "$0")" && pwd)"

# --- Vídeo: correção do frame verde no NVIDIA proprietário ----------------
#
# Sintoma: o trailer decodifica (duração e timeline corretas) mas o frame sai
# como retângulo verde chapado. Verde é o plano Y (luminância) do YUV sendo lido
# como se fosse RGB — falha de CONVERSÃO, não de decode.
#
# Causa: com driver NVIDIA proprietário, o upload de textura do vídeo no sink
# GL do WebKitGTK entrega o frame sem converter o plano de cor.
#
# Correção cirúrgica: desligar SÓ o GL sink do vídeo.
#
#   NÃO use WEBKIT_DISABLE_DMABUF_RENDERER=1 aqui. Ele funciona para o vídeo,
#   mas desliga o zero-copy do compositing do webview INTEIRO — a UI toda passa
#   a copiar por CPU e o app fica arrastado. Já foi testado: vídeo ok, app
#   travado.
#
# Com GL sink desligado, apenas o vídeo usa caminho de software; a UI continua
# acelerada. Sobrescreva com ARCADIA_KEEP_GL_SINK=1 em GPUs/drivers onde o sink
# GL funciona (Intel, AMD, NVIDIA aberto).
if [ "${ARCADIA_KEEP_GL_SINK:-0}" != "1" ]; then
    export WEBKIT_GST_DISABLE_GL_SINK=1
fi

# WebKitGTK no Linux delega TODA a decodificação de mídia ao GStreamer do
# sistema — diferente do Chromium no Windows, que embarca H.264 no próprio
# binário. Sem os plugins avdec_h264 / hlsdemux o <video> dos trailers fica
# em tela preta sem erro visível na UI. Os plugins extraídos sem root vivem em
# ~/.local/lib/gstreamer-1.0 e precisam entrar no caminho de busca do
# WebKitWebProcess, que herda o ambiente do processo pai.
if [ -d "$HOME/.local/lib/gstreamer-1.0" ]; then
    export GST_PLUGIN_PATH="${GST_PLUGIN_PATH:+$GST_PLUGIN_PATH:}$HOME/.local/lib/gstreamer-1.0"
fi

# Rede de segurança: se os plugins não estiverem em nenhum dos dois lugares, o
# vídeo fica preto silenciosamente. Avisa no stderr em vez de deixar o usuário
# descobrir pelo sintoma.
if ! gst-inspect-1.0 openh264dec >/dev/null 2>&1 \
   && ! gst-inspect-1.0 avdec_h264 >/dev/null 2>&1; then
    echo "arcadia: nenhum decoder H.264 no GStreamer — trailers não vão tocar." >&2
    echo "  Arch:  sudo pacman -S gst-plugins-bad gst-libav" >&2
    echo "  Debian: sudo apt install gstreamer1.0-plugins-bad gstreamer1.0-libav" >&2
fi

# Binário: build local tem precedência; instalação de sistema como fallback.
BIN="$DIR/target/release/arcadia"
if [ ! -x "$BIN" ]; then
    BIN="$(command -v arcadia || true)"
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
    echo "arcadia: binário não encontrado." >&2
    echo "  Compile com: cd \"$DIR\" && cargo build --release -p arcadia --features custom-protocol" >&2
    exit 1
fi

# Modo: flag explícita > variável de ambiente > padrão console.
MODE="${ARCADIA_MODE:-console}"
case "${1:-}" in
    --desktop|--window) MODE="desktop" ;;
    --console|--fullscreen) MODE="console" ;;
esac
export ARCADIA_MODE="$MODE"

exec "$BIN" "$@"
