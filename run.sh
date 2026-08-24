#!/usr/bin/env bash
# Compatibilidade para instalações antigas: o frontend oficial atual é Electron.
# Use arcadia.sh diretamente para os modos console/desktop/gamescope.
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$DIR/arcadia.sh" "$@"
