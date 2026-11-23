#!/usr/bin/env bash
# Empacotamento Linux do Arcadia (Rust + iced).
#
# Gera um tarball portátil em dist/native-linux/ com o binário release,
# wrapper de gamepad, worker torrent, boot.mp4, atalho .desktop e ícone.
# O SHA256SUMS fica ao lado do tarball.
#
# Uso:
#   ./scripts/package-native-linux.sh                  # compila release e empacota
#   ./scripts/package-native-linux.sh --skip-build     # usa binário já compilado
#   ./scripts/package-native-linux.sh --output /tmp/x  # muda o diretório de saída
#
# Variáveis:
#   ARCADIA_NATIVE_BIN       binário alternativo (útil com --skip-build)
#   ARCADIA_GAMEPAD_BIN      wrapper de gamepad alternativo (com --skip-build)
#   ARCADIA_NATIVE_VERSION   sobrescreve a versão lida do workspace
#   ARCADIA_NATIVE_ICON      sobrescreve o ícone procurado no repositório
#   CARGO_BUILD_JOBS         repassado ao Cargo, se definido
#   CARGO_TARGET_DIR         respeitado no build e na busca do binário
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

SKIP_BUILD=0
OUTPUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --skip-build) SKIP_BUILD=1 ;;
        --output)
            [ $# -ge 2 ] || { echo "erro: --output exige um diretório" >&2; exit 2; }
            OUTPUT="$2"
            shift
            ;;
        --output=*) OUTPUT="${1#--output=}" ;;
        -h|--help)
            sed -n '2,/^set -euo pipefail$/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "erro: argumento desconhecido: $1" >&2; exit 2 ;;
    esac
    shift
done

# --- Versão e diretórios ----------------------------------------------------

VERSION="${ARCADIA_NATIVE_VERSION:-}"
if [ -z "$VERSION" ]; then
    # Lê `version` de [workspace.package], não a primeira versão de dependência.
    VERSION="$(awk '
        /^\[workspace\.package\]/ { section = 1; next }
        /^\[/ { section = 0 }
        section && /^version[[:space:]]*=/ {
            gsub(/["[:space:]]/, "", $3)
            print $3
            exit
        }
    ' "$ROOT/Cargo.toml")"
fi
[ -n "$VERSION" ] || {
    echo "erro: não foi possível ler a versão em $ROOT/Cargo.toml" >&2
    echo "  defina ARCADIA_NATIVE_VERSION=<versão> para continuar" >&2
    exit 1
}

ARCH="$(uname -m)"
PKG="arcadia-native-$VERSION-linux-$ARCH"

if [ -z "$OUTPUT" ]; then
    OUTPUT="$ROOT/dist/native-linux"
else
    case "$OUTPUT" in
        /*) ;;
        *) OUTPUT="$ROOT/$OUTPUT" ;;
    esac
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in
    /*) ;;
    *) TARGET_DIR="$ROOT/$TARGET_DIR" ;;
esac
BIN="${ARCADIA_NATIVE_BIN:-$TARGET_DIR/release/arcadia-native}"
case "$BIN" in
    /*) ;;
    *) BIN="$ROOT/$BIN" ;;
esac
GAMEPAD="${ARCADIA_GAMEPAD_BIN:-$TARGET_DIR/release/arcadia-gamepad}"
case "$GAMEPAD" in
    /*) ;;
    *) GAMEPAD="$ROOT/$GAMEPAD" ;;
esac

# --- Build ------------------------------------------------------------------

if [ "$SKIP_BUILD" -eq 0 ]; then
    command -v cargo >/dev/null 2>&1 || {
        echo "erro: cargo não encontrado no PATH" >&2
        exit 1
    }
    echo "==> cargo build --locked --release -p arcadia-native -p arcadia-gamepad"
    if [ -n "${CARGO_BUILD_JOBS:-}" ]; then
        echo "    CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS"
    fi
    ( cd "$ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" cargo build --locked --release -p arcadia-native -p arcadia-gamepad )
fi

if [ ! -f "$BIN" ]; then
    echo "erro: binário nativo não encontrado em $BIN" >&2
    if [ "$SKIP_BUILD" -eq 1 ]; then
        echo "  --skip-build foi usado; compile antes com:" >&2
        echo "    cargo build --locked --release -p arcadia-native -p arcadia-gamepad" >&2
    fi
    echo "  ou aponte ARCADIA_NATIVE_BIN para outro executável" >&2
    exit 1
fi
if [ ! -x "$BIN" ]; then
    echo "erro: $BIN existe mas não é executável" >&2
    exit 1
fi
[ -f "$GAMEPAD" ] && [ -x "$GAMEPAD" ] || {
    echo "erro: wrapper de gamepad não encontrado ou não executável em $GAMEPAD" >&2
    exit 1
}

# --- Staging ----------------------------------------------------------------

STAGE="$OUTPUT/$PKG"
rm -rf "$STAGE"
mkdir -p "$STAGE"

install -m 0755 "$BIN" "$STAGE/arcadia-native"
install -m 0755 "$GAMEPAD" "$STAGE/arcadia-gamepad"
echo "==> binário: $BIN"

# O app procura boot.mp4 ao lado do executável e em resources/boot.mp4.
BOOT="$ROOT/boot.mp4"
if [ -f "$BOOT" ]; then
    install -m 0644 "$BOOT" "$STAGE/boot.mp4"
    echo "==> boot.mp4: $BOOT"
else
    echo "aviso: boot.mp4 não encontrado; o app abre sem a tela de abertura" >&2
fi

ICON="${ARCADIA_NATIVE_ICON:-$ROOT/crates/arcadia-native/assets/icons/icon.png}"
install -m 0644 "$ICON" "$STAGE/arcadia-native.png"
echo "==> ícone: $ICON"

mkdir -p "$STAGE/resources/torrent_rpc"
install -m 0644 "$ROOT/resources/torrent_rpc/main.py" \
    "$ROOT/resources/torrent_rpc/torrent_downloader.py" "$STAGE/resources/torrent_rpc/"

cat > "$STAGE/arcadia-native.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Arcadia
GenericName=Game Library
Comment=Biblioteca e launcher de jogos do Arcadia
Exec=arcadia-native
Icon=arcadia-native
Terminal=false
Categories=Game;
StartupWMClass=arcadia-native
EOF

{
    printf '# Arcadia Native %s\n\n' "$VERSION"
    cat <<'EOF'
Arcadia em Rust + iced.

## Conteúdo

- `arcadia-native` — binário do aplicativo;
- `arcadia-gamepad` — wrapper Linux de controle para jogos externos;
- `resources/torrent_rpc/` — worker de download por torrent (Python);
- `boot.mp4` — vídeo de abertura, procurado ao lado do binário;
- `arcadia-native.desktop` — atalho para ambientes X11/Wayland;
- `arcadia-native.png` — ícone do aplicativo.

## Dependências de runtime

- GStreamer 1.0 com os plugins base, good e bad. Para H.264 também serve o
  libav. Debian/Ubuntu:

      sudo apt install gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav libudev1 libdav1d7 libgtk-3-0t64 libwebkit2gtk-4.1-0

  Fedora: `gstreamer1-plugins-base`, `gstreamer1-plugins-good`,
  `gstreamer1-plugins-bad-free`. Arch: `gst-plugins-base`, `gst-plugins-good`,
  `gst-plugins-bad`.
- Driver gráfico com Vulkan ou OpenGL e sessão X11/Wayland. O iced usa wgpu.
- `libudev` para gamepad, `dav1d` para capas AVIF e GTK 3/WebKitGTK 4.1 para a
  janela isolada de verificação de fontes.
- Python 3 com `libtorrent` para downloads por torrent.
- Fontes Inter são embutidas no binário. Não é preciso instalar fonte do
  sistema para a interface.
- Rede é opcional para abrir a biblioteca local; capas, trailers e conta
  dependem do backend configurado.

O GStreamer não vai dentro do tarball: ele precisa vir da distribuição, casado
com o driver de vídeo e o compositor do usuário.

## Instalar

    mkdir -p ~/.local/bin/resources/torrent_rpc ~/.local/share/applications ~/.local/share/icons/hicolor/512x512/apps
    install -m 0755 arcadia-native arcadia-gamepad ~/.local/bin/
    install -m 0644 boot.mp4 ~/.local/bin/
    install -m 0644 resources/torrent_rpc/*.py ~/.local/bin/resources/torrent_rpc/
    install -m 0644 arcadia-native.desktop ~/.local/share/applications/
    install -m 0644 arcadia-native.png ~/.local/share/icons/hicolor/512x512/apps/

Depois é só abrir "Arcadia" no menu ou rodar `arcadia-native` (com
`~/.local/bin` no PATH).

## Rodar sem instalar

    ./arcadia-native

O `boot.mp4` é procurado ao lado do executável (e em `resources/boot.mp4`);
sem ele o app abre direto na interface.

## Verificar o download

    sha256sum -c SHA256SUMS

## Limitações

- Não há instalador nem auto-update neste tarball; é um pacote portátil.
- Windows tem pacote portátil próprio em `.zip`; veja
  `scripts/package-native-windows.ps1` e `docs/NATIVE-PACKAGING.md`.
- O binário exige a glibc do host que compilou. Os artefatos oficiais saem do
  CI em Ubuntu 24.04 (glibc 2.39); distribuições mais antigas podem recusar.
EOF
} > "$STAGE/README.md"

# --- Tarball determinístico + checksum --------------------------------------

TARBALL="$OUTPUT/$PKG.tar.gz"
echo "==> gerando $TARBALL"
rm -f "$TARBALL"
# --sort/--mtime/--owner fixos e `gzip -n` (sem nome nem timestamp) tornam o
# arquivo reproduzível para o mesmo conteúdo, independente do relógio do build.
tar --sort=name \
    --mtime='UTC 2020-01-01' \
    --owner=0 --group=0 --numeric-owner \
    --mode='u+rwX,go+rX,go-w' \
    --format=gnu \
    -C "$OUTPUT" -cf - "$PKG" | gzip -9 -n > "$TARBALL"

( cd "$OUTPUT" && sha256sum "$(basename "$TARBALL")" > SHA256SUMS )

echo
echo "==> artefatos"
ls -lh "$TARBALL" "$OUTPUT/SHA256SUMS" | awk '{print "    "$5"\t"$9}'
echo "==> conteúdo do tarball"
tar -tzf "$TARBALL" | sed 's/^/    /'
