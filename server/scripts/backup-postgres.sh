#!/bin/sh
set -eu

umask 077

SERVER_DIR=${SERVER_DIR:-/home/zes/.local/share/arcadia/server}
BACKUP_DIR=${BACKUP_DIR:-/home/zes/backups/arcadia}
STAMP=$(date -u +%Y%m%d-%H%M%S)

. "$SERVER_DIR/.env"
: "${DATABASE_URL:?DATABASE_URL nao configurada}"

mkdir -p "$BACKUP_DIR"

DB_TMP="$BACKUP_DIR/arcadia-postgres-$STAMP.dump.tmp"
DB_FILE="$BACKUP_DIR/arcadia-postgres-$STAMP.dump"
FILES_TMP="$BACKUP_DIR/arcadia-storage-$STAMP.tar.gz.tmp"
FILES_FILE="$BACKUP_DIR/arcadia-storage-$STAMP.tar.gz"

cleanup() {
  rm -f "$DB_TMP" "$FILES_TMP"
}
trap cleanup EXIT INT TERM

pg_dump --dbname="$DATABASE_URL" --format=custom --compress=6 --file="$DB_TMP"
pg_restore --list "$DB_TMP" >/dev/null
mv "$DB_TMP" "$DB_FILE"

tar -C "$SERVER_DIR" -czf "$FILES_TMP" avatars backgrounds banners
mv "$FILES_TMP" "$FILES_FILE"

sha256sum "$DB_FILE" "$FILES_FILE" > "$BACKUP_DIR/arcadia-$STAMP.sha256"

find "$BACKUP_DIR" -type f -name 'arcadia-*' -mtime +14 -delete
trap - EXIT INT TERM
