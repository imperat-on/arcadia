#!/usr/bin/env bash
# Empacotamento Linux do Arcadia (shell Tauri/Rust).
#
# Gera .deb e AppImage com o ambiente de mídia correto.
#
# DOIS PROBLEMAS QUE ESTE SCRIPT RESOLVE
# -------------------------------------
#
# 1. CODECS QUE FALTAM
#
# O `<video>` da UI usa WebKitGTK, que no Linux delega toda decodificação ao
# GStreamer do sistema. O `bundleMediaFramework` do Tauri embarca os plugins
# básicos (~194), mas deixa de fora justamente os patenteados:
#
#   libgstopenh264.so   decoder H.264       <- trailers da Steam
#   libgsthls.so        demuxer HLS         <- todo trailer moderno
#   libgstlibav.so      decoder via ffmpeg  (alternativa, ~130 libs)
#
# Sem eles o trailer é uma tela preta silenciosa. Este script injeta
# openh264 + hls + libopenh264 dentro do AppDir e reempacota. openh264 vem
# no lugar do libav de propósito: cobre H.264 com 143 KB em vez de arrastar
# o ffmpeg inteiro.
#
# 2. A PILHA GRÁFICA DO HOST DE BUILD
#
# O linuxdeploy copia para `<AppDir>/usr/lib` uma pilha GTK/WebKit/glib inteira
# do host que construiu o pacote, e o runtime do AppImage prefixa `usr/lib` no
# LD_LIBRARY_PATH — então tudo que está lá VENCE o sistema do usuário.
# Empacotado no CI (Ubuntu 22.04) isso significa entregar WebKitGTK 2.36 de
# 2022 e glib 2.72 para quem roda Arch com WebKit 2.54 e glib 2.90: o
# compositor briga entre duas pilhas gráficas e a interface fica arrastada.
# Medido antes da correção: 81% de um núcleo em repouso; depois: 2%.
#
# `trim_appdir_graphics` remove essa pilha depois do bundle. WebKit, GTK,
# GStreamer e glib passam a vir do sistema (casados com driver de vídeo e
# compositor), e o AppImage leva só o binário e os codecs. Efeito colateral
# bem-vindo: 147 MB -> 39 MB.
#
# USO
#   ./scripts/package-linux.sh              # deb + appimage + rpm
#   ./scripts/package-linux.sh --deb        # só .deb
#   ./scripts/package-linux.sh --appimage   # só AppImage
#   ./scripts/package-linux.sh --rpm        # só .rpm
#   ./scripts/package-linux.sh --verify     # empacota os três e valida
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI="$ROOT/ui/node_modules/.bin/tauri"
BUNDLE="$ROOT/target/release/bundle"
APPDIR="$BUNDLE/appimage/Arcadia.AppDir"

# Plugin GStreamer que precisa ser embarcado: nome do .so no AppDir.
GST_PLUGINS=(libgstopenh264.so libgsthls.so)
# Bibliotecas de sistema que esses plugins exigem. A versão NÃO é fixa: o
# plugin vem do host que empacota, e cada distro compila contra a SONAME da
# sua época. Fixar `libopenh264.so.8` deixou o AppImage do CI (Ubuntu 22.04)
# com um `libgstopenh264.so` que pedia `.so.6` e nunca carregava — o decoder
# H.264 simplesmente não existia em runtime e o vídeo ficava em buffering
# infinito. `resolver_gst_libs` lê o `ldd` do próprio plugin e descobre o nome
# certo; a lista acima fica só como fallback quando o ldd não roda.
GST_LIBS=(libopenh264.so.8)

DO_DEB=0
DO_APPIMAGE=0
DO_RPM=0
DO_VERIFY=0
for arg in "$@"; do
    case "$arg" in
        --deb) DO_DEB=1 ;;
        --appimage) DO_APPIMAGE=1 ;;
        --rpm) DO_RPM=1 ;;
        --verify) DO_VERIFY=1; DO_DEB=1; DO_APPIMAGE=1; DO_RPM=1 ;;
        -h|--help) sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "arg desconhecido: $arg" >&2; exit 2 ;;
    esac
done
if [ $DO_DEB -eq 0 ] && [ $DO_APPIMAGE -eq 0 ] && [ $DO_RPM -eq 0 ]; then
    DO_DEB=1; DO_APPIMAGE=1; DO_RPM=1
fi

# --- Pré-requisitos ---------------------------------------------------------

if [ ! -x "$TAURI" ]; then
    echo "erro: Tauri CLI não encontrado em $TAURI" >&2
    echo "  instale com: npm --prefix ui i -D @tauri-apps/cli@2" >&2
    exit 1
fi

export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "erro: cargo fora do PATH" >&2; exit 1; }

# patchelf é exigido pelo bundler Linux. Arch não traz por padrão; aceita uma
# cópia local extraída sem root.
if ! command -v patchelf >/dev/null 2>&1; then
    if [ -x "$ROOT/.tools/patchelf" ]; then
        export PATH="$ROOT/.tools:$PATH"
    else
        echo "erro: patchelf ausente (necessário para deb/appimage)" >&2
        echo "  Arch:  sudo pacman -S patchelf" >&2
        exit 1
    fi
fi

# --- Localiza os plugins de codec no host -----------------------------------

# Onde procurar: os plugins extraídos pelo usuário primeiro, depois o sistema.
# O caminho depende da distro: Arch/Fedora usam `/usr/lib/gstreamer-1.0`, e as
# derivadas Debian (Ubuntu do CI) usam o diretório multiarch
# `/usr/lib/<tripla>/gstreamer-1.0`. Procurar só no primeiro fazia o job de
# empacotamento falhar mesmo com `gstreamer1.0-plugins-bad` instalado.
gst_dir_candidates=(
    "$HOME/.local/lib/gstreamer-1.0"
    "/usr/lib/gstreamer-1.0"
    "/usr/lib/x86_64-linux-gnu/gstreamer-1.0"
    "/usr/lib/aarch64-linux-gnu/gstreamer-1.0"
    "/usr/lib64/gstreamer-1.0"
)
# Complementa com o que o próprio `pkg-config` ou o `gst-inspect` reportarem,
# para cobrir triplas que não estão na lista fixa.
if command -v gst-inspect-1.0 >/dev/null 2>&1; then
    while IFS= read -r dir; do
        [ -n "$dir" ] && gst_dir_candidates+=("$dir")
    done < <(gst-inspect-1.0 --plugins 2>/dev/null \
        | sed -n 's|^ *Filename *\(.*\)/[^/]*$|\1|p' | sort -u)
fi
if command -v pkg-config >/dev/null 2>&1; then
    pc_dir=$(pkg-config --variable=pluginsdir gstreamer-1.0 2>/dev/null || true)
    [ -n "$pc_dir" ] && gst_dir_candidates+=("$pc_dir")
fi

find_gst_plugin() {
    local name="$1" dir
    for dir in "${gst_dir_candidates[@]}"; do
        [ -e "$dir/$name" ] && { echo "$dir/$name"; return 0; }
    done
    return 1
}

missing=()
for p in "${GST_PLUGINS[@]}"; do
    find_gst_plugin "$p" >/dev/null || missing+=("$p")
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "erro: plugins GStreamer ausentes no host: ${missing[*]}" >&2
    echo "  Arch: sudo pacman -S gst-plugins-bad" >&2
    exit 1
fi

# --- Build + bundle --------------------------------------------------------

bundles=()
[ $DO_DEB -eq 1 ] && bundles+=(deb)
[ $DO_APPIMAGE -eq 1 ] && bundles+=(appimage)
[ $DO_RPM -eq 1 ] && bundles+=(rpm)

echo "==> compilando e empacotando: ${bundles[*]}"
# O binário do wrapper XInput é dependência do bundle: sem ele compilado, o
# `tauri build` falha ao copiar o recurso. `-p arcadia` não o constrói porque
# são binários separados do mesmo workspace.
cargo build --release -p arcadia-gamepad
cd "$ROOT/src-tauri"
"$TAURI" build --bundles "$(IFS=,; echo "${bundles[*]}")"

# --- Remove a pilha gráfica do AppDir --------------------------------------
#
# O linuxdeploy copia para `<AppDir>/usr/lib` uma pilha GTK/WebKit/glib inteira
# do host que construiu o pacote. O runtime do AppImage prefixa `usr/lib` no
# LD_LIBRARY_PATH, então **tudo que está lá vence o sistema**. Empacotado no CI
# (Ubuntu 22.04) isso significa entregar WebKitGTK 2.36 de 2022, GTK 3.24.33 e
# glib 2.72 para quem roda Arch com WebKit 2.54 e glib 2.90 — medido: 3x mais
# CPU em repouso e a interface arrastada, com o compositor brigando entre duas
# pilhas gráficas.
#
# A pilha gráfica é responsabilidade do sistema: WebKitGTK, GTK3, GStreamer e
# glib precisam ser os do host, casados com o driver de vídeo e o compositor.
# O que o AppImage deve carregar é só o binário do app e os codecs que faltam.
trim_appdir_graphics() {
    [ -d "$APPDIR" ] || { echo "aviso: AppDir ausente, pulando limpeza"; return 0; }

    local lib="$APPDIR/usr/lib"
    local antes; antes="$(find "$lib" -name '*.so*' 2>/dev/null | wc -l)"

    # Tudo que é biblioteca de sistema sai. O binário do app fica em usr/bin e
    # os recursos em usr/share — nenhum dos dois é tocado aqui.
    find "$lib" -maxdepth 1 -name '*.so*' -delete 2>/dev/null || true
    rm -rf "$lib"/x86_64-linux-gnu "$lib"/gdk-pixbuf-2.0 "$lib"/gtk-3.0 \
           "$lib"/gio "$lib"/girepository-1.0 2>/dev/null || true

    # O hook do linuxdeploy aponta GTK_PATH/GDK_PIXBUF_MODULE_FILE para dentro
    # do bundle. Sem as libs correspondentes ele só aponta para o vazio, então
    # sai junto.
    if [ -f "$APPDIR/AppRun" ]; then
        sed -i '/linuxdeploy-plugin-gtk/d' "$APPDIR/AppRun" 2>/dev/null || true
    fi
    rm -f "$APPDIR"/apprun-hooks/linuxdeploy-plugin-gtk.sh 2>/dev/null || true

    local depois; depois="$(find "$lib" -name '*.so*' 2>/dev/null | wc -l)"
    echo "==> pilha gráfica removida do AppDir: $antes -> $depois bibliotecas"
    echo "    WebKit/GTK/GStreamer/glib passam a vir do sistema do usuário"
}

if [ $DO_APPIMAGE -eq 1 ]; then
    trim_appdir_graphics
fi

# --- Injeta os codecs no AppDir --------------------------------------------

inject_appimage_codecs() {
    [ -d "$APPDIR" ] || { echo "aviso: AppDir ausente, pulando injeção"; return 0; }

    local dst="$APPDIR/usr/lib/gstreamer-1.0"
    mkdir -p "$dst"
    local injected=0

    for p in "${GST_PLUGINS[@]}"; do
        local src; src="$(find_gst_plugin "$p")"
        cp -f "$src" "$dst/$p"
        # Fixa as dependências de runtime do plugin (ex.: libopenh264) para que
        # o loader do AppRun as resolva de dentro do pacote.
        if command -v patchelf >/dev/null 2>&1; then
            patchelf --set-rpath '$ORIGIN/..' "$dst/$p" 2>/dev/null || true
        fi
        echo "    + $p"
        injected=$((injected + 1))

        # A SONAME que o plugin pede, lida do binário. Cada distro compila o
        # gst-plugins-bad contra a libopenh264 da sua época: o plugin do CI
        # (Ubuntu 22.04) pede `libopenh264.so.6`, o de um Arch atual pede
        # `.so.8`. Fixar um nome deixou o AppImage publicado com o decoder
        # quebrado — o vídeo ficava em buffering infinito, sem erro visível.
        #
        # Estratégia em duas etapas:
        #   1. se a SONAME exata existe no host, copia;
        #   2. se não existe, copia a versão que houver e cria um symlink com
        #      o nome pedido. A ABI do libopenh264 é estável entre essas
        #      versões (o decoder só é chamado pelo plugin), e é isso ou
        #      entregar um pacote sem H.264.
        local faltando
        faltando="$(ldd "$dst/$p" 2>/dev/null | awk '/not found/ {print $1}' | sort -u)"
        for so in $faltando; do
            local alvo=""
            for dir in /usr/lib /usr/lib/x86_64-linux-gnu /lib /lib/x86_64-linux-gnu \
                       "$HOME/.local/lib" "$HOME/.local/lib64"; do
                [ -e "$dir/$so" ] && { alvo="$dir/$so"; break; }
            done
            if [ -n "$alvo" ]; then
                cp -fL "$alvo" "$APPDIR/usr/lib/$so"
                echo "    + $so (de $alvo)"
                injected=$((injected + 1))
                continue
            fi
            # Etapa 2: procura qualquer versão do mesmo nome-base.
            local base="${so%%.so*}"
            local outra=""
            for dir in /usr/lib /usr/lib/x86_64-linux-gnu /lib /lib/x86_64-linux-gnu \
                       "$HOME/.local/lib" "$HOME/.local/lib64"; do
                outra="$(find "$dir" -maxdepth 1 -name "${base}.so.*" 2>/dev/null | sort -V | tail -1)"
                [ -n "$outra" ] && break
            done
            if [ -n "$outra" ]; then
                cp -fL "$outra" "$APPDIR/usr/lib/$(basename "$outra")"
                ln -sf "$(basename "$outra")" "$APPDIR/usr/lib/$so"
                echo "    + $so -> $(basename "$outra") (mesma ABI, nome ajustado)"
                injected=$((injected + 1))
            else
                echo "    AVISO: $p precisa de $so e nenhuma versão está no host" >&2
            fi
        done
    done

    # Fallback: a lista fixa cobre o caso em que o ldd acima não resolve nada
    # (host sem o plugin de origem). Só adiciona o que ainda faltar.
    for lib in "${GST_LIBS[@]}"; do
        [ -e "$APPDIR/usr/lib/$lib" ] && continue
        local real; real="$(readlink -f "/usr/lib/$lib" 2>/dev/null || true)"
        [ -n "$real" ] && [ -e "$real" ] || continue
        cp -fL "$real" "$APPDIR/usr/lib/$lib"
        echo "    + $lib (fallback)"
        injected=$((injected + 1))
    done

    echo "==> $injected arquivo(s) de codec injetado(s) no AppDir"

    # Reempacota o AppDir no AppImage final.
    local runtime="$HOME/.cache/tauri/AppRun-x86_64"
    local mkapp="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"
    if [ ! -x "$runtime" ] || [ ! -x "$mkapp" ]; then
        echo "aviso: runtime do AppImage ausente no cache do Tauri —" >&2
        echo "       o AppImage publicado NÃO conterá os codecs." >&2
        return 0
    fi

    local out; out="$(ls -t "$BUNDLE/appimage"/*.AppImage | head -1)"
    echo "==> reempacotando $out"
    # O plugin standalone lê ARCH e VERSION do ambiente e escreve o AppImage
    # ao lado do AppDir.
    ARCH=x86_64 VERSION="$(node -p "require('$ROOT/ui/package.json').version" 2>/dev/null || echo 1.5.5)" \
        OUTPUT="$BUNDLE/appimage/$(basename "$out")" \
        "$mkapp" --appdir="$APPDIR" 2>&1 | tail -5
}

if [ $DO_APPIMAGE -eq 1 ]; then
    inject_appimage_codecs
fi

# --- Relatório -------------------------------------------------------------

echo
echo "==> artefatos"
ls -lh "$BUNDLE"/deb/*.deb "$BUNDLE"/appimage/*.AppImage "$BUNDLE"/rpm/*.rpm 2>/dev/null \
    | awk '{print "    "$5"\t"$9}'

if [ $DO_VERIFY -eq 1 ]; then
    echo
    exec "$ROOT/scripts/verify-package.sh"
fi
