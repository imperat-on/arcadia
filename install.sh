#!/usr/bin/env bash
# Instalação completa do Arcadia: dependências de sistema, front-end, config
# inicial e atalho no menu de aplicativos.
#
# Duas formas de usar:
#   Local:  ./install.sh
#   Remoto: curl -fsSL https://raw.githubusercontent.com/imperat-on/arcadia/master/install.sh | bash
#
# Flags:
#   -y, --yes   Não pergunta nada (assume sim para dependências; mantém o resto
#               do comportamento). Para rodar sem TTY.
set -e

DIR="$(cd "$(dirname "$0")" && pwd)"

# --- helpers ---------------------------------------------------------------
ERRO() { echo "ERRO: $*" >&2; }

# Confirma com o usuário; devolve 0 (sim) ou 1 (não). Com -y ou sem TTY,
# responde sim automaticamente para não travar a instalação.
PERGUNTAR() {
    local msg="$1"
    if [ "$SEM_PERGUNTAS" = 1 ]; then return 0; fi
    if [ ! -t 0 ]; then return 0; fi
    local r
    read -rp "$msg [s/N] " r
    [[ "$r" =~ ^[sS] ]]
}

SEM_PERGUNTAS=0
if [ "$1" = "-y" ] || [ "$1" = "--yes" ]; then SEM_PERGUNTAS=1; fi

DESTINO="$HOME/.local/share/arcadia"

# --- Modo remoto (curl | bash) --------------------------------------------
# Quando o script vem de stdin, "$0" resolve para o diretório atual — não é o
# repo. Clona para ~/.local/share/arcadia e re-executa de lá.
if [ ! -f "$DIR/app/package.json" ]; then
    REPO="https://github.com/imperat-on/arcadia.git"
    echo "==> Baixando o Arcadia para $DESTINO"
    if [ -d "$DESTINO/.git" ]; then
        # Já é um clone git: atualiza.
        git -C "$DESTINO" pull --ff-only
    elif [ -e "$DESTINO" ] && [ -n "$(ls -A "$DESTINO" 2>/dev/null)" ]; then
        # Existe MAS não é um clone válido (ex.: tentativa anterior interrompida,
        # pasta parcial). `git clone` falharia com "destination path already
        # exists and is not an empty directory" e o set -e abortaria sem copiar
        # nada. Move para backup e clona limpo.
        echo "    $DESTINO já existe mas não é um clone válido — movendo para backup e clonando de novo."
        mv "$DESTINO" "$DESTINO.bak-$(date +%Y%m%d-%H%M%S)"
        git clone "$REPO" "$DESTINO"
    else
        git clone "$REPO" "$DESTINO"
    fi
    exec bash "$DESTINO/install.sh" "$@"
fi

# --- Instalação MANUAL em pasta diferente de ~/.local/share/arcadia ---------
# O app (arcadia.sh, Electron) assume ~/.local/share/arcadia como
# base. Quem baixou o ZIP/clonou em outra pasta e rodou ./install.sh daqui
# instalaria numa pasta que o app não acha depois. Copia para o destino padrão
# e re-executa de lá.
if [ "$DIR" != "$DESTINO" ]; then
    echo "==> Instalando em $DESTINO (local padrão do app)"
    mkdir -p "$(dirname "$DESTINO")"
    # Dados do usuário que DEVEM ser preservados ao reinstalar por cima
    PRESERVAR=(config.json library.json contas games prefixes logs session.json owned_games.json sources.json sources_index.json torrent_state.json art trailers wine bin/plugins.json bin/deps bin/dotnet bin/tmp)
    BACKUP=""
    if [ -d "$DESTINO" ]; then
        # Move o destino atual para backup (proteção total) e devolve os dados
        # do usuário depois da cópia do código.
        BACKUP="$DESTINO.bak-$(date +%Y%m%d-%H%M%S)"
        echo "    $DESTINO já existe — movendo para backup ($BACKUP)."
        mv "$DESTINO" "$BACKUP"
    fi
    # Copia o código novo (a fonte é o repo/zip extraído)
    cp -a "$DIR/." "$DESTINO/"
    # Devolve os dados do usuário do backup (se havia instalação anterior)
    if [ -n "$BACKUP" ] && [ -d "$BACKUP" ]; then
        for item in "${PRESERVAR[@]}"; do
            if [ -e "$BACKUP/$item" ]; then
                mkdir -p "$(dirname "$DESTINO/$item")"
                cp -a "$BACKUP/$item" "$DESTINO/$item"
            fi
        done
        echo "    Dados preservados (config.json, biblioteca, contas, jogos…)."
    fi
    exec bash "$DESTINO/install.sh" "$@"
fi

echo "==> Instalando o Arcadia a partir de $DIR"

# --- 0/4 Dependências de sistema -------------------------------------------
# O app precisa de: python3 (torrent/arquivos auxiliares), steam (nativa), dotnet (roda o
# DepotDownloader), procps (pgrep/pkill p/ vigia de jogos), coreutils (du/df),
# yt-dlp (baixa os trailers), ffmpeg (junta vídeo+áudio e faz o remux), tar
# (extrai Proton/Wine e o SLSsteam), findutils, git e node/npm (front-end).
echo "==> 0/4 Verificando dependências de sistema"
FALTAM=()
for cmd in python3 steam dotnet pgrep du df yt-dlp ffmpeg tar find git node npm unzip; do
    command -v "$cmd" >/dev/null 2>&1 || FALTAM+=("$cmd")
done

if [ ${#FALTAM[@]} -gt 0 ]; then
    echo "    Faltando: ${FALTAM[*]}"
    # Mapeia comandos -> pacotes por distro.
    declare -A PKG_ARCH=( [python3]=python [steam]=steam [dotnet]=dotnet-runtime [pgrep]=procps-ng [du]=coreutils [df]=coreutils [yt-dlp]=yt-dlp [ffmpeg]=ffmpeg [tar]=tar [find]=findutils [git]=git [node]=nodejs [npm]=npm [unzip]=unzip )
    declare -A PKG_DEB=(  [python3]=python3 [steam]=steam [dotnet]=dotnet-runtime-9.0 [pgrep]=procps [du]=coreutils [df]=coreutils [yt-dlp]=yt-dlp [ffmpeg]=ffmpeg [tar]=tar [find]=findutils [git]=git [node]=nodejs [npm]=npm [unzip]=unzip )
    PKGS=()
    if command -v pacman >/dev/null 2>&1; then
        for c in "${FALTAM[@]}"; do PKGS+=("${PKG_ARCH[$c]}"); done
        echo "    Vou instalar via pacman (sudo): ${PKGS[*]}"
        if PERGUNTAR "    OK instalar essas dependências?"; then
            sudo pacman -S --needed --noconfirm "${PKGS[@]}"
        else
            echo "    Dependências não instaladas — o app pode não funcionar."
        fi
    elif command -v apt-get >/dev/null 2>&1; then
        for c in "${FALTAM[@]}"; do PKGS+=("${PKG_DEB[$c]}"); done
        echo "    Vou instalar via apt (sudo): ${PKGS[*]}"
        if PERGUNTAR "    OK instalar essas dependências?"; then
            sudo apt-get update && sudo apt-get install -y "${PKGS[@]}"
        else
            echo "    Dependências não instaladas — o app pode não funcionar."
        fi
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
if npm install; then
    echo "    npm install OK"
else
    ERRO "npm install falhou. Verifique sua conexão e o Node/npm."
    exit 1
fi

# O binário do Electron (~100MB) é baixado por um postinstall que, em algumas
# configs de npm, é pulado ou falha silenciosamente — deixando o app sem "motor"
# e o arcadia.sh reclamando de "No such file or directory". Garante aqui.
# Checamos o binário DE VERDADE: o require('electron') só devolve o caminho do
# binário como string e "passa" mesmo com o download ausente/corrompido.
if [ ! -x "node_modules/electron/dist/electron" ]; then
    echo "    Electron: binário ausente, baixando…"
    node node_modules/electron/install.js 2>/dev/null || npm rebuild electron
    # O install.js do electron usa o extract-zip 2.0.1, que trava silenciosamente
    # (sai com exit 0 sem extrair nada) em Node >= 22. Se o binário continuar
    # ausente, extraímos na mão o zip que ele já baixou no cache (~/.cache/electron).
    if [ ! -x "node_modules/electron/dist/electron" ]; then
        VER="$(node -p "require('./node_modules/electron/package.json').version")"
        ZIP="$(find "$HOME/.cache/electron" -name "electron-v${VER}-linux-x64.zip" -print -quit 2>/dev/null)"
        if [ -n "$ZIP" ]; then
            echo "    install.js não extraiu o binário — extraindo do cache com unzip…"
            unzip -o -q "$ZIP" -d node_modules/electron/dist
            printf 'electron' > node_modules/electron/path.txt
        fi
    fi
    if [ ! -x "node_modules/electron/dist/electron" ]; then
        ERRO "falha ao baixar o Electron — rode 'cd app && npm rebuild electron'"
        exit 1
    fi
    echo "    Electron OK"
fi

# O dist/ não vai no git (é gerado). O arcadia.sh reconstrói sozinho quando
# falta, mas aí a PRIMEIRA abertura trava alguns segundos sem explicação.
# Compilando aqui, o app abre instantâneo desde a primeira vez.
echo "    Compilando o front-end…"
if npm run build; then
    echo "    Build OK"
else
    ERRO "build do front-end falhou."
    exit 1
fi

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
if "$DIR/install-desktop.sh"; then
    echo "    Atalho criado."
else
    ERRO "falha ao criar o atalho (install-desktop.sh) — o app ainda funciona, sem o ícone no menu."
fi

# --- 4/4 Resumo -------------------------------------------------------------
echo ""
echo "Pronto! Rode:  ./arcadia-desktop.sh   (modo desktop)"
echo "         ou:   ./arcadia.sh           (modo console/tela cheia)"
echo ""
echo "Opcionais dentro do app (botões): .NET local, SLSsteam, SLScheevo,"
echo "versões de Wine/Proton — tudo se instala pela interface."
echo ""
echo "Para desinstalar: ./uninstall.sh  (pergunta o que preservar)"
