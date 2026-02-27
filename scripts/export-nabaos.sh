#!/usr/bin/env bash
set -euo pipefail

SRC_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DEST_DIR="$HOME/nabaos"

echo "NabaOS Export"
echo "============="
echo "Source: $SRC_DIR"
echo "Dest:   $DEST_DIR"

if [ -d "$DEST_DIR" ]; then
    echo "ERROR: $DEST_DIR already exists. Remove it first."
    exit 1
fi

mkdir -p "$DEST_DIR"

# --- Copy source tree (excluding plans, dev docs, sibling projects, build artifacts) ---
rsync -a --exclude='.git' \
    --exclude='docs/plans/' \
    --exclude='status_25022026_comparison.md' \
    --exclude='NABA_AGENT_OS_UPDATED.md' \
    --exclude='CLAUDE.md' \
    --exclude='target/' \
    --exclude='.claude/' \
    --exclude='node_modules/' \
    "$SRC_DIR/" "$DEST_DIR/"

# --- Rename nyaya-web to nabaos-web ---
if [ -d "$DEST_DIR/nyaya-web" ]; then
    mv "$DEST_DIR/nyaya-web" "$DEST_DIR/nabaos-web"
fi

# --- User-facing string replacements ---
# Binary/package name in Cargo.toml
find "$DEST_DIR" -name 'Cargo.toml' -exec sed -i 's/name = "nabaos"/name = "nabaos"/g' {} +

# Environment variable prefix NABA_ -> NABA_
find "$DEST_DIR" \( -name '*.rs' -o -name '*.sh' -o -name '*.toml' -o -name '*.yml' -o -name '*.yaml' -o -name '*.md' -o -name 'Dockerfile' -o -name 'Makefile' -o -name '*.service' -o -name '*.svelte' -o -name '*.ts' -o -name '*.js' -o -name '*.env*' \) \
    -exec sed -i 's/NABA_/NABA_/g' {} +

# Brand name NabaOS -> NabaOS in display text
find "$DEST_DIR" \( -name '*.rs' -o -name '*.md' -o -name '*.sh' -o -name '*.toml' -o -name '*.yml' -o -name '*.svelte' -o -name '*.ts' \) \
    -exec sed -i 's/NabaOS/NabaOS/g' {} +

# Package/binary references nabaos -> nabaos
find "$DEST_DIR" \( -name '*.rs' -o -name '*.md' -o -name '*.sh' -o -name '*.toml' -o -name '*.yml' -o -name 'Dockerfile' -o -name 'Makefile' -o -name '*.service' \) \
    -exec sed -i 's/nabaos/nabaos/g' {} +

# Rust crate name: nyaya_agent -> nabaos (underscore variant used in `use` statements)
find "$DEST_DIR" -name '*.rs' -exec sed -i 's/nyaya_agent/nabaos/g' {} +

# Default data directory
find "$DEST_DIR" \( -name '*.rs' -o -name '*.sh' \) -exec sed -i 's/\.nabaos/.nabaos/g' {} +

# Docker volume names
if [ -f "$DEST_DIR/docker-compose.yml" ]; then
    sed -i 's/nyaya-data/nabaos-data/g' "$DEST_DIR/docker-compose.yml"
    sed -i 's/nyaya-models/nabaos-models/g' "$DEST_DIR/docker-compose.yml"
fi

# Dockerfile paths and user
if [ -f "$DEST_DIR/Dockerfile" ]; then
    sed -i 's|/etc/nyaya/|/etc/nabaos/|g' "$DEST_DIR/Dockerfile"
    sed -i 's/nyaya:nyaya/nabaos:nabaos/g' "$DEST_DIR/Dockerfile"
fi

# Makefile
if [ -f "$DEST_DIR/Makefile" ]; then
    sed -i 's/nabaos/nabaos/g' "$DEST_DIR/Makefile"
fi

# --- Remove competitor mentions from Rust comments ---
find "$DEST_DIR/src" -name '*.rs' -exec sed -i '/\/\/.*[Oo]pen[Cc]law/d' {} +
find "$DEST_DIR/src" -name '*.rs' -exec sed -i '/\/\/.*[Ss]teve [Jj]obs/d' {} +

# --- Clean up files that shouldn't ship ---
rm -rf "$DEST_DIR/docs/plans" 2>/dev/null || true
rm -f "$DEST_DIR/status_25022026_comparison.md" 2>/dev/null || true
rm -f "$DEST_DIR/NABA_AGENT_OS_UPDATED.md" 2>/dev/null || true
rm -f "$DEST_DIR/CLAUDE.md" 2>/dev/null || true

echo ""
echo "Export complete. Files in $DEST_DIR"
echo ""
echo "Next steps:"
echo "  cd $DEST_DIR"
echo "  cargo build --release"
echo "  git init && git config user.name 'Abhinaba Basu' && git config user.email 'mail@abhinaba.com'"
echo "  git add -A && git commit -m 'NabaOS v0.1.0 — personal agent runtime'"
