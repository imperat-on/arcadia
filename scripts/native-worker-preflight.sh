#!/usr/bin/env bash
# Preflight dos workers do frontend nativo.
#
# Problema que isto resolve: cada worktree do Orca tinha o próprio `target/`,
# então três workers compilando iced/wgpu do zero ao mesmo tempo saturam a
# máquina (load > 40) e parecem travados no terminal.
#
# Este wrapper:
#   - compartilha o `target/` do worktree principal entre os workers;
#   - limita os jobs de compilação (padrão 4);
#   - aplica um timeout (padrão 15 min) para nenhum comando ficar pendurado.
#
# Uso:
#   scripts/native-worker-preflight.sh cargo check -p arcadia-native
#   scripts/native-worker-preflight.sh cargo test -p arcadia-native
#   ARCADIA_CMD_TIMEOUT=300 scripts/native-worker-preflight.sh cargo test ...
set -euo pipefail

RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
COMMON="$(git -C "$RAIZ" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
if [ -n "$COMMON" ]; then
    BASE="$(dirname "$COMMON")"
else
    BASE="$RAIZ"
fi

export CARGO_TARGET_DIR="${ARCADIA_TARGET_DIR:-$BASE/target}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-1}"

LIMITE="${ARCADIA_CMD_TIMEOUT:-900}"
exec timeout --foreground "$LIMITE" "$@"
