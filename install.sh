#!/usr/bin/env bash
# Instalação completa do Arcadia: dependências de sistema, front-end, config
# inicial e atalho no menu de aplicativos.
#
# Duas formas de usar:
#   Local:  ./install.sh
#   Remoto: curl -fsSL https://raw.githubusercontent.com/imperat-on/arcadia/master/install.sh | bash
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"

# Modo curl|bash: se não estamos dentro do repo, clona primeiro e re-executa.
if [ ! -f "$DIR/app/package.json" ]; then
    REPO="https://github.com/imperat-on/arcadia.git"
    DESTINO="$HOME/.local/share/arcadia"
    echo "==> Baixando o Arcadia para $DESTINO"
    if [ -d "$DESTINO/.git" ]; then
        git -C "$DESTINO" pull --ff-only
    else
        git clone "$REPO" "$DESTINO"
    fi
    exec bash "$DESTINO/install.sh"
fi

# --- 0/4 Dependências de sistema -------------------------------------------
# O app precisa de: python3 (indexador), steam (nativa), dotnet (roda o
# DepotDownloader), procps (pgrep/pkill p/ vigia de jogos), coreutils (du/df),
# yt-dlp (baixa os trailers), ffmpeg (junta vídeo+áudio e faz o remux), tar
# (extrai Proton/Wine e o SLSsteam), findutils, git e node/npm (front-end).
echo "==> 0/4 Verificando dependências de sistema"
FALTAM=()
for cmd in python3 steam dotnet pgrep du df yt-dlp ffmpeg tar find git node npm; do
    command -v "$cmd" >/dev/null 2>&1 || FALTAM+=("$cmd")
done

if [ ${#FALTAM[@]} -gt 0 ]; then
    echo "    Faltando: ${FALTAM[*]}"
    # Mapeia comandos -> pacotes por distro.
    declare -A PKG_ARCH=( [python3]=python [steam]=steam [dotnet]=dotnet-runtime [pgrep]=procps-ng [du]=coreutils [df]=coreutils [yt-dlp]=yt-dlp [ffmpeg]=ffmpeg [tar]=tar [find]=findutils [git]=git [node]=nodejs [npm]=npm )
    declare -A PKG_DEB=(  [python3]=python3 [steam]=steam [dotnet]=dotnet-runtime-9.0 [pgrep]=procps [du]=coreutils [df]=coreutils [yt-dlp]=yt-dlp [ffmpeg]=ffmpeg [tar]=tar [find]=findutils [git]=git [node]=nodejs [npm]=npm )
    PKGS=()
    if command -v pacman >/dev/null 2>&1; then
        for c in "${FALTAM[@]}"; do PKGS+=("${PKG_ARCH[$c]}"); done
        echo "    Instalando via pacman (sudo): ${PKGS[*]}"
        sudo pacman -S --needed --noconfirm "${PKGS[@]}"
    elif command -v apt-get >/dev/null 2>&1; then
        for c in "${FALTAM[@]}"; do PKGS+=("${PKG_DEB[$c]}"); done
        echo "    Instalando via apt (sudo): ${PKGS[*]}"
        sudo apt-get update && sudo apt-get install -y "${PKGS[@]}"
    else
        echo "    AVISO: distro não reconhecida — instale manualmente: ${FALTAM[*]}"
    fi
else
    echo "    Tudo presente."
fi

# Opcional: o yt-dlp usa o Deno para resolver o desafio JS do YouTube, exigido
# só em vídeos com restrição de idade. Sem ele o resto dos trailers funciona,
# então nunca abortamos a instalação por causa disso.
if ! command -v deno >/dev/null 2>&1; then
    echo "    Deno (opcional, trailers com restrição de idade): instalando…"
    if command -v pacman >/dev/null 2>&1; then
        sudo pacman -S --needed --noconfirm deno || echo "    (sem Deno — trailers +18 podem falhar)"
    else
        echo "    (sem Deno — instale manualmente se quiser trailers +18)"
    fi
fi

# --- 1/4 Front-end ----------------------------------------------------------
echo "==> 1/4 Dependências do front-end (npm install)"
cd "$DIR/app"
npm install

# O binário do Electron (~100MB) é baixado por um postinstall que, em algumas
# configs de npm, é pulado ou falha silenciosamente — deixando o app sem "motor"
# e o arcadia.sh reclamando de "No such file or directory". Garante aqui.
if ! node -e "require('electron')" >/dev/null 2>&1; then
    echo "    Electron: binário ausente, baixando…"
    node node_modules/electron/install.js 2>/dev/null || npm rebuild electron || \
        echo "    AVISO: falha ao baixar o Electron — rode 'cd app && npm rebuild electron'"
fi

# O dist/ não vai no git (é gerado). O arcadia.sh reconstrói sozinho quando
# falta, mas aí a PRIMEIRA abertura trava alguns segundos sem explicação.
# Compilando aqui, o app abre instantâneo desde a primeira vez.
echo "    Compilando o front-end…"
npm run build

# --- 2/4 Configuração -------------------------------------------------------
echo "==> 2/4 Configuração inicial"
if [ ! -f "$DIR/config.json" ]; then
    cp "$DIR/config.example.json" "$DIR/config.json"
    echo "    config.json criado — edite e cole suas chaves (Steam API / Hubcap)."
else
    echo "    config.json já existe (mantido)."
fi

# --- 3/4 Atalho -------------------------------------------------------------
echo "==> 3/4 Atalho no menu de aplicativos"
"$DIR/install-desktop.sh"

# --- 4/4 Resumo -------------------------------------------------------------
echo ""
echo "Pronto! Rode:  ./arcadia-desktop.sh   (modo desktop)"
echo "         ou:   ./arcadia.sh           (modo console/tela cheia)"
echo ""
echo "Opcionais dentro do app (botões): .NET local, SLSsteam, SLScheevo,"
echo "versões de Wine/Proton — tudo se instala pela interface."
