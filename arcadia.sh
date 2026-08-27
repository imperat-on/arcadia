#!/usr/bin/env bash
# Arcadia — front-end estilo PS5 (Electron) para sua biblioteca sincronizada.
# Uso:
#   ./arcadia.sh            -> TELA CHEIA (modo console, padrão)
#   ./arcadia.sh --window   -> janela (para testar/depurar)
#   ./arcadia.sh --gamescope-> tela cheia 4K no gamescope (legado)
set -e

# Diretório do próprio script (instalação pode estar em qualquer pasta, não só
# ~/.local/share/arcadia). Quem clonou/extraiu o repo e roda daqui, funciona.
DIR="$(cd "$(dirname "$0")" && pwd)"
# Se for invocado de outro lugar apontando pro caminho padrão (ex.: atalho do
# menu com Exec=.../arcadia.sh), o dirname já resolve certo. Fallback: se o
# script foi chamado por symlink/atalho que não existe, usa o padrão.
if [ ! -f "$DIR/app/package.json" ] && [ -f "$HOME/.local/share/arcadia/app/package.json" ]; then
    DIR="$HOME/.local/share/arcadia"
fi

# A biblioteca é estado do usuário, não parte do checkout.
DATA_DIR="${ARCADIA_DATA_DIR:-$HOME/.local/share/arcadia}"
if [[ "$DATA_DIR" == "~/"* ]]; then DATA_DIR="$HOME/${DATA_DIR:2}"; fi
if [[ "$DATA_DIR" != /* ]]; then DATA_DIR="$PWD/$DATA_DIR"; fi
export ARCADIA_DATA_DIR="$DATA_DIR"

# Resolve o binário do Electron de forma tolerante: o caminho do npm às vezes
# muda ou o download falha. Ordem: (1) caminho padrão do npm; (2) o que o
# pacote 'electron' resolve; (3) Electron do sistema (pacman/apt). Se nada
# existir, avisa como consertar em vez de estourar um críptico "No such file".
ELECTRON="$DIR/app/node_modules/electron/dist/electron"
if [ ! -x "$ELECTRON" ]; then
    ELECTRON="$(cd "$DIR/app" && node -p "require('electron')" 2>/dev/null || true)"
fi
if [ -z "$ELECTRON" ] || [ ! -x "$ELECTRON" ]; then
    ELECTRON="$(command -v electron || true)"  # Electron do sistema
fi
if [ -z "$ELECTRON" ] || [ ! -x "$ELECTRON" ]; then
    echo "arcadia: Electron não encontrado." >&2
    echo "  Conserte com:  cd \"$DIR/app\" && npm rebuild electron" >&2
    echo "  Ou instale o do sistema (Arch: sudo pacman -S electron)." >&2
    exit 1
fi

# 1) Reconstrói o front-end se algum fonte mudou desde o último build
cd "$DIR/app"
if [ ! -f dist/index.html ] || [ -n "$(find src electron index.html vite.config.* package.json -newer dist/index.html 2>/dev/null)" ]; then
    echo "arcadia: fontes mudaram, reconstruindo o front-end…"
    npm run build
fi

# 2) Abre o app
if [ "$1" = "--gamescope" ]; then
    export ARCADIA_GAMESCOPE=1 # ativa a detecção de jogo por processo no Electron
    exec gamescope --backend sdl -W 3840 -H 2160 -r 120 -C 1 --force-grab-cursor -f -- \
        env PS5_FULLSCREEN=1 ARCADIA_MODE=console "$ELECTRON" . --no-sandbox
elif [ "$1" = "--desktop" ] || [ "$1" = "--force-desktop" ]; then
    # Modo desktop (estilo Heroic): janela, mouse, sem boot/gamepad.
    # Se o usuário ligou "Iniciar em modo console" nas Configurações, ignora o
    # --desktop e sobe em modo console mesmo — EXCETO com --force-desktop (ou
    # ARCADIA_FORCE_DESKTOP=1), a rota de escape para voltar ao desktop.
    if [ "$1" = "--force-desktop" ] || [ "$2" = "--force-desktop" ] || [ "$ARCADIA_FORCE_DESKTOP" = "1" ]; then
        exec env ARCADIA_MODE=desktop ARCADIA_FORCE_DESKTOP=1 "$ELECTRON" . --no-sandbox
    fi
    # Sem um modo explícito, o main resolve a preferência persistida. Isso
    # também funciona quando ARCADIA_DATA_DIR aponta para outra instalação.
    exec "$ELECTRON" . --no-sandbox
elif [ "$1" = "--window" ]; then
    exec "$ELECTRON" . --no-sandbox
else
    # Padrão: tela cheia direto (sem gamescope) — fullscreen nativo do Electron.
    exec env PS5_FULLSCREEN=1 "$ELECTRON" . --no-sandbox
fi
