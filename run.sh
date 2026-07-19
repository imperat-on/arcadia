#!/usr/bin/env bash
# Atualiza a biblioteca e abre o front-end PS5.
# Uso:
#   ./run.sh            -> roda na área de trabalho (para testar)
#   ./run.sh --gamescope-> roda em tela cheia 4K no gamescope (modo console)
set -e
DIR="$HOME/.local/share/arcadia"

# 1) Indexa Steam/Heroic/Lutris -> library.json
python3 "$DIR/index.py"

# 2) Abre o app Godot
if [ "$1" = "--gamescope" ]; then
    exec gamescope --backend sdl -W 3840 -H 2160 -r 120 -C 1 --force-grab-cursor -f -- \
        godot --path "$DIR/ui" --rendering-driver vulkan
else
    exec godot --path "$DIR/ui"
fi
