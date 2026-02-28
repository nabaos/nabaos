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
bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)
```

The installer preserves all data in `~/.nabaos/`. Only the binary is replaced.

### Verify the new version

```bash
nabaos --version
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
NABA_VERSION=v0.1.1 bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)
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
    image: ghcr.io/nabaos/nabaos:v0.1.1
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

Your data is safe during upgrades because it lives in Docker volumes (`nabaos-data`, `nabaos-models`), which are independent of the container image.

---

## Breaking Changes Policy

NabaOS follows these rules for version changes:

| Version bump | What can change | Example |
|--------------|-----------------|---------|
| **Patch** (0.1.x) | Bug fixes only. No config changes, no CLI changes. | `v0.1.0` to `v0.1.1` |
| **Minor** (0.x.0) | New features, new CLI commands, new config options. Existing behavior does not break. | `v0.1.0` to `v0.2.0` |
| **Major** (x.0.0) | May change config format, CLI interface, or data directory layout. Migration guide provided. | `v0.2.0` to `v1.0.0` |

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
NABA_VERSION=v0.1.0 bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)
sudo systemctl restart nabaos
```

### Docker: roll back to a previous tag

```bash
docker compose down
docker run -d \
  --name nabaos \
  --restart unless-stopped \
  -e NABA_LLM_PROVIDER=anthropic \
  -e NABA_LLM_API_KEY="$NABA_LLM_API_KEY" \
  -v nabaos-data:/data \
  -v nabaos-models:/models \
  ghcr.io/nabaos/nabaos:v0.1.0
```

---

## Upgrade Checklist

1. **Back up** your data directory or Docker volumes (see [Backup and Restore](./backup-restore.md)).
2. **Read the release notes** for the target version.
3. **Save the current binary** (native) or note the current image tag (Docker).
4. **Upgrade** using the install script or `docker pull`.
5. **Restart** the service.
6. **Verify** with `nabaos --version`, `nabaos admin cache stats`, and `nabaos status`.
7. **Test** by sending a query through your usual channel.
8. If something is wrong, **roll back** using the steps above.
