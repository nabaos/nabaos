# Backup and Restore

> **What you'll learn**
>
> - What files and databases to back up
> - How to create a timestamped backup with a simple script
> - How to restore from a backup
> - How to back up Docker volumes

---

## What to Back Up

All NabaOS state lives under a single data directory. By default this is `~/.nabaos/` for native installs or `/data` inside Docker containers.

| Path | Contents | Critical? |
|------|----------|-----------|
| `agents/` | Agent definitions and configurations | Yes |
| `plugins/` | Installed plugin manifests and code | Yes |
| `catalog/` | Agent catalog entries | Yes |
| `config/constitutions/` | Constitution TOML files that define agent boundaries | Yes |
| `config/` | General configuration files | Yes |
| `models/` | ONNX model files (BERT, embeddings) | No -- can be re-downloaded |
| `logs/` | Application logs | No -- informational only |
| `nyaya.db` | SQLite database: cache entries, cost tracking, intent cache | Yes |
| `vault.db` | Encrypted secrets vault | Yes |

### Priority files

At minimum, always back up:

1. **`nyaya.db`** -- Contains the fingerprint cache, intent cache, cost tracking history, and behavioral profiles. Losing this means the agent loses its learned cache entries and starts cold.
2. **`vault.db`** -- Contains encrypted secrets. If you lose this without a backup, stored secrets are gone.
3. **`config/constitutions/`** -- Your constitution files define the agent's security boundaries. These are critical to recreate your setup.
4. **`agents/`** and **`plugins/`** -- Your agent and plugin configurations.

---

## Backup Script

Save this as `backup-nyaya.sh` and run it periodically (e.g., via cron):

```bash
#!/usr/bin/env bash
set -euo pipefail

# --- Configuration ---
DATA_DIR="${NABA_DATA_DIR:-$HOME/.nabaos}"
BACKUP_DIR="${NABA_BACKUP_DIR:-$HOME/nyaya-backups}"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP_FILE="${BACKUP_DIR}/nyaya-backup-${TIMESTAMP}.tar.gz"
MAX_BACKUPS=7  # Keep the last 7 backups

# --- Create backup directory ---
mkdir -p "$BACKUP_DIR"

# --- Create the backup ---
echo "Backing up ${DATA_DIR} ..."
tar -czf "$BACKUP_FILE" \
  -C "$(dirname "$DATA_DIR")" \
  "$(basename "$DATA_DIR")"

echo "Backup saved to: ${BACKUP_FILE}"
ls -lh "$BACKUP_FILE"

# --- Rotate old backups ---
# Keep only the last MAX_BACKUPS files
cd "$BACKUP_DIR"
ls -1t nyaya-backup-*.tar.gz 2>/dev/null | tail -n +$((MAX_BACKUPS + 1)) | xargs -r rm -f
echo "Backup rotation complete (keeping last ${MAX_BACKUPS})"
```

Make it executable and run it:

```bash
chmod +x backup-nyaya.sh
./backup-nyaya.sh
```

Expected output:

```
Backing up /home/user/.nabaos ...
Backup saved to: /home/user/nyaya-backups/nyaya-backup-20260224-100000.tar.gz
-rw-r--r-- 1 user user 2.3M Feb 24 10:00 /home/user/nyaya-backups/nyaya-backup-20260224-100000.tar.gz
Backup rotation complete (keeping last 7)
```

### Automate with cron

Run the backup daily at 2:00 AM:

```bash
crontab -e
```

Add this line:

```
0 2 * * * /home/user/backup-nyaya.sh >> /home/user/nyaya-backups/backup.log 2>&1
```

---

## Restore Process

### 1. Stop the agent

```bash
# systemd
sudo systemctl stop nabaos

# Docker
docker compose down

# or if running directly
pkill nabaos
```

### 2. Identify the backup to restore

```bash
ls -lt ~/nyaya-backups/
```

Expected output:

```
-rw-r--r-- 1 user user 2.3M Feb 24 10:00 nyaya-backup-20260224-100000.tar.gz
-rw-r--r-- 1 user user 2.1M Feb 23 10:00 nyaya-backup-20260223-100000.tar.gz
-rw-r--r-- 1 user user 2.0M Feb 22 10:00 nyaya-backup-20260222-100000.tar.gz
```

### 3. Restore the data directory

```bash
# Back up the current state first (in case you need it)
mv ~/.nabaos ~/.nabaos.old

# Extract the backup
tar -xzf ~/nyaya-backups/nyaya-backup-20260224-100000.tar.gz -C ~/
```

Verify the restored files:

```bash
ls ~/.nabaos/
```

Expected output:

```
agents  catalog  config  logs  models  nyaya.db  plugins  vault.db
```

### 4. Start the agent

```bash
# systemd
sudo systemctl start nabaos

# Docker
docker compose up -d

# or directly
nabaos daemon
```

### 5. Verify

```bash
nabaos cache stats
```

If cache entries appear, the restore was successful.

---

## Docker Volume Backup

When running in Docker, data lives in named volumes. Back them up with `docker run` and a temporary container:

### Create a backup

```bash
# Back up the data volume
docker run --rm \
  -v nabaos_nyaya-data:/source:ro \
  -v "$(pwd)":/backup \
  debian:bookworm-slim \
  tar -czf /backup/nyaya-data-$(date +%Y%m%d-%H%M%S).tar.gz -C /source .

# Back up the models volume (optional -- models can be re-downloaded)
docker run --rm \
  -v nabaos_nyaya-models:/source:ro \
  -v "$(pwd)":/backup \
  debian:bookworm-slim \
  tar -czf /backup/nyaya-models-$(date +%Y%m%d-%H%M%S).tar.gz -C /source .
```

Expected output:

```
# No output on success. Verify the file was created:
ls -lh nyaya-data-*.tar.gz
-rw-r--r-- 1 root root 2.3M Feb 24 10:00 nyaya-data-20260224-100000.tar.gz
```

### Restore a Docker volume

```bash
# Stop the containers first
docker compose down

# Remove the existing volume (this destroys current data!)
docker volume rm nabaos_nyaya-data

# Recreate the volume and restore the backup
docker volume create nabaos_nyaya-data
docker run --rm \
  -v nabaos_nyaya-data:/target \
  -v "$(pwd)":/backup:ro \
  debian:bookworm-slim \
  tar -xzf /backup/nyaya-data-20260224-100000.tar.gz -C /target

# Start the containers
docker compose up -d
```

---

## SQLite Database Notes

The agent uses SQLite databases (`nyaya.db`, `vault.db`). SQLite is safe to back up by copying the file **only if the agent is stopped** or if you use the backup script while the agent is running and SQLite WAL mode is enabled (which it is by default).

For a guaranteed consistent backup while the agent is running, you can use the SQLite `.backup` command:

```bash
sqlite3 ~/.nabaos/nyaya.db ".backup '/tmp/nyaya.db.bak'"
sqlite3 ~/.nabaos/vault.db ".backup '/tmp/vault.db.bak'"
```

This produces a consistent snapshot even while the databases are being written to.

---

## Disaster Recovery Checklist

1. Stop the agent.
2. Restore the data directory from the most recent backup.
3. If models are missing, they will be re-downloaded on first run (or restore from a models backup).
4. Verify environment variables are set (API keys, tokens).
5. Start the agent.
6. Run `nabaos cache stats` and `nabaos costs` to confirm data integrity.
7. Send a test query through your messaging channel (Telegram, web dashboard) to confirm end-to-end operation.
