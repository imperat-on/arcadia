#!/usr/bin/env bash
# Desinstalação completa do Arcadia.
#
# Duas formas de usar:
#   Local:  ./uninstall.sh
#   Remoto: curl -fsSL https://raw.githubusercontent.com/imperat-on/arcadia/master/uninstall.sh | bash
#
# Flags:
#   -y, --yes   Não pergunta nada: mantém jogos/prefixos/SLSsteam (move-os
#               para fora do diretório antes de apagar) e não pede a
#               confirmação final. Para rodar sem TTY (ex.: outro script).
set -e
DIR="$HOME/.local/share/arcadia"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/512x512/apps"
PIXMAPS="$HOME/.local/share/pixmaps"
SEM_PERGUNTAS=0
if [ "$1" = "-y" ] || [ "$1" = "--yes" ]; then SEM_PERGUNTAS=1; fi

# Sem TTY (ex.: curl … | bash) não dá pra ler resposta: assume modo -y
# em vez de travar/pular a confirmação.
if [ "$SEM_PERGUNTAS" = 0 ] && [ ! -t 0 ]; then
    echo "Sem terminal interativo — assumindo modo não-interativo (preserva jogos/prefixos/SLSsteam)."
    SEM_PERGUNTAS=1
fi

if [ ! -d "$DIR" ]; then
    echo "Arcadia não encontrado em $DIR — nada para desinstalar."
    exit 0
fi

echo "Isso vai remover o Arcadia deste computador ($DIR)."
echo

# --- 0/5 Fecha o app, se estiver aberto -------------------------------------
if pgrep -f "$DIR/app/node_modules/electron" >/dev/null 2>&1; then
    echo "==> 0/5 Fechando o Arcadia…"
    pkill -f "$DIR/app/node_modules/electron" 2>/dev/null || true
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
rm -f "$APPS/arcadia.desktop"
rm -f "$ICONS/arcadia.png"
rm -f "$PIXMAPS/arcadia.png"
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

echo
echo "Arcadia desinstalado."
[ -d "$BACKUP" ] && echo "O que você pediu para manter ficou em: $BACKUP"
echo
echo "OBS: jogos Steam baixados pela loja do Arcadia (DepotDownloader) ficam"
echo "dentro da própria instalação da Steam e NÃO são tocados por este script."
echo "Para removê-los, use o botão \"Remover\" na Loja do Arcadia ANTES de"
echo "desinstalar — senão eles continuam instalados normalmente na Steam."
echo
echo "Dependências de sistema instaladas pelo install.sh (steam, dotnet,"
echo "ffmpeg, node, etc.) não são removidas — são pacotes comuns que outros"
echo "programas podem estar usando."
