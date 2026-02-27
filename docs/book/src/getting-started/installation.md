# Installation

> **What you'll learn**
>
> - System requirements for running NabaOS
> - Four ways to install: one-line script, Cargo, Docker, or from source
> - How to verify your installation

## System Requirements

| Requirement | Minimum |
|-------------|---------|
| OS | 64-bit Linux (x86_64, aarch64) or macOS (Apple Silicon, Intel) |
| RAM | 512 MB free (the SetFit ONNX model uses ~80 MB at runtime) |
| Disk | 200 MB (binary + models + SQLite databases) |
| Network | Outbound HTTPS for LLM API calls (not needed for cached requests) |

Optional:
- **Docker** -- required only if you want container-isolated task execution or the Docker install method.
- **A Telegram/Discord/Slack bot token** -- only if you want a messaging channel. The CLI and web dashboard work without one.

---

## Method 1: One-Line Installer (Recommended)

The install script detects your OS and architecture, downloads the correct
pre-built binary, and places it in `~/.nabaos/bin`.

```bash
curl -fsSL https://get.nyaya.dev/install.sh | sh
```

**What it does:**

1. Detects your platform (`linux-x86_64`, `linux-aarch64`, `darwin-x86_64`, `darwin-aarch64`).
2. Downloads the latest release binary from GitHub Releases.
3. Downloads the SetFit ONNX model files (~25 MB).
4. Installs to `~/.nabaos/bin/nyaya`.
5. Adds `~/.nabaos/bin` to your `PATH` (appends to `~/.bashrc` or `~/.zshrc`).

**Expected output:**

```text
Detecting platform... linux-x86_64
Downloading nyaya v0.1.0...
  [####################################] 100%
Downloading SetFit ONNX model...
  [####################################] 100%
Installing to ~/.nabaos/bin/nyaya
Adding ~/.nabaos/bin to PATH in ~/.bashrc

Installation complete! Run:
  source ~/.bashrc
  nyaya --version
```

After the script finishes, open a new terminal or run `source ~/.bashrc`
(or `source ~/.zshrc`) to pick up the new PATH entry.

---

## Method 2: Cargo Install

If you already have a Rust toolchain (1.75+):

```bash
cargo install nabaos
```

This compiles from source on crates.io and places the `nyaya` binary in
`~/.cargo/bin/`. The ONNX model files are downloaded automatically on first run.

**Expected output:**

```text
  Compiling nabaos v0.1.0
    ...
  Installing ~/.cargo/bin/nyaya
   Installed package `nabaos v0.1.0`
```

---

## Method 3: Docker

Run NabaOS as a container with your LLM API key passed as an environment variable:

```bash
docker run -d \
  --name nabaos \
  -e NABA_LLM_PROVIDER=anthropic \
  -e NABA_LLM_API_KEY=sk-ant-your-key-here \
  -v nyaya-data:/data \
  -p 8919:8919 \
  ghcr.io/nabaos/nabaos:latest \
  daemon
```

This starts the daemon, which runs the scheduler loop and (if configured) the
Telegram bot and web dashboard. The web dashboard is available at
`http://localhost:8919`.

To run CLI commands against the container:

```bash
docker exec nabaos nyaya classify "check my email"
docker exec nabaos nyaya cache stats
```

---

## Method 4: Build from Source

```bash
git clone https://github.com/nabaos/nabaos.git
cd nabaos
cargo build --release
```

The binary is at `target/release/nyaya`. Copy it to a directory on your PATH
or run it directly:

```bash
./target/release/nyaya --version
```

To also download the ONNX model files:

```bash
./scripts/download-models.sh
```

---

## Verify Your Installation

Regardless of which method you used, verify that NabaOS is working:

```bash
nyaya --version
```

**Expected output:**

```text
nyaya 0.1.0
```

If you see a version number, the installation succeeded. Next, run the setup
wizard to configure your LLM provider and constitution.

---

## Troubleshooting

### `command not found: nyaya`

The binary is not on your PATH. Either:

- Open a new terminal (the installer may have updated your shell profile).
- Run `source ~/.bashrc` or `source ~/.zshrc`.
- Manually add the install directory to PATH:
  ```bash
  export PATH="$HOME/.nabaos/bin:$PATH"
  ```

### `error: Model directory not found`

The SetFit ONNX model files have not been downloaded yet. Run:

```bash
# If installed via the one-line installer or cargo install:
nyaya setup

# If building from source:
./scripts/download-models.sh
```

### Permission denied on Linux

The binary may not have the execute bit set. Fix it:

```bash
chmod +x ~/.nabaos/bin/nyaya
```

### macOS Gatekeeper blocks the binary

On macOS, the first run may trigger a "developer cannot be verified" warning.
Allow it in System Settings > Privacy & Security, or run:

```bash
xattr -d com.apple.quarantine ~/.nabaos/bin/nyaya
```

### Docker: port 8919 already in use

Another service is using port 8919. Either stop that service or map to a
different port:

```bash
docker run -d -p 9090:8919 ... ghcr.io/nabaos/nabaos:latest daemon
```

---

## Next Step

Proceed to [First Run](first-run.md) to walk through the setup wizard and
send your first query.
