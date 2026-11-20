#!/usr/bin/env bash
# Verificação do pacote Windows gerado no Linux.
#
# Não executa o .exe (não há Windows aqui). Prova o que é provável sem uma
# máquina Windows: formato PE, arquitetura, dependências de DLL, e — o mais
# importante — que o instalador embarca o binário, o boot.mp4 e o
# WebView2Loader.dll, sem os quais o app abre em branco ou nem abre.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="x86_64-pc-windows-gnu"
OUT="$ROOT/target/$TARGET/release"

pass=0; fail=0
declare -a failures=()
ok()   { printf '  \033[32mok\033[0m    %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFALHA\033[0m %s\n' "$1"; fail=$((fail+1)); failures+=("$1"); }
skip() { printf '  \033[33mskip\033[0m  %s\n' "$1"; }

echo "==> verificação do pacote Windows"

EXE="$OUT/arcadia.exe"
SETUP="$(ls -t "$OUT"/bundle/nsis/*.exe 2>/dev/null | head -1)"

# --- 1. binário -------------------------------------------------------------

if [ -f "$EXE" ]; then
    ok "arcadia.exe existe ($(numfmt --to=iec "$(stat -c%s "$EXE")"))"
    info="$(file -b "$EXE")"
    if printf '%s' "$info" | grep -q 'PE32+'; then
        ok "arcadia.exe é PE32+ (x86-64)"
    else
        bad "arcadia.exe não é PE32+: $info"
    fi
    if printf '%s' "$info" | grep -q 'x86-64'; then
        ok "arquitetura x86-64 confirmada"
    else
        bad "arquitetura não é x86-64: $info"
    fi
else
    bad "arcadia.exe não encontrado em $OUT"
fi

# --- 2. dependências de DLL -------------------------------------------------
#
# Só DLLs do próprio Windows são aceitáveis, com UMA exceção: WebView2Loader.dll,
# que o Tauri embarca junto. Qualquer outra significa runtime ausente na máquina
# do usuário.

if [ -f "$EXE" ] && command -v x86_64-w64-mingw32-objdump >/dev/null 2>&1; then
    dlls="$(x86_64-w64-mingw32-objdump -p "$EXE" 2>/dev/null \
            | awk '/DLL Name/{print tolower($3)}' | sort -u)"
    externas="$(printf '%s\n' "$dlls" | grep -vE \
        '^(advapi32|bcrypt|bcryptprimitives|combase|comctl32|crypt32|dwmapi|gdi32|imm32|kernel32|msvcrt|ntdll|ole32|oleaut32|propsys|rpcrt4|shell32|shlwapi|user32|userenv|ws2_32|api-ms-win-.*|webview2loader)\.dll$' \
        | grep -v '^$' || true)"

    if [ -z "$externas" ]; then
        ok "apenas DLLs de sistema + WebView2Loader"
    else
        bad "depende de DLL externa não embarcada: $(echo $externas)"
    fi

    if printf '%s\n' "$dlls" | grep -qx 'webview2loader.dll'; then
        if [ -f "$OUT/WebView2Loader.dll" ]; then
            ok "WebView2Loader.dll presente ao lado do exe"
        else
            bad "exe exige WebView2Loader.dll mas ela não está em $OUT"
        fi
    fi
else
    skip "objdump cruzado indisponível; dependências de DLL não conferidas"
fi

# --- 3. instalador ----------------------------------------------------------

if [ -n "$SETUP" ] && [ -f "$SETUP" ]; then
    ok "setup.exe existe ($(numfmt --to=iec "$(stat -c%s "$SETUP")"))"
    if file -b "$SETUP" | grep -qi 'Nullsoft'; then
        ok "setup.exe é instalador NSIS"
    else
        bad "setup.exe não é um instalador NSIS válido"
    fi

    # O script NSIS gerado descreve exatamente o que entra no pacote — é a
    # fonte da verdade, já que o payload está comprimido em LZMA.
    nsi="$OUT/nsis/x64/installer.nsi"
    if [ -f "$nsi" ]; then
        # `boot.mp4` e o worker de torrent vivem sob `resources\` no instalador
        # Windows (o Tauri preserva o caminho de origem no NSIS), enquanto no
        # AppImage eles ficam achatados em `Arcadia/`. O verificador do Linux
        # cobre o segundo caso; aqui vale o caminho com prefixo.
        for item in 'MAINBINARYSRCPATH' 'resources\\boot.mp4' 'WebView2Loader.dll' 'torrent_rpc\\main.py'; do
            if grep -q "$item" "$nsi"; then
                ok "instalador inclui $(echo "$item" | tr -d '\\')"
            else
                bad "instalador NÃO inclui $item"
            fi
        done
    else
        skip "installer.nsi não encontrado; conteúdo não verificado"
    fi
else
    # O `.exe` sozinho já é artefato entregável. O instalador depende do
    # bundler do host, que aborta sem appindicator — aviso, não falha.
    skip "setup.exe não gerado (bundler do host sem appindicator); o .exe está íntegro"
fi

echo
if [ $fail -eq 0 ]; then
    printf '\033[32m%s checagens, 0 falhas — pacote Windows íntegro\033[0m\n' "$pass"
    exit 0
else
    printf '\033[31m%s ok, %s falha(s)\033[0m\n' "$pass" "$fail"
    for f in "${failures[@]}"; do printf '  - %s\n' "$f"; done
    exit 1
fi
