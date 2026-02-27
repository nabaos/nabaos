# Upgrading

> **What you'll learn**
>
> - How to upgrade NabaOS to a new version
> - How to pin a specific version
> - How to upgrade Docker deployments
> - What the breaking changes policy is
> - How to roll back if something goes wrong

---

## Upgrading a Native Install

The install script detects an existing installation and replaces the old binary. Run the same command you used to install:

```bash
curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh | bash
```

Expected output:

```
    _   __                         ___                    __     ____  _____
   / | / /_  ____  ___ ___  ____ _/   | ____ ____  ____  / /_   / __ \/ ___/
  /  |/ / / / / / / / __ `/ / / / / /| |/ __ `/ _ \/ __ \/ __/  / / / /\__ \
 / /|  / /_/ / /_/ / /_/ / /_/ / ___ / /_/ /  __/ / / / /_/   / /_/ /___/ /
/_/ |_/\__, /\__,_/\__,_/\__, /_/  |_\__, /\___/_/ /_/\__/    \____//____/
      /____/            /____/       /____/

         NabaOS  —  Installer

[info]  Install directory : /home/user/.local/bin
[info]  Data directory    : /home/user/.nabaos
[info]  Detected platform: linux / x86_64

[info]  Attempting pre-built binary install...
[info]  Resolving latest release from GitHub...
[ ok ]  Latest release: v1.3.0
[info]  Downloading nabaos-linux-x86_64.tar.gz ...
[ ok ]  Downloaded nabaos-linux-x86_64.tar.gz
[info]  Verifying SHA256 checksum...
[ ok ]  Checksum verified
[info]  Extracting to /home/user/.local/bin ...
[ ok ]  Installed nabaos to /home/user/.local/bin/nabaos

[ ok ]  Data directories ready
[ ok ]  Default constitution already exists — skipping download
```

The installer preserves all data in `~/.nabaos/`. Only the binary is replaced.

### Verify the new version

```bash
nabaos --version
```

Expected output:

```
nabaos 1.3.0
```

### Restart the service

If running under systemd:

```bash
sudo systemctl restart nabaos
```

---

## Version Pinning

To install a specific version instead of the latest, set the `NABA_VERSION` environment variable:

```bash
NABA_VERSION=v1.2.0 curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh | bash
```

Expected output:

```
[info]  Using requested version: v1.2.0
[info]  Downloading nabaos-linux-x86_64.tar.gz ...
[ ok ]  Downloaded nabaos-linux-x86_64.tar.gz
[ ok ]  Checksum verified
[ ok ]  Installed nabaos to /home/user/.local/bin/nabaos
```

This is useful for:

- Staying on a known-good version in production.
- Testing a specific release before rolling it out.
- Reproducing an issue on an older version.

---

## Upgrading Docker Deployments

### Pull the new image

```bash
docker pull ghcr.io/nabaos/nabaos:latest
```

Expected output:

```
latest: Pulling from nabaos/nabaos
a1b2c3d4: Already exists
e5f6a7b8: Pull complete
Digest: sha256:abc123...
Status: Downloaded newer image for ghcr.io/nabaos/nabaos:latest
```

### Recreate the container

```bash
docker compose down
docker compose up -d
```

Or with a single command:

```bash
docker compose up -d --force-recreate
```

### Pin a specific Docker tag

In your `docker-compose.yml`, replace `latest` with a version tag:

```yaml
services:
  nabaos:
    image: ghcr.io/nabaos/nabaos:v1.2.0
    # ...
```

Then:

```bash
docker compose pull
docker compose up -d
```

### Verify the running version

```bash
docker exec nabaos nabaos --version
```

Expected output:

```
nabaos 1.3.0
```

Your data is safe during upgrades because it lives in Docker volumes (`nyaya-data`, `nyaya-models`), which are independent of the container image.

---

## Breaking Changes Policy

NabaOS follows these rules for version changes:

| Version bump | What can change | Example |
|--------------|-----------------|---------|
| **Patch** (1.2.x) | Bug fixes only. No config changes, no CLI changes. | `v1.2.0` to `v1.2.1` |
| **Minor** (1.x.0) | New features, new CLI commands, new config options. Existing behavior does not break. | `v1.2.0` to `v1.3.0` |
| **Major** (x.0.0) | May change config format, CLI interface, or data directory layout. Migration guide provided. | `v1.3.0` to `v2.0.0` |

Before upgrading across a major version:

1. Read the release notes on the [GitHub releases page](https://github.com/nabaos/nabaos/releases).
2. Back up your data directory (see [Backup and Restore](./backup-restore.md)).
3. Follow the migration guide included in the release notes.

---

## Rollback

If a new version causes problems, you can revert to the previous version.

### Native install: keep the previous binary

Before upgrading, save a copy of the current binary:

```bash
cp ~/.local/bin/nabaos ~/.local/bin/nabaos.previous
```

To roll back:

```bash
# Stop the agent
sudo systemctl stop nabaos   # if using systemd

# Swap binaries
mv ~/.local/bin/nabaos ~/.local/bin/nabaos.broken
mv ~/.local/bin/nabaos.previous ~/.local/bin/nabaos

# Verify
nabaos --version

# Restart
sudo systemctl start nabaos
```

### Native install: reinstall a specific version

If you did not save the previous binary, use version pinning:

```bash
NABA_VERSION=v1.2.0 curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh | bash
sudo systemctl restart nabaos
```

### Docker: roll back to a previous tag

```bash
# Edit docker-compose.yml to use the previous version
#   image: ghcr.io/nabaos/nabaos:v1.2.0

docker compose pull
docker compose up -d --force-recreate
```

Or without editing the file:

```bash
docker compose down
docker run -d \
  --name nabaos \
  --restart unless-stopped \
  -e NABA_LLM_PROVIDER=anthropic \
  -e NABA_LLM_API_KEY="$NABA_LLM_API_KEY" \
  -v nyaya-data:/data \
  -v nyaya-models:/models \
  ghcr.io/nabaos/nabaos:v1.2.0
```

---

## Upgrade Checklist

1. **Back up** your data directory or Docker volumes (see [Backup and Restore](./backup-restore.md)).
2. **Read the release notes** for the target version.
3. **Save the current binary** (native) or note the current image tag (Docker).
4. **Upgrade** using the install script or `docker pull`.
5. **Restart** the service.
6. **Verify** with `nabaos --version`, `nabaos cache stats`, and `nabaos costs`.
7. **Test** by sending a query through your usual channel.
8. If something is wrong, **roll back** using the steps above.
