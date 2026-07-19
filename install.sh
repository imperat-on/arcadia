#!/usr/bin/env bash
# Instalação completa do Arcadia: dependências do front-end, config inicial e
# atalho no menu de aplicativos. Uso: ./install.sh
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"

echo "==> 1/3 Dependências do front-end (npm install)"
cd "$DIR/app"
npm install

echo "==> 2/3 Configuração inicial"
if [ ! -f "$DIR/config.json" ]; then
    cp "$DIR/config.example.json" "$DIR/config.json"
    echo "    config.json criado — edite e cole suas chaves (Steam API / Hubcap)."
else
    echo "    config.json já existe (mantido)."
fi

echo "==> 3/3 Atalho no menu de aplicativos"
"$DIR/install-desktop.sh"

echo ""
echo "Pronto! Rode:  ./arcadia-desktop.sh   (modo desktop)"
echo "         ou:   ./arcadia.sh           (modo console/tela cheia)"
