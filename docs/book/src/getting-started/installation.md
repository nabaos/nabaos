# Installation

> **What you'll learn**
>
> - System requirements for running NabaOS
> - Four ways to install: one-line script, Cargo, Docker, or from source
> - How to verify your installation

## System Requirements

| Requirement | Minimum |
|-------------|---------|
| OS | 64-bit Linux (x86_64) or macOS (Apple Silicon) |
| RAM | 512 MB free (the ONNX models use ~80 MB at runtime) |
| Disk | 200 MB (binary + models + SQLite databases) |
| Network | Outbound HTTPS for LLM API calls (not needed for cached requests) |

> **Note:** Only two release targets are currently provided: `linux-amd64` and `darwin-arm64`. Other platforms must build from source.

Optional:
- **Docker** -- required only if you want container-isolated task execution or the Docker install method.
- **A Telegram/Discord/Slack bot token** -- only if you want a messaging channel. The CLI and web dashboard work without one.

---

## Method 1: One-Line Installer (Recommended)

The install script detects your OS and architecture, downloads the correct
pre-built binary, and places it in `~/.local/bin`.

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)
```

**What it does:**

1. Detects your platform (`linux-amd64`, `darwin-arm64`).
2. Downloads the latest release binary from GitHub Releases.
3. Downloads the default constitution template.
4. Installs to `~/.local/bin/nabaos`.
5. Adds `~/.local/bin` to your `PATH` (appends to `~/.bashrc` or `~/.zshrc`).

**Expected output:**

```text
Detecting platform... linux-amd64
Downloading nabaos v0.1.0...
  [####################################] 100%
Downloading default constitution...
  [####################################] 100%
Installing to ~/.local/bin/nabaos
Adding ~/.local/bin to PATH in ~/.bashrc

Installation complete! Run:
  source ~/.bashrc
  nabaos --version
```

After the script finishes, open a new terminal or run `source ~/.bashrc`
(or `source ~/.zshrc`) to pick up the new PATH entry.

---

## Method 2: Cargo Install

If you already have a Rust toolchain (1.80+):

```bash
cargo install --git https://github.com/nabaos/nabaos.git
```

This compiles from source and places the `nabaos` binary in
`~/.cargo/bin/`. The ONNX model files are downloaded automatically on first run
via `nabaos setup`.

To include the BERT classifier (Tier 1), enable the `bert` feature gate:

```bash
cargo install --git https://github.com/nabaos/nabaos.git --features bert
```

> **Note:** The `bert` feature is optional. Without it, Tiers 1-2 degrade
> gracefully to `unknown_unknown` classification and queries fall through to
> the LLM tiers.

**Expected output:**

```text
  Compiling nabaos v0.1.0
    ...
  Installing ~/.cargo/bin/nabaos
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
  -v nabaos-data:/data \
  -p 8919:8919 \
  ghcr.io/nabaos/nabaos:latest \
  start
```

This starts the server, which runs the scheduler loop and (if configured) the
Telegram bot and web dashboard. The web dashboard is available at
`http://localhost:8919`.

To run CLI commands against the container:

```bash
docker exec nabaos nabaos admin classify "check my email"
docker exec nabaos nabaos admin cache stats
```

---

## Method 4: Build from Source

```bash
git clone https://github.com/nabaos/nabaos.git
cd nabaos
cargo build --release
```

To include the BERT classifier:

```bash
cargo build --release --features bert
```

The binary is at `target/release/nabaos`. Copy it to a directory on your PATH
or run it directly:

```bash
./target/release/nabaos --version
```

---

## Verify Your Installation

Regardless of which method you used, verify that NabaOS is working:

```bash
nabaos --version
```

**Expected output:**

```text
nabaos 0.1.0
```

If you see a version number, the installation succeeded. Next, run the setup
wizard to configure your LLM provider and constitution.

---

## Troubleshooting

### `command not found: nabaos`

The binary is not on your PATH. Either:

- Open a new terminal (the installer may have updated your shell profile).
- Run `source ~/.bashrc` or `source ~/.zshrc`.
- Manually add the install directory to PATH:
  ```bash
  export PATH="$HOME/.local/bin:$PATH"
  ```

### `error: Model directory not found`

The ONNX model files have not been downloaded yet. Run:

```bash
nabaos setup
```

The setup wizard will download the required models.

### Permission denied on Linux

The binary may not have the execute bit set. Fix it:

```bash
chmod +x ~/.local/bin/nabaos
```

### macOS Gatekeeper blocks the binary

On macOS, the first run may trigger a "developer cannot be verified" warning.
Allow it in System Settings > Privacy & Security, or run:

```bash
xattr -d com.apple.quarantine ~/.local/bin/nabaos
```

### Docker: port 8919 already in use

Another service is using port 8919. Either stop that service or map to a
different port:

```bash
docker run -d -p 9090:8919 ... ghcr.io/nabaos/nabaos:latest start
```

---

## Next Step

Proceed to [First Run](first-run.md) to walk through the setup wizard and
send your first query.
