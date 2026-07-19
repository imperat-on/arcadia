#!/usr/bin/env bash
# Arcadia — front-end estilo PS5 (Electron) unificando Steam/Heroic/Lutris.
# Uso:
#   ./arcadia.sh            -> TELA CHEIA (modo console, padrão)
#   ./arcadia.sh --window   -> janela (para testar/depurar)
#   ./arcadia.sh --gamescope-> tela cheia 4K no gamescope (legado)
set -e
DIR="$HOME/.local/share/arcadia"

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

# 1) Biblioteca: só bloqueia na PRIMEIRA execução (sem library.json ainda).
#    Nas demais, o app abre na hora com o library.json anterior e reindexa em
#    BACKGROUND (main.js), avisando o renderer quando terminar. Isso tira ~17s
#    da abertura, que antes esperava o index.py rodar por completo.
if [ ! -f "$DIR/library.json" ]; then
    echo "arcadia: primeira indexação (só desta vez)…"
    python3 "$DIR/index.py"
fi

# 2) Reconstrói o front-end se algum fonte mudou desde o último build
cd "$DIR/app"
if [ ! -f dist/index.html ] || [ -n "$(find src electron index.html vite.config.* package.json -newer dist/index.html 2>/dev/null)" ]; then
    echo "arcadia: fontes mudaram, reconstruindo o front-end…"
    npm run build
fi

# 3) Abre o app
if [ "$1" = "--gamescope" ]; then
    export ARCADIA_GAMESCOPE=1 # ativa a detecção de jogo por processo no Electron
    exec gamescope --backend sdl -W 3840 -H 2160 -r 120 -C 1 --force-grab-cursor -f -- \
        env PS5_FULLSCREEN=1 "$ELECTRON" . --no-sandbox
elif [ "$1" = "--desktop" ] || [ "$1" = "--force-desktop" ]; then
    # Modo desktop (estilo Heroic): janela, mouse, sem boot/gamepad.
    # Se o usuário ligou "Iniciar em modo console" nas Configurações, ignora o
    # --desktop e sobe em modo console mesmo — EXCETO com --force-desktop (ou
    # ARCADIA_FORCE_DESKTOP=1), a rota de escape para voltar ao desktop.
    if [ "$1" != "--force-desktop" ] && [ "$2" != "--force-desktop" ] && [ -z "$ARCADIA_FORCE_DESKTOP" ] && \
       grep -q '"start_in_console_mode"[[:space:]]*:[[:space:]]*true' "$DIR/config.json" 2>/dev/null; then
        exec env PS5_FULLSCREEN=1 "$ELECTRON" . --no-sandbox
    fi
    exec env ARCADIA_MODE=desktop "$ELECTRON" . --no-sandbox
elif [ "$1" = "--window" ]; then
    exec "$ELECTRON" . --no-sandbox
else
    # Padrão: tela cheia direto (sem gamescope) — fullscreen nativo do Electron.
    exec env PS5_FULLSCREEN=1 "$ELECTRON" . --no-sandbox
fi
