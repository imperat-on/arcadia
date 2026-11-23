#!/usr/bin/env bash
# Preflight dos workers do frontend nativo.
#
# Problema que isto resolve: três workers compilando iced/wgpu ao mesmo tempo
# saturam a máquina (load > 40) e parecem travados no terminal.
#
# Este wrapper:
#   - limita os jobs de compilação (padrão 4);
#   - aplica um timeout (padrão 15 min) para nenhum comando ficar pendurado.
#
# IMPORTANTE: cada worktree usa o PRÓPRIO `target/`. Compartilhar o target entre
# checkouts já fez o Cargo reutilizar um binário de teste de outro worker e o
# teste mentir. O registry/git cache do Cargo (~/.cargo) continua compartilhado,
# que é seguro; o target não.
#
# Uso:
#   scripts/native-worker-preflight.sh cargo check -p arcadia-native
#   scripts/native-worker-preflight.sh cargo test -p arcadia-native
#   ARCADIA_CMD_TIMEOUT=300 scripts/native-worker-preflight.sh cargo test ...
set -euo pipefail

# Opt-in explícito, por conta de quem chama. Não derive do worktree principal.
if [ -n "${ARCADIA_TARGET_DIR:-}" ]; then
    export CARGO_TARGET_DIR="$ARCADIA_TARGET_DIR"
fi

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-1}"

LIMITE="${ARCADIA_CMD_TIMEOUT:-900}"
exec timeout --foreground "$LIMITE" "$@"
