#!/usr/bin/env bash
# Desinstalação do Arcadia no Linux.
#
# Uso: ./uninstall.sh [--yes]
#
# Flags:
#   -y, --yes   Não pergunta nada: mantém jogos/prefixos/SLSsteam (move-os
#               para fora do diretório antes de apagar) e não pede a
#               confirmação final. Para rodar sem TTY (ex.: outro script).
set -e
DIR="${ARCADIA_DATA_DIR:-$HOME/.local/share/arcadia}"
case "$DIR" in
    /*) ;;
    *) DIR="$PWD/$DIR" ;;
esac
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/512x512/apps"
PIXMAPS="$HOME/.local/share/pixmaps"
SEM_PERGUNTAS=0
if [ "${1:-}" = "-y" ] || [ "${1:-}" = "--yes" ]; then SEM_PERGUNTAS=1; fi

# Sem TTY não dá pra ler resposta: assume modo -y
# em vez de travar/pular a confirmação.
if [ "$SEM_PERGUNTAS" = 0 ] && [ ! -t 0 ]; then
    echo "Sem terminal interativo — assumindo modo não-interativo (preserva jogos/prefixos/SLSsteam)."
    SEM_PERGUNTAS=1
fi

if [ ! -d "$DIR" ] && [ ! -f "$APPS/arcadia-native.desktop" ] && [ ! -x "$HOME/.local/bin/arcadia-native" ]; then
    echo "Arcadia não encontrado em $DIR — nada para desinstalar."
    exit 0
fi

# Segurança: só remove se $DIR parecer mesmo o Arcadia. A instalação Rust
# empacota o binário em bin/ e os recursos em resources/ — o diretório de
# dados carrega config.json/library. Evita apagar uma pasta que não é o
# projeto por engano.
if [ -d "$DIR" ] && [ ! -f "$DIR/config.json" ] && [ ! -f "$DIR/bin/arcadia-native" ] &&
    [ ! -f "$DIR/bin/arcadia" ] && [ ! -f "$DIR/library.json" ] && [ ! -d "$DIR/contas" ]; then
    echo "ERRO: $DIR existe mas não parece ser o Arcadia." >&2
    echo "Abortando para não apagar algo por engano. Se quiser mesmo, remova na mão." >&2
    exit 1
fi

echo "Isso vai remover o Arcadia deste computador ($DIR)."
echo

# --- 0/5 Fecha o app, se estiver aberto -------------------------------------
# Encerra o aplicativo antes de remover arquivos que ele pode gravar.
if pgrep -u "$(id -u)" -x 'arcadia(-native)?' >/dev/null 2>&1; then
    echo "==> 0/5 Fechando o Arcadia…"
    pkill -u "$(id -u)" -x 'arcadia(-native)?' 2>/dev/null || true
    sleep 1
fi

# --- 1/5 Pergunta pelo que tem DADO DO USUÁRIO, não só cache/binário -------
# games/ (jogos Epic via Legendary) e prefixes/ (prefixos Wine — podem ter
# save de jogos custom/Epic) NÃO são cache: apagar sem perguntar destruiria
# biblioteca de jogos e progresso de save do usuário. Por padrão preservamos.
MANTER_JOGOS=1
MANTER_PREFIXOS=1
REMOVER_SLSSTEAM=0

if [ -d "$DIR/games" ] && [ -n "$(ls -A "$DIR/games" 2>/dev/null)" ]; then
    if [ "$SEM_PERGUNTAS" = 1 ]; then
        echo "==> 1/5 games/ (jogos Epic) será preservado — use sem -y para escolher."
    else
        TAM=$(du -sh "$DIR/games" 2>/dev/null | cut -f1)
        read -rp "==> 1/5 Encontrei jogos Epic instalados em games/ (${TAM:-?}). Apagar também? [s/N] " r
        [[ "$r" =~ ^[sS] ]] && MANTER_JOGOS=0
    fi
fi

if [ -d "$DIR/prefixes" ] && [ -n "$(ls -A "$DIR/prefixes" 2>/dev/null)" ]; then
    if [ "$SEM_PERGUNTAS" = 1 ]; then
        echo "    prefixes/ (prefixos Wine) será preservado — use sem -y para escolher."
    else
        TAM=$(du -sh "$DIR/prefixes" 2>/dev/null | cut -f1)
        read -rp "    Encontrei prefixos Wine (podem ter saves) em prefixes/ (${TAM:-?}). Apagar também? [s/N] " r
        [[ "$r" =~ ^[sS] ]] && MANTER_PREFIXOS=0
    fi
fi

# SLSsteam é uma ferramenta de TERCEIROS (slsteam-moon) instalada pelo botão
# da loja, fora da pasta do Arcadia — perguntamos à parte porque outro
# programa pode depender dela.
if [ -d "$HOME/.local/share/SLSsteam" ] || [ -d "$HOME/.config/SLSsteam" ]; then
    if [ "$SEM_PERGUNTAS" = 1 ]; then
        echo "    SLSsteam (ferramenta de terceiros) será preservada — use sem -y para escolher."
    else
        read -rp "    Remover também a SLSsteam (ferramenta de terceiros usada pra injeção de jogos)? [s/N] " r
        [[ "$r" =~ ^[sS] ]] && REMOVER_SLSSTEAM=1
    fi
fi

if [ "$SEM_PERGUNTAS" != 1 ]; then
    echo
    read -rp "Confirma a desinstalação do Arcadia? [s/N] " CONFIRMA
    [[ "$CONFIRMA" =~ ^[sS] ]] || { echo "Cancelado."; exit 0; }
fi

# --- 2/5 Atalho no menu de aplicativos + ícones -----------------------------
echo "==> 2/5 Removendo atalho e ícones"
rm -f "$APPS/arcadia.desktop" "$APPS/arcadia-native.desktop"
rm -f "$ICONS/arcadia.png" "$ICONS/arcadia-native.png"
rm -f "$PIXMAPS/arcadia.png" "$PIXMAPS/arcadia-native.png"
if [ -x "$HOME/.local/bin/arcadia-native" ]; then
    rm -f "$HOME/.local/bin/arcadia-native" "$HOME/.local/bin/arcadia-gamepad" \
        "$HOME/.local/bin/arcadia-source-helper" \
        "$HOME/.local/bin/boot.mp4" "$HOME/.local/bin/resources/torrent_rpc/main.py" \
        "$HOME/.local/bin/resources/torrent_rpc/torrent_downloader.py"
    rmdir "$HOME/.local/bin/resources/torrent_rpc" "$HOME/.local/bin/resources" 2>/dev/null || true
fi
update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

# --- 3/5 SLSsteam (opcional) -------------------------------------------------
if [ "$REMOVER_SLSSTEAM" = 1 ]; then
    echo "==> 3/5 Removendo SLSsteam"
    rm -rf "$HOME/.local/share/SLSsteam" "$HOME/.config/SLSsteam"
else
    echo "==> 3/5 SLSsteam preservada (nada a fazer)"
fi

# --- 4/5 Preserva jogos/prefixos pedidos antes de apagar o resto -----------
BACKUP="$HOME/arcadia-jogos-preservados"
if [ "$MANTER_JOGOS" = 1 ] && [ -d "$DIR/games" ]; then
    echo "==> 4/5 Movendo games/ para $BACKUP/games"
    mkdir -p "$BACKUP"
    mv "$DIR/games" "$BACKUP/games"
fi
if [ "$MANTER_PREFIXOS" = 1 ] && [ -d "$DIR/prefixes" ]; then
    echo "    Movendo prefixes/ para $BACKUP/prefixes"
    mkdir -p "$BACKUP"
    mv "$DIR/prefixes" "$BACKUP/prefixes"
fi

# --- 5/5 Remove o resto do diretório do projeto -----------------------------
echo "==> 5/5 Removendo $DIR"
rm -rf "$DIR"

# Instalação via gerenciador de pacotes (deb/rpm/AppImage): o AppImage não
# deixa rastro no sistema, mas deb/rpm podem ter instalado em /usr.
if command -v dpkg >/dev/null 2>&1 && dpkg -s arcadia >/dev/null 2>&1; then
    echo "    Detectada instalação .deb do Arcadia: rode 'sudo apt remove arcadia'."
fi
if command -v rpm >/dev/null 2>&1 && rpm -q arcadia >/dev/null 2>&1; then
    echo "    Detectada instalação .rpm do Arcadia: rode 'sudo dnf remove arcadia'."
fi

echo
echo "Arcadia desinstalado."
[ -d "$BACKUP" ] && echo "O que você pediu para manter ficou em: $BACKUP"
echo
echo "OBS: jogos Steam baixados pela loja do Arcadia (DepotDownloader) ficam"
echo "dentro da própria instalação da Steam e NÃO são tocados por este script."
echo "Para removê-los, use o botão \"Remover\" na Loja do Arcadia ANTES de"
echo "desinstalar — senão eles continuam instalados normalmente na Steam."
echo
echo "Pacotes de sistema usados pelo Arcadia continuam disponíveis a outros programas."
