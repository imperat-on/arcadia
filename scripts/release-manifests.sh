#!/usr/bin/env bash
# Gera latest.yml (Windows/NSIS) e latest-linux.yml (AppImage) para o
# auto-update do Arcadia Native.
#
# Uso: ./scripts/release-manifests.sh <versao> <AppImage> <setup.exe> <saida>
set -euo pipefail

[ $# -eq 4 ] || {
    echo "uso: $0 <versao> <AppImage> <setup.exe> <saida>" >&2
    exit 2
}

VERSION="$1"
LINUX="$2"
WINDOWS="$3"
OUTPUT="$4"

for file in "$LINUX" "$WINDOWS"; do
    [ -f "$file" ] || {
        echo "erro: arquivo ausente: $file" >&2
        exit 1
    }
done
mkdir -p "$OUTPUT"

sha512_base64() {
    openssl dgst -sha512 -binary "$1" | base64 -w0
}
bytes() {
    stat -c%s "$1"
}
release_date="$(date -u +%Y-%m-%dT%H:%M:%S.000Z)"
linux_name="$(basename "$LINUX")"
windows_name="$(basename "$WINDOWS")"
linux_sha="$(sha512_base64 "$LINUX")"
windows_sha="$(sha512_base64 "$WINDOWS")"

cat > "$OUTPUT/latest-linux.yml" <<EOF
version: $VERSION
files:
  - url: $linux_name
    sha512: $linux_sha
    size: $(bytes "$LINUX")
path: $linux_name
sha512: $linux_sha
releaseDate: '$release_date'
EOF

cat > "$OUTPUT/latest.yml" <<EOF
version: $VERSION
files:
  - url: $windows_name
    sha512: $windows_sha
    size: $(bytes "$WINDOWS")
path: $windows_name
sha512: $windows_sha
releaseDate: '$release_date'
EOF

echo "==> latest.yml e latest-linux.yml gerados em $OUTPUT"
