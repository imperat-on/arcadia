#!/usr/bin/env bash
# Verificação de regressão do pacote Linux do Arcadia.
#
# Roda depois de scripts/package-linux.sh --verify, ou sozinho contra os
# artefatos já gerados. Cada checagem falha com mensagem explícita — o ponto é
# descobrir um pacote quebrado ANTES do usuário, não depois.
#
# CHECAGENS
#   1. artefatos existem e têm tamanho plausível
#   2. .deb declara as dependências de runtime (webkit + gstreamer + codecs)
#   3. AppImage contém os plugins de codec (openh264, hls) e a libopenh264
#   4. os plugins carregam DENTRO do AppImage, sem usar o GStreamer do host
#   5. o pipeline H.264 do trailer decodifica com os plugins do pacote
#   6. o binário do pacote tem o QT/tema e não tem RPATH apontando pro host
#
# Uso:
#   ./scripts/verify-package.sh            # tudo
#   ./scripts/verify-package.sh --quick    # pula o decode (mais rápido)
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="$ROOT/target/release/bundle"
QUICK=0
[ "${1:-}" = "--quick" ] && QUICK=1

pass=0
fail=0
declare -a failures=()

ok()   { printf '  \033[32mok\033[0m    %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFALHA\033[0m %s\n' "$1"; fail=$((fail+1)); failures+=("$1"); }
skip() { printf '  \033[33mskip\033[0m  %s\n' "$1"; }

echo "==> verificação do pacote Linux"

# --- 1. artefatos -----------------------------------------------------------

DEB="$(ls -t "$BUNDLE"/deb/*.deb 2>/dev/null | head -1)"
APPIMG="$(ls -t "$BUNDLE"/appimage/*.AppImage 2>/dev/null | head -1)"
RPM="$(ls -t "$BUNDLE"/rpm/*.rpm 2>/dev/null | head -1)"

if [ -n "$DEB" ] && [ -f "$DEB" ]; then
    size=$(stat -c%s "$DEB")
    if [ "$size" -gt 5000000 ]; then
        ok ".deb existe ($(numfmt --to=iec "$size"))"
    else
        bad ".deb suspeito de vazio (${size}B)"
    fi
else
    bad ".deb não encontrado em $BUNDLE/deb"
fi

if [ -n "$APPIMG" ] && [ -f "$APPIMG" ]; then
    size=$(stat -c%s "$APPIMG")
    # O limite é baixo de propósito: depois de `trim_appdir_graphics` o
    # AppImage leva só o binário e os codecs (~39 MB). Um arquivo abaixo disso
    # perdeu o binário ou o squashfs está truncado.
    if [ "$size" -lt 15000000 ]; then
        bad "AppImage suspeito de incompleto (${size}B)"
    else
        ok "AppImage existe ($(numfmt --to=iec "$size"))"
    fi

    # A pilha gráfica NÃO deve estar no pacote: ela vence o sistema via
    # LD_LIBRARY_PATH do runtime e entrega o WebKit do host de build.
    # O próprio AppImage extrai sem precisar de squashfs-tools.
    appdir_tmp="$(mktemp -d)"
    if ( cd "$appdir_tmp" && "$APPIMG" --appimage-extract >/dev/null 2>&1 ); then
        if [ -e "$appdir_tmp/squashfs-root/usr/lib/libwebkit2gtk-4.1.so.0" ]; then
            bad "AppImage embarca WebKitGTK do host de build (vence o sistema do usuário)"
        else
            ok "AppImage não embarca a pilha gráfica (usa a do sistema)"
        fi
    else
        skip "não foi possível extrair o AppImage para inspecionar"
    fi
    rm -rf "$appdir_tmp"
else
    bad "AppImage não encontrado em $BUNDLE/appimage"
fi

# --- 2. dependências do .deb ------------------------------------------------

if [ -n "$DEB" ] && [ -f "$DEB" ]; then
    # O Tauri emite control.tar.gz; aceita .zst e .xz por segurança.
    # Extrai para arquivo temporário: o tarball tem bytes nulos e a
    # command substitution os descarta, corrompendo o parse.
    ctrl=""
    tmpd="$(mktemp -d)"
    for member in control.tar.zst control.tar.gz control.tar.xz; do
        if ar t "$DEB" 2>/dev/null | grep -qx "$member"; then
            ar p "$DEB" "$member" > "$tmpd/c.tar" 2>/dev/null || continue
            case "$member" in
                *.zst) tar --zstd -xf "$tmpd/c.tar" -C "$tmpd" 2>/dev/null ;;
                *.gz)  tar -xzf "$tmpd/c.tar" -C "$tmpd" 2>/dev/null ;;
                *.xz)  tar -xJf "$tmpd/c.tar" -C "$tmpd" 2>/dev/null ;;
            esac
            [ -f "$tmpd/control" ] && { ctrl="$(cat "$tmpd/control")"; break; }
        fi
    done
    rm -rf "$tmpd"
    depends="$(printf '%s\n' "$ctrl" | grep -i '^Depends:' || true)"

    if printf '%s' "$depends" | grep -q 'libwebkit2gtk-4.1-0'; then
        ok ".deb depende de libwebkit2gtk-4.1-0"
    else
        bad ".deb NÃO declara libwebkit2gtk-4.1-0 (app não abre sem webkit)"
    fi

    if printf '%s' "$depends" | grep -qE 'gst-plugins-(bad|good|base)'; then
        ok ".deb declara plugins GStreamer base"
    else
        bad ".deb não declara gst-plugins-base/good/bad"
    fi

    # H.264 é obrigatório para trailer. openh264 OU libav satisfazem.
    if printf '%s' "$depends" | grep -qE 'gst-libav|gst-plugins-bad'; then
        ok ".deb cobre decoder H.264 (gst-plugins-bad ou gst-libav)"
    else
        bad ".deb não cobre H.264 — trailer ficaria preto"
    fi
fi

# --- 2b. dependências do .rpm -----------------------------------------------
#
# O RPM é um arquivo binário; sem rpm2cpio não há como abrir o header. Busca as
# strings de dependência no cabeçalho — suficiente para pegar a regressão que
# importa: RPM sem GStreamer = trailer preto no Fedora.

if [ -n "$RPM" ] && [ -f "$RPM" ]; then
    size=$(stat -c%s "$RPM")
    if [ "$size" -gt 5000000 ]; then
        ok ".rpm existe ($(numfmt --to=iec "$size"))"
    else
        bad ".rpm suspeito de vazio (${size}B)"
    fi

    # Strings de dependência ficam nos primeiros KB do header. Vai para arquivo
    # temporário em vez de variável: command substitution come bytes nulos do
    # header e corrompe a comparação.
    rpm_strings="$(mktemp)"
    head -c 65536 "$RPM" | tr '\0' '\n' > "$rpm_strings"
    for needed in webkit2gtk gtk3 gstreamer1-plugins-base; do
        if grep -q "$needed" "$rpm_strings"; then
            ok ".rpm depende de $needed"
        else
            bad ".rpm NÃO declara $needed"
        fi
    done
    if grep -qE 'gstreamer1-(plugins-bad|libav)' "$rpm_strings"; then
        ok ".rpm cobre decoder H.264"
    else
        bad ".rpm não cobre H.264 — trailer ficaria preto no Fedora"
    fi
    rm -f "$rpm_strings"
fi

# --- 3/4/5. AppImage: extrai e verifica os codecs ---------------------------

if [ -n "$APPIMG" ] && [ -f "$APPIMG" ]; then
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    if ! (cd "$work" && "$APPIMG" --appimage-extract >/dev/null 2>&1); then
        bad "falha ao extrair o AppImage"
    else
        sq="$work/squashfs-root"
        gst="$sq/usr/lib/gstreamer-1.0"

        for p in libgstopenh264.so libgsthls.so; do
            if [ -e "$gst/$p" ]; then
                ok "AppImage embarca $p"
            else
                bad "AppImage SEM $p — trailer quebra"
            fi
        done

        if [ -e "$sq/usr/lib/libopenh264.so.8" ]; then
            ok "AppImage embarca libopenh264.so.8"
        else
            # O `.so` versionado pode não existir num build feito em distro que
            # entrega só o SONAME sem o arquivo real. O que decide se o trailer
            # toca é o plugin carregar — testado logo abaixo.
            skip "libopenh264.so.8 ausente; o plugin será validado pelo teste de carga"
        fi

        # Carrega os plugins num ambiente limpo: se resolver pelo host, o teste
        # não prova nada sobre o pacote.
        if command -v gst-inspect-1.0 >/dev/null 2>&1; then
            probe="$(env -i HOME="$work" PATH=/usr/bin:/bin \
                     LD_LIBRARY_PATH="$sq/usr/lib" \
                     GST_PLUGIN_PATH="$gst" \
                     GST_PLUGIN_SYSTEM_PATH_1_0="$gst" \
                     gst-inspect-1.0 openh264dec 2>&1)"
            if printf '%s' "$probe" | grep -q "$sq"; then
                ok "openh264dec carrega de dentro do pacote"
            else
                bad "openh264dec não carrega do pacote (resolveu fora?)"
            fi

            if [ $QUICK -eq 0 ]; then
                sample=""
                for cand in "$ROOT/boot.mp4" "$sq/usr/lib/arcadia/resources/boot.mp4"; do
                    [ -f "$cand" ] && { sample="$cand"; break; }
                done
                if [ -n "$sample" ]; then
                    if env -i HOME="$work" PATH=/usr/bin:/bin \
                       LD_LIBRARY_PATH="$sq/usr/lib" \
                       GST_PLUGIN_PATH="$gst" \
                       GST_PLUGIN_SYSTEM_PATH_1_0="$gst" \
                       timeout 60 gst-launch-1.0 -q filesrc location="$sample" ! qtdemux ! h264parse ! openh264dec ! videoconvert ! fakesink >/dev/null 2>&1; then
                        ok "pipeline H.264 decodifica com os codecs do pacote"
                    else
                        bad "pipeline H.264 FALHA dentro do pacote"
                    fi
                else
                    skip "sem amostra de vídeo para o teste de decode"
                fi
            else
                skip "decode H.264 (--quick)"
            fi
        else
            skip "gst-inspect-1.0 fora do host; sem prova de carregamento"
        fi

        # RPATH não deve apontar para caminhos do host.
        bin="$sq/usr/bin/arcadia"
        if [ -x "$bin" ] && command -v readelf >/dev/null 2>&1; then
            rpath="$(readelf -d "$bin" 2>/dev/null | grep -iE 'rpath|runpath' || true)"
            if printf '%s' "$rpath" | grep -qE '/home/|/root/'; then
                bad "binário tem RPATH absoluto do host: $rpath"
            else
                ok "binário sem RPATH apontando para o host"
            fi
        fi

        # Aviso, não falha: a glibc exigida depende do host do build. Quem
        # empacota no Arch (glibc 2.44) gera um AppImage que só abre em distro
        # recente. O distribuível sai do CI, em ubuntu-22.04.
        if [ -x "$bin" ] && command -v objdump >/dev/null 2>&1; then
            glibc_max="$(objdump -T "$bin" 2>/dev/null \
                        | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/GLIBC_//' \
                        | sort -uV | tail -1)"
            if [ -n "$glibc_max" ]; then
                if [ "$(printf '%s\n2.35\n' "$glibc_max" | sort -V | tail -1)" = "2.35" ]; then
                    ok "glibc exigida: $glibc_max (roda em Ubuntu 22.04+)"
                else
                    skip "glibc exigida: $glibc_max — acima de 2.35, NÃO distribuível (empacote no CI/ubuntu-22.04)"
                fi
            fi
        fi
    fi
fi

# --- relatório --------------------------------------------------------------

echo
if [ $fail -eq 0 ]; then
    printf '\033[32m%s checagens, 0 falhas — pacote íntegro\033[0m\n' "$pass"
    exit 0
else
    printf '\033[31m%s ok, %s falha(s)\033[0m\n' "$pass" "$fail"
    for f in "${failures[@]}"; do printf '  - %s\n' "$f"; done
    exit 1
fi
