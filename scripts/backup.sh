#!/usr/bin/env bash
set -euo pipefail

DATA_DIR="${NABA_DATA_DIR:-$HOME/.nabaos}"
BACKUP_DIR="${1:-$(pwd)}"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
BACKUP_FILE="$BACKUP_DIR/nabaos-backup-$TIMESTAMP.tar.gz"

echo "NabaOS Backup"
echo "============="
echo "Data directory: $DATA_DIR"
echo "Backup file:    $BACKUP_FILE"

if [ ! -d "$DATA_DIR" ]; then
    echo "ERROR: Data directory not found: $DATA_DIR"
    exit 1
fi

FILES=()
for f in "$DATA_DIR"/*.db; do
    [ -f "$f" ] && FILES+=("$f")
done
[ -d "$DATA_DIR/config" ] && FILES+=("$DATA_DIR/config")
[ -f "$DATA_DIR/constitution.toml" ] && FILES+=("$DATA_DIR/constitution.toml")
[ -d "$DATA_DIR/models" ] && FILES+=("$DATA_DIR/models")

if [ ${#FILES[@]} -eq 0 ]; then
    echo "WARNING: No files found to backup"
    exit 1
fi

tar -czf "$BACKUP_FILE" "${FILES[@]}" 2>/dev/null
SIZE=$(du -h "$BACKUP_FILE" | cut -f1)
echo "Backup complete: $BACKUP_FILE ($SIZE)"
