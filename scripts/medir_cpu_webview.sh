#!/usr/bin/env bash
# Compara o custo de CPU do WebKitWebProcess com e sem o GL sink desligado.
#
# A variavel WEBKIT_GST_DISABLE_GL_SINK existe para corrigir o frame verde do
# trailer no driver NVIDIA proprietario, mas mexe no compositing do webview
# inteiro. Este script mede quanto isso custa em CPU parada (sem interacao).
set -e

BIN="${1:-/home/zes/Documents/Projects/arcadia-rust/target/release/arcadia}"
SEGUNDOS="${2:-8}"

medir() {
    local rotulo="$1"
    shift
    local env_extra=("$@")

    env "${env_extra[@]}" "$BIN" >/dev/null 2>&1 &
    local pid=$!
    # Espera a janela montar e o WebKit subir.
    sleep 6

    local total=0 amostras=0
    for _ in $(seq 1 "$SEGUNDOS"); do
        # Soma os dois campos de CPU de todos os WebKitWebProcess filhos.
        local soma=0
        while read -r web; do
            [ -z "$web" ] && continue
            local t
            t=$(awk '{print $14+$15}' "/proc/$web/stat" 2>/dev/null || echo 0)
            soma=$((soma + t))
        done < <(pgrep -P "$pid" -f WebKitWebProcess 2>/dev/null; pgrep -f WebKitWebProcess 2>/dev/null | head -2)
        total=$((total + soma))
        amostras=$((amostras + 1))
        sleep 1
    done

    local rss
    rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
    echo "  $rotulo: $((total / amostras)) ticks/s   RSS=$((rss / 1024))MB"

    kill -TERM "$pid" 2>/dev/null || true
    pkill -f 'WebKitWebProcess' 2>/dev/null || true
    sleep 2
}

echo "==> medindo (${SEGUNDOS}s por cenario)"
echo
medir "COM GL sink desligado (atual)" WEBKIT_GST_DISABLE_GL_SINK=1
medir "SEM a variavel (padrao)"        ARCADIA_KEEP_GL_SINK=1
echo
echo "Menos ticks/s = menos CPU. Compare os dois numeros."
