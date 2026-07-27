#!/bin/bash
# fix_downloader.sh — worker do Arcadia pra baixar+extrair fixes.
# Port direto do downloader.sh do luatools-moon (mesmo formato de state file,
# mesmos manifests .slssteam_fix_dlls/launchers). Sem o Steam runtime no PATH.
#
# Args: <URL> <DEST_PATH> <EXTRACT_DIR> <STATE_FILE> [<USER_AGENT>] [<HEADER_FILE>]
# Env:  SEVENZ (caminho 7zz/7z opcional), EXTRACT_NESTED=1 (default), FIX_TYPE.

set -u
unset LD_LIBRARY_PATH LD_PRELOAD LD_AUDIT STEAM_RUNTIME_LIBRARY_PATH STEAM_ZENITY

URL="$1"
DEST_PATH="$2"
EXTRACT_DIR="$3"
STATE_FILE="$4"
USER_AGENT="${5:-arcadia}"
HEADER_FILE="${6:-}"

CONNECT_TIMEOUT="${CONNECT_TIMEOUT:-8}"
MAX_TIME="${MAX_TIME:-0}"
SPEED_LIMIT="${SPEED_LIMIT:-1024}"
SPEED_TIME="${SPEED_TIME:-45}"
SEVENZ="${SEVENZ:-}"

write_state() {
  [ -n "$STATE_FILE" ] || return 0
  printf '{"status": "%s", "bytesRead": %s, "totalBytes": %s}\n' "$1" "${2:-0}" "${3:-0}" > "$STATE_FILE"
}
json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
write_failed() {
  local reason="$1" code="${2:-}"
  if [ -n "$code" ]; then
    printf '{"status": "failed", "error": "%s", "errorCode": "%s"}\n' "$(json_escape "$reason")" "$(json_escape "$code")" > "$STATE_FILE"
  else
    printf '{"status": "failed", "error": "%s"}\n' "$(json_escape "$reason")" > "$STATE_FILE"
  fi
}

write_state "downloading" 0 0

CURL_HEADERS=()
if [ -n "$HEADER_FILE" ] && [ -r "$HEADER_FILE" ]; then
  CURL_HEADERS=(--header "@$HEADER_FILE")
fi

TOTAL="$(curl -sIL -A "$USER_AGENT" --connect-timeout "$CONNECT_TIMEOUT" --max-time 6 \
  "${CURL_HEADERS[@]}" "$URL" 2>/dev/null | tr -d '\r' \
  | awk -F': ' 'tolower($1)=="content-length"{v=$2} END{print v+0}')"
[ -z "$TOTAL" ] && TOTAL=0

PART="${DEST_PATH}.part.$$"
HTTP_CODE_PATH="${PART}.http"
curl --fail -L -A "$USER_AGENT" \
  --connect-timeout "$CONNECT_TIMEOUT" \
  ${MAX_TIME:+--max-time "$MAX_TIME"} \
  --speed-limit "$SPEED_LIMIT" --speed-time "$SPEED_TIME" \
  --write-out '%{http_code}' "${CURL_HEADERS[@]}" -o "$PART" "$URL" \
  > "$HTTP_CODE_PATH" &
CURL_PID=$!

while kill -0 "$CURL_PID" 2>/dev/null; do
  if [ -f "$PART" ]; then
    sz="$(stat -c %s "$PART" 2>/dev/null || echo 0)"
    write_state "downloading" "$sz" "$TOTAL"
  fi
  sleep 0.3
done
wait "$CURL_PID"
rc=$?
HTTP_CODE="$(cat "$HTTP_CODE_PATH" 2>/dev/null || true)"
rm -f "$HTTP_CODE_PATH"

if [ "$rc" -ne 0 ]; then
  rm -f "$PART"
  if [ "$rc" -eq 3 ]; then
    write_failed "The download link from this source is not valid." "badurl"
  elif [ "$rc" -eq 28 ]; then
    write_failed "Download stalled. Try again, or pick another source." "stalled"
  elif [ "$rc" -eq 22 ] && { [ "$HTTP_CODE" = "401" ] || [ "$HTTP_CODE" = "403" ]; }; then
    write_failed "Ryuu authentication was rejected or expired." "authentication"
  else
    write_failed "Download failed — try another source."
  fi
  exit 1
fi
mv -f "$PART" "$DEST_PATH" || { write_failed "Could not save the file."; exit 1; }

write_state "extracting" "$TOTAL" "$TOTAL"

if [ -z "$EXTRACT_DIR" ]; then
  write_state "done" "$TOTAL" "$TOTAL"
  exit 0
fi

mkdir -p "$EXTRACT_DIR"

# Extrai: 7zz se existe (cobre .rar), senão unzip.
if [ -n "$SEVENZ" ] && [ -x "$SEVENZ" ]; then
  "$SEVENZ" x -bd -y -o"$EXTRACT_DIR" "$DEST_PATH" >/dev/null 2>&1
  rc=$?
else
  unzip -o -q "$DEST_PATH" -d "$EXTRACT_DIR" 2>/dev/null
  rc=$?
fi
if [ $rc -ne 0 ]; then
  write_failed "The archive could not be opened."
  exit 1
fi

# Manifests: DLLs e launchers do arquivo (antes de qualquer extração nested).
MANIFEST="$EXTRACT_DIR/.slssteam_fix_dlls"
LAUNCHER_MANIFEST="$EXTRACT_DIR/.slssteam_fix_launchers"
DLL_ACC="$(mktemp)"
LAUNCHER_ACC="$(mktemp)"

list_from_archive() {  # $1 = archive
  if [ -n "$SEVENZ" ] && [ -x "$SEVENZ" ]; then
    "$SEVENZ" l -ba -slt "$1" 2>/dev/null | sed -n 's/^Path = //p'
  else
    unzip -l "$1" 2>/dev/null | awk 'NR>3 && $NF!=""{print $NF}' | head -n -2
  fi
}

list_from_archive "$DEST_PATH" | grep -iE '\.dll$' | sed 's#.*[/\\]##' >> "$DLL_ACC"
list_from_archive "$DEST_PATH" | tr '\\' '/' | grep -iE '(^|/)(launcher\.exe|launcher_[^/]+\.exe|[^/]+_launcher\.exe)$' >> "$LAUNCHER_ACC"

# Extração nested (rar/zip dentro do zip).
if [ "${EXTRACT_NESTED:-1}" = "1" ]; then
  found=0
  while IFS= read -r -d '' arc; do
    found=1
    b="$(basename "$arc")"
    # Pula volumes secundários (partN.rar N>1, .rNN, .zNN).
    skip=0
    case "$b" in
      *.part0*2.rar|*.part0*3.rar|*.part0*4.rar|*.part0*5.rar|*.part0*6.rar|*.part0*7.rar|*.part0*8.rar|*.part0*9.rar) skip=1 ;;
      *.r[0-9]|*.r[0-9][0-9]|*.z[0-9]|*.z[0-9][0-9]) skip=1 ;;
    esac
    if [ "$skip" = "0" ]; then
      list_from_archive "$arc" | grep -iE '\.dll$' | sed 's#.*[/\\]##' >> "$DLL_ACC"
      list_from_archive "$arc" | tr '\\' '/' | grep -iE '(^|/)(launcher\.exe|launcher_[^/]+\.exe|[^/]+_launcher\.exe)$' >> "$LAUNCHER_ACC"
      if [ -n "$SEVENZ" ] && [ -x "$SEVENZ" ]; then
        "$SEVENZ" x -bd -y -o"$EXTRACT_DIR" "$arc" >/dev/null 2>&1 || true
      fi
    fi
  done < <(find "$EXTRACT_DIR" -type f \( -iname '*.rar' -o -iname '*.zip' -o -iname '*.7z' -o -iname '*.r[0-9][0-9]' -o -iname '*.z[0-9][0-9]' \) -print0 2>/dev/null)

  if [ "$found" = "1" ]; then
    find "$EXTRACT_DIR" -type f \( -iname '*.rar' -o -iname '*.zip' -o -iname '*.7z' -o -iname '*.r[0-9][0-9]' -o -iname '*.z[0-9][0-9]' \) -delete 2>/dev/null || true
  fi
fi

if [ -s "$DLL_ACC" ]; then
  sort -u -f "$DLL_ACC" > "$MANIFEST" 2>/dev/null || true
fi
if [ -s "$LAUNCHER_ACC" ]; then
  sort -u -f "$LAUNCHER_ACC" > "$LAUNCHER_MANIFEST" 2>/dev/null || true
fi
rm -f "$DLL_ACC" "$LAUNCHER_ACC"

write_state "extracted" "$TOTAL" "$TOTAL"
