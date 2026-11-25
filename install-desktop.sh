#!/usr/bin/env bash
# Instala o Arcadia como aplicativo no menu do sistema (.desktop + ícone).
# Uso: ./install-desktop.sh   (rode de dentro da pasta do projeto)
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/512x512/apps"

mkdir -p "$APPS" "$ICONS" "$HOME/.local/share/pixmaps"
cp "$DIR/app/public/logo-512.png" "$ICONS/arcadia.png"
cp "$DIR/app/public/logo-512.png" "$HOME/.local/share/pixmaps/arcadia.png"

cat > "$APPS/arcadia.desktop" <<EOF
[Desktop Entry]
Name=Arcadia
Comment=Front-end de jogos (Steam, Epic, custom) para Linux
Exec=$DIR/arcadia-desktop.sh
Icon=arcadia
Terminal=false
Type=Application
Categories=Game;
Keywords=steam;epic;games;launcher;
StartupWMClass=arcadia
EOF

echo "Arcadia instalado no menu de aplicativos (arcadia.desktop + ícone)."
# Atualiza caches para o ícone aparecer na hora.
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$APPS" 2>/dev/null || true
