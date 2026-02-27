# systemd Service

> **What you'll learn**
>
> - How to create a systemd unit file for NabaOS
> - How to enable the service so it starts automatically on boot
> - How to view logs with `journalctl`
> - How to configure restart policies and environment files
> - How to run the agent as a dedicated system user

---

## Prerequisites

- NabaOS installed at `/usr/local/bin/nabaos` (or `~/.local/bin/nabaos`)
- A Linux system with systemd (Debian 12+, Ubuntu 22.04+, Fedora 38+, etc.)

If you installed via the install script, the binary is at `~/.local/bin/nabaos`. For a system-wide service, copy it to `/usr/local/bin/`:

```bash
sudo cp ~/.local/bin/nabaos /usr/local/bin/nabaos
```

Verify:

```bash
nabaos --version
```

Expected output:

```
nabaos 0.1.0
```

---

## Create a Dedicated User

Run the agent under its own unprivileged user for security isolation:

```bash
sudo useradd -r -s /usr/sbin/nologin -m -d /var/lib/nabaos nyaya
```

Create the data directories:

```bash
sudo mkdir -p /var/lib/nabaos/{agents,plugins,catalog,models,config/constitutions,logs}
sudo chown -R nyaya:nyaya /var/lib/nabaos
```

---

## Environment File

Create the environment file that the service will read:

```bash
sudo mkdir -p /etc/nabaos
sudo tee /etc/nabaos/env > /dev/null << 'EOF'
# LLM provider and API key (required)
NABA_LLM_PROVIDER=anthropic
NABA_LLM_API_KEY=sk-ant-api03-xxxxx

# Telegram bot token (optional)
NABA_TELEGRAM_BOT_TOKEN=123456:ABC-DEF

# Data and model paths
NABA_DATA_DIR=/var/lib/nabaos
NABA_MODEL_PATH=/var/lib/nabaos/models

# Cost control
NABA_DAILY_BUDGET_USD=10.0

# Logging
NABA_LOG_LEVEL=info

# Security alerts (optional)
# NABA_SECURITY_BOT_TOKEN=...
# NABA_ALERT_CHAT_ID=...
EOF
```

Lock down the file permissions (it contains API keys):

```bash
sudo chmod 600 /etc/nabaos/env
sudo chown nyaya:nyaya /etc/nabaos/env
```

---

## systemd Unit File

Create the service file:

```bash
sudo tee /etc/systemd/system/nabaos.service > /dev/null << 'EOF'
[Unit]
Description=NabaOS - Security-first AI agent runtime
Documentation=https://nabaos.github.io/nabaos/
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=nyaya
Group=nyaya

# Environment
EnvironmentFile=/etc/nabaos/env

# Working directory
WorkingDirectory=/var/lib/nabaos

# Start the daemon
ExecStart=/usr/local/bin/nabaos daemon

# Restart policy: always restart with a 5-second delay
Restart=always
RestartSec=5

# Stop gracefully, then force-kill after 30 seconds
TimeoutStopSec=30

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/nabaos
PrivateTmp=yes

# Logging goes to journald
StandardOutput=journal
StandardError=journal
SyslogIdentifier=nabaos

[Install]
WantedBy=multi-user.target
EOF
```

---

## Install and Start the Service

Reload systemd to pick up the new unit file, then enable and start it:

```bash
# Reload systemd daemon
sudo systemctl daemon-reload

# Enable the service to start on boot
sudo systemctl enable nabaos

# Start the service now
sudo systemctl start nabaos
```

Expected output from `enable`:

```
Created symlink /etc/systemd/system/multi-user.target.wants/nabaos.service
  → /etc/systemd/system/nabaos.service.
```

Check the service status:

```bash
sudo systemctl status nabaos
```

Expected output:

```
● nabaos.service - NabaOS - Security-first AI agent runtime
     Loaded: loaded (/etc/systemd/system/nabaos.service; enabled; preset: enabled)
     Active: active (running) since Mon 2026-02-24 10:00:01 UTC; 5s ago
       Docs: https://nabaos.github.io/nabaos/
   Main PID: 12345 (nabaos)
      Tasks: 8 (limit: 4915)
     Memory: 45.2M
        CPU: 320ms
     CGroup: /system.slice/nabaos.service
             └─12345 /usr/local/bin/nabaos daemon

Feb 24 10:00:01 server nabaos[12345]: INFO  NabaOS starting...
Feb 24 10:00:01 server nabaos[12345]: INFO  Loading configuration from /var/lib/nabaos/config
Feb 24 10:00:02 server nabaos[12345]: INFO  Security layer initialized
Feb 24 10:00:02 server nabaos[12345]: INFO  Daemon listening
```

---

## Viewing Logs

The agent logs to journald via `tracing-subscriber`. Use `journalctl` to view them.

### Follow logs in real time

```bash
sudo journalctl -u nabaos -f
```

Expected output:

```
Feb 24 10:00:02 server nabaos[12345]: INFO  Daemon listening
Feb 24 10:05:11 server nabaos[12345]: INFO  Cache hit: check_email (fingerprint match)
Feb 24 10:05:11 server nabaos[12345]: INFO  Request completed in 12ms
```

### View logs since boot

```bash
sudo journalctl -u nabaos -b
```

### View logs from a specific time range

```bash
sudo journalctl -u nabaos --since "2026-02-24 09:00" --until "2026-02-24 12:00"
```

### Show only errors

```bash
sudo journalctl -u nabaos -p err
```

---

## Restart Policy

The unit file uses `Restart=always` with `RestartSec=5`. This means:

- If the process exits for **any reason** (crash, OOM, signal), systemd waits 5 seconds and starts it again.
- This includes exits with code 0 (normal exit). If you want to exclude clean exits, use `Restart=on-failure` instead.

To restart the service manually:

```bash
sudo systemctl restart nabaos
```

To stop the service:

```bash
sudo systemctl stop nabaos
```

To disable the service from starting on boot:

```bash
sudo systemctl disable nabaos
```

---

## Common Operations

### Reload environment changes

If you edit `/etc/nabaos/env`, restart the service to pick up the changes:

```bash
sudo systemctl restart nabaos
```

### Check if the agent is using expected resources

```bash
sudo systemctl show nabaos --property=MemoryCurrent,CPUUsageNSec
```

Expected output:

```
MemoryCurrent=47316992
CPUUsageNSec=1250000000
```

### View the full unit file

```bash
systemctl cat nabaos
```

---

## Running the Web Dashboard as a Separate Service

If you also want to run the web dashboard under systemd, create a second unit file:

```bash
sudo tee /etc/systemd/system/nyaya-web.service > /dev/null << 'EOF'
[Unit]
Description=NabaOS - Web Dashboard
After=nabaos.service
Requires=nabaos.service

[Service]
Type=simple
User=nyaya
Group=nyaya
EnvironmentFile=/etc/nabaos/env
WorkingDirectory=/var/lib/nabaos
ExecStart=/usr/local/bin/nabaos web --bind 127.0.0.1:3000
Restart=always
RestartSec=5
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/nabaos
PrivateTmp=yes
StandardOutput=journal
StandardError=journal
SyslogIdentifier=nyaya-web

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now nyaya-web
```

The dashboard will be available at `http://127.0.0.1:3000`. Place a reverse proxy (nginx, Caddy) in front of it for HTTPS.
