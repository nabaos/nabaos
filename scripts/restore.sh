#!/usr/bin/env bash
set -euo pipefail

BACKUP_FILE="${1:-}"
DATA_DIR="${NABA_DATA_DIR:-$HOME/.nabaos}"

if [ -z "$BACKUP_FILE" ]; then
    echo "Usage: restore.sh <backup-file.tar.gz> [data-dir]"
    exit 1
fi

if [ ! -f "$BACKUP_FILE" ]; then
    echo "ERROR: Backup file not found: $BACKUP_FILE"
    exit 1
fi

echo "NabaOS Restore"
echo "=============="
echo "Backup file:    $BACKUP_FILE"
echo "Data directory: $DATA_DIR"

mkdir -p "$DATA_DIR"
tar -xzf "$BACKUP_FILE" -C / 2>/dev/null || tar -xzf "$BACKUP_FILE" -C "$DATA_DIR" 2>/dev/null || {
    echo "ERROR: Failed to extract backup"
    exit 1
}

ERRORS=0
for db in "$DATA_DIR"/*.db; do
    [ -f "$db" ] || continue
    if sqlite3 "$db" "SELECT 1;" >/dev/null 2>&1; then
        echo "  OK: $(basename "$db")"
    else
        echo "  FAIL: $(basename "$db") — corrupted"
        ERRORS=$((ERRORS + 1))
    fi
done

if [ $ERRORS -eq 0 ]; then
    echo "Restore complete. All databases verified."
else
    echo "Restore complete with $ERRORS corrupted database(s)."
    exit 1
fi
