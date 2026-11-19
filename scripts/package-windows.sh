#!/usr/bin/env bash
# Empacotamento Windows do Arcadia (shell Tauri/Rust), a partir do Linux.
#
# Gera `arcadia.exe` (portable) e `Arcadia-<versao>-x64-setup.exe` (NSIS).
#
# O QUE É NECESSÁRIO
# ------------------
#   rustup target add x86_64-pc-windows-gnu
#   mingw-w64-gcc, mingw-w64-binutils, mingw-w64-headers,
#   mingw-w64-crt, mingw-w64-winpthreads     (toolchain de link)
#   makensis                                  (compilador do instalador)
#
# Nenhum precisa de root: o script aceita tudo em ARCADIA_WIN_TOOLS (default
# ~/.local/share/arcadia-tools/win), com `bin/` no PATH contendo os binários.
#
# TRÊS PEGADINHAS QUE CUSTARAM CARO
#   1. `dlltool` (mingw-w64-binutils) é obrigatório: sem ele o `windows-sys`
#      não compila — o erro fala de dlltool, não de binutils.
#   2. `stddef.h` do mingw-w64-gcc é um wrapper `#include_next`: sem o pacote
#      mingw-w64-headers ele falha com "stddef.h: No such file or directory"
#      dentro do build do `ring`.
#   3. O `tauri-winres` procura `rc.exe` do Windows SDK e aborta. Basta apontar
#      RC_x86_64_pc_windows_gnu para o `x86_64-w64-mingw32-windres` do mingw.
#
# USO
#   ./scripts/package-windows.sh            # exe + setup
#   ./scripts/package-windows.sh --exe      # só o binário portátil
#   ./scripts/package-windows.sh --setup    # só o instalador
#   ./scripts/package-windows.sh --verify   # empacota e valida
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI="$ROOT/ui/node_modules/.bin/tauri"
TARGET="x86_64-pc-windows-gnu"
OUT="$ROOT/target/$TARGET/release"
TOOLS="${ARCADIA_WIN_TOOLS:-$HOME/.local/share/arcadia-tools/win}"

DO_EXE=0
DO_SETUP=0
DO_VERIFY=0
for arg in "$@"; do
    case "$arg" in
        --exe) DO_EXE=1 ;;
        --setup) DO_SETUP=1 ;;
        --verify) DO_VERIFY=1; DO_EXE=1; DO_SETUP=1 ;;
        -h|--help) sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "arg desconhecido: $arg" >&2; exit 2 ;;
    esac
done
if [ $DO_EXE -eq 0 ] && [ $DO_SETUP -eq 0 ]; then
    DO_EXE=1; DO_SETUP=1
fi

# --- Pré-requisitos ---------------------------------------------------------

export PATH="$HOME/.cargo/bin:$TOOLS/bin:$TOOLS/nsis/bin:$PATH"

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "erro: '$1' não encontrado." >&2
        echo "  esperado em $TOOLS/bin (ou no PATH)." >&2
        exit 1
    }
}

need cargo
need x86_64-w64-mingw32-gcc
need x86_64-w64-mingw32-ar
need x86_64-w64-mingw32-dlltool
need x86_64-w64-mingw32-windres
[ -x "$TAURI" ] || { echo "erro: Tauri CLI ausente em $TAURI" >&2; exit 1; }

if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    echo "==> instalando alvo $TARGET"
    rustup target add "$TARGET"
fi

# O build script do Tauri usa `cc` e `embed-resource`; nenhum dos dois encontra
# o compilador cruzado sozinho.
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
export RC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-windres
export RC=x86_64-w64-mingw32-windres

# --- Build ------------------------------------------------------------------

echo "==> compilando para $TARGET"
cd "$ROOT/src-tauri"
if [ $DO_SETUP -eq 1 ]; then
    need makensis
    # O `tauri build` do host checa a presença de appindicator/GTK antes de
    # empacotar, mesmo quando o alvo é Windows. Quando a checagem falha, o
    # bundler aborta com "Can't detect any appindicator library" DEPOIS de já
    # ter produzido o `.exe` — foi o que aconteceu no CI.
    #
    # Aqui o bundle é tratado como opcional: se o `.exe` já existe, o
    # instalador é uma etapa extra, não um requisito. Sem ele o artefato ainda
    # é entregável e o script avisa por quê.
    if ! "$TAURI" build --target "$TARGET" --bundles nsis; then
        if [ -f "$OUT/arcadia.exe" ]; then
            echo "aviso: o instalador NSIS não foi gerado (bundler abortou)." >&2
            echo "       O .exe já está pronto; instale libayatana-appindicator3-dev" >&2
            echo "       para o bundle funcionar em hosts onde a checagem falha." >&2
        else
            echo "erro: build falhou e o .exe não foi gerado" >&2
            exit 1
        fi
    fi
else
    # Só o executável: `tauri build` sem bundle ainda passa pelo build script.
    "$TAURI" build --target "$TARGET"
fi

[ -f "$OUT/arcadia.exe" ] || { echo "erro: arcadia.exe não foi gerado" >&2; exit 1; }

# --- Relatório --------------------------------------------------------------

echo
echo "==> artefatos"
ls -lh "$OUT/arcadia.exe" 2>/dev/null | awk '{print "    "$5"\t"$9}'
ls -lh "$OUT"/bundle/nsis/*.exe 2>/dev/null | awk '{print "    "$5"\t"$9}'

if [ $DO_VERIFY -eq 1 ]; then
    echo
    exec "$ROOT/scripts/verify-package-windows.sh"
fi
