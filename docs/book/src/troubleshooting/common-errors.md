# Common Errors

> **What you'll learn**
>
> - Every `NabaError` variant, what causes it, and how to fix it
> - Specific error messages with exact fix commands
> - Where to go for more help when the fix does not work

---

## Error Reference

NabaOS uses the `NabaError` enum for all error types. Each variant is
listed below with its common messages, causes, and fixes.

---

### `Config` -- Configuration Error

Configuration errors occur when required environment variables are missing, the
config file is malformed, or a required resource is not specified.

#### "NABA_LLM_API_KEY not set"

**Symptom:** NabaOS fails to start or rejects queries that require LLM routing
(Tier 3/4).

**Cause:** The LLM provider API key is not in the environment.

**Fix:**

```bash
# For Anthropic (default)
export NABA_LLM_PROVIDER=anthropic
export NABA_LLM_API_KEY=sk-ant-api03-your-key-here

# For OpenAI
export NABA_LLM_PROVIDER=openai
export NABA_LLM_API_KEY=sk-your-key-here

# Persist across sessions
echo 'export NABA_LLM_API_KEY=sk-ant-api03-your-key-here' >> ~/.bashrc
source ~/.bashrc
```

Or re-run the setup wizard, which will prompt for the key:

```bash
nabaos setup
```

**Docs:** [First Run > Step 2](../getting-started/first-run.md#step-2-set-your-llm-provider)

---

#### "NABA_TELEGRAM_BOT_TOKEN not set" / "TELEGRAM" related errors

**Symptom:** The service starts but reports that Telegram is disabled, or
Telegram messages are not received.

**Cause:** The Telegram bot token is not set, or the token is invalid.

**Fix:**

```bash
export NABA_TELEGRAM_BOT_TOKEN=1234567890:ABCDefghIJKLmnopQRSTuvwxYZ123456789

# For the security alert bot (separate token)
export NABA_SECURITY_BOT_TOKEN=0987654321:ZYXwvuTSRQponMLKJIhgfeDCBA987654321
export NABA_ALERT_CHAT_ID=your-chat-id
```

**Docs:** [Telegram Setup](../guides/telegram-setup.md)

---

#### "Invalid constitution" / "Constitution file not found"

**Symptom:** NabaOS refuses to start because the constitution file is missing or
has a syntax error.

**Cause:** The YAML constitution file is malformed, missing required fields, or
the path in the configuration does not point to a valid file.

**Fix:**

```bash
# Check constitution syntax
nabaos config rules check

# View the active constitution
nabaos config rules show

# Reset to a default constitution template
nabaos config rules use-template default
```

**Docs:** [Constitution Customization](../guides/constitution-customization.md)

---

### `ModelLoad` -- Model Loading Error

#### "Model directory not found" / "ONNX model file not found"

**Symptom:** Classification commands (`nabaos admin classify`) fail with a model
loading error on first run.

**Cause:** The ONNX model files have not been downloaded. They are not bundled
with the binary to keep the download small.

**Fix:**

```bash
# Download models via setup
nabaos setup

# Or specify a custom model path
export NABA_MODEL_PATH=/path/to/your/models
```

> **Note:** If the binary was built without the `bert` feature gate, Tiers 1-2
> are disabled and classification degrades to `unknown_unknown`. This is not an
> error -- it means the BERT and SetFit models are simply not available.

---

#### "Model format not supported" / "ONNX runtime error"

**Symptom:** The model files exist but fail to load.

**Cause:** The ONNX model files were downloaded for a different architecture,
or the ONNX Runtime version is incompatible.

**Fix:**

```bash
# Delete and re-download
rm -rf ~/.nabaos/models/
nabaos setup

# Verify the model files
ls -la ~/.nabaos/models/
# Expected: setfit-w5h2.onnx, tokenizer.json, config.json
```

---

### `Inference` -- Inference Error

#### "Inference failed" / "Model output shape mismatch"

**Symptom:** Classification runs but returns an error instead of a result.

**Cause:** The model file is corrupted, truncated during download, or was built
for a different version of the classifier.

**Fix:**

```bash
# Re-download models (force)
rm -rf ~/.nabaos/models/
nabaos setup

# Verify with a test classification
nabaos admin classify "test query"
```

---

### `Cache` -- Cache Error

#### "Cache database corrupted" / "SQLite error: database disk image is malformed"

**Symptom:** Queries that should hit the cache return errors. The
`admin cache stats` command fails.

**Cause:** The SQLite database file for the fingerprint or intent cache was
corrupted, typically by a crash during a write operation or disk full condition.

**Fix:**

```bash
# Check cache health
nabaos admin cache stats

# If corrupted, delete the database and let it rebuild
rm ~/.nabaos/cache.db

# The cache will repopulate as queries come in.
# Tier 0 (fingerprint) rebuilds on first repeat query.
# Tier 2 (intent) rebuilds as classifications accumulate.
```

---

#### "Cache full" / "Maximum cache entries exceeded"

**Symptom:** New cache entries are not being stored.

**Cause:** The cache has reached its configured maximum size.

**Fix:**

```bash
# View cache stats
nabaos admin cache stats

# Delete the cache database and let it rebuild from scratch
rm ~/.nabaos/cache.db
```

---

### `Vault` -- Vault Error

#### "Vault passphrase incorrect" / "Decryption failed"

**Symptom:** The agent cannot access stored secrets (API keys, tokens).

**Cause:** The vault passphrase does not match the one used when the vault was
created, or the encrypted vault file is corrupted.

**Fix:**

```bash
# Re-store secrets with the correct passphrase
nabaos config vault store NABA_LLM_API_KEY

# If you forgot the passphrase, delete the vault and re-create
rm ~/.nabaos/vault.enc
nabaos config vault store NABA_LLM_API_KEY
```

---

#### "Vault file not found"

**Symptom:** The agent reports a missing vault file on first run.

**Cause:** The vault has not been initialized yet. It is created automatically
when you store the first secret.

**Fix:**

```bash
nabaos config vault store NABA_LLM_API_KEY
```

---

### `ConstitutionViolation` -- Constitution Violation

#### "Query blocked by constitution rule: [rule name]"

**Symptom:** A query is rejected with a constitution violation message. The
query is not processed and no LLM call is made.

**Cause:** The query matched a `block` enforcement rule in the active
constitution. This is working as designed.

**Fix (if the block is incorrect):**

```bash
# View the active constitution rules
nabaos config rules show

# Check which rule matched
nabaos config rules check "your query here"
```

Common reasons for unexpected blocks:

- **Keyword trigger too broad:** A rule like `trigger_keywords: ["private"]`
  will block any query containing the word "private," even "private equity."
  Use more specific keywords or switch to action+target triggers.

- **Out-of-domain block:** The query is outside the agent's declared domain.
  Check `allowed_domains` in the constitution.

**Docs:** [Constitution Schema](../reference/constitution-schema.md),
[Constitution Customization](../guides/constitution-customization.md)

---

### `Database` -- Database Error (rusqlite)

#### "database is locked"

**Symptom:** Multiple operations fail with a "database is locked" error.

**Cause:** Another process (or another instance of NabaOS) has a write lock on
the SQLite database. SQLite allows only one writer at a time.

**Fix:**

```bash
# Check for other NabaOS processes
ps aux | grep nabaos

# Stop duplicate instances
sudo systemctl stop nabaos   # if using systemd

# If a process crashed and left a lock file
rm ~/.nabaos/*.db-wal ~/.nabaos/*.db-shm

# Restart
nabaos start
```

---

#### "unable to open database file"

**Symptom:** NabaOS cannot create or open its SQLite databases.

**Cause:** The data directory does not exist, or the user does not have write
permissions.

**Fix:**

```bash
# Check the data directory
ls -la ~/.nabaos/

# Create it if missing
mkdir -p ~/.nabaos

# Fix permissions
chmod 700 ~/.nabaos

# Or use a custom data directory
export NABA_DATA_DIR=/path/with/write/access
```

---

### `Io` -- I/O Error

#### "Permission denied" (file system)

**Symptom:** NabaOS cannot read config files, write to the data directory, or
access model files.

**Cause:** The NabaOS process does not have the required file system permissions.

**Fix:**

```bash
# Check file ownership
ls -la ~/.nabaos/

# Fix ownership (replace 'youruser' with your username)
chown -R youruser:youruser ~/.nabaos/

# Fix permissions
chmod -R u+rw ~/.nabaos/
```

---

#### "No space left on device"

**Symptom:** Any write operation (cache, logs, database) fails.

**Cause:** The disk partition is full.

**Fix:**

```bash
# Check disk space
df -h ~/.nabaos/

# Clean up old logs
rm ~/.nabaos/logs/*.log.old

# Move the data directory to a larger partition
export NABA_DATA_DIR=/mnt/larger-disk/nabaos
```

---

### `Json` -- JSON Parse Error

#### "expected value at line N column N"

**Symptom:** A configuration file or API response cannot be parsed as JSON.

**Cause:** The JSON file has a syntax error (missing comma, trailing comma,
unquoted key, etc.), or an API returned unexpected non-JSON content.

**Fix:**

```bash
# Validate JSON syntax
python3 -m json.tool < ~/.nabaos/config.json

# Or use jq
jq . < ~/.nabaos/config.json

# If the error is from an API response, enable debug logging to see
# the raw response:
RUST_LOG=debug nabaos ask "test"
```

---

### `Yaml` -- YAML Parse Error

#### "did not find expected key" / "mapping values are not allowed here"

**Symptom:** A YAML configuration file (constitution, manifest, chain) fails
to parse.

**Cause:** YAML indentation error, missing colon, or a value that needs quoting.

**Fix:**

```bash
# Validate YAML syntax
python3 -c "import yaml; yaml.safe_load(open('constitution.yaml'))"

# Common issues:
# - Tabs instead of spaces (YAML requires spaces)
# - Missing space after colon (key:value → key: value)
# - Unquoted strings with special characters (use quotes: "value: with colon")

# Re-generate from template if stuck
nabaos config rules use-template default
```

---

### `Wasm` -- WASM Runtime Error

#### "Wasm module failed to load" / "fuel exhausted"

**Symptom:** A cached work module or agent plugin fails to execute.

**Cause:** The WASM module is incompatible with the current wasmtime runtime
version, corrupted, or exceeded its fuel (execution step) budget.

**Fix:**

```bash
# Delete the cache database to clear cached WASM modules
rm ~/.nabaos/cache.db

# The next identical query will regenerate the module from scratch.

# If fuel exhaustion is the issue, the module may contain an infinite loop.
# Check the chain definition for unbounded recursion.
```

---

### `PermissionDenied` -- Permission Denied

#### "Agent 'X' does not have permission 'Y'"

**Symptom:** An agent's chain step fails because it tried to call an ability
not listed in its manifest.

**Cause:** The agent's manifest does not declare the required permission, or
the constitution blocks the permission.

**Fix:**

```bash
# Check what permissions the agent has
nabaos config agent permissions <agent-name>

# Add the missing permission to the agent's manifest:
# permissions:
#   - existing.permission
#   - missing.permission     # <-- add this

# Re-package and re-install the agent
nabaos config agent package ~/my-agents/<agent-name> --output agent.nap
nabaos config agent install agent.nap
```

---

#### "Constitution denies permission 'Y' for agent 'X'"

**Symptom:** The permission is declared in the manifest but still denied.

**Cause:** The constitution's boundaries section blocks this permission even
when declared.

**Fix:**

```bash
# Check constitution boundaries
nabaos config rules show

# Look for:
# boundaries:
#   approved_tools: ["tool.a", "tool.b"]
#
# If your tool is not in approved_tools, it will be denied.
```

**Docs:** [Constitution Schema](../reference/constitution-schema.md)

---

## Quick Reference

| Error variant | Common cause | Quick fix |
|---|---|---|
| `Config` | Missing env var | `export NABA_LLM_API_KEY=...` |
| `ModelLoad` | Models not downloaded | `nabaos setup` |
| `Inference` | Corrupt model file | Delete `~/.nabaos/models/` and re-run `nabaos setup` |
| `Cache` | Corrupt SQLite | `rm ~/.nabaos/cache.db` |
| `Vault` | Wrong passphrase | Delete `~/.nabaos/vault.enc` and re-store secrets |
| `ConstitutionViolation` | Rule too broad | `nabaos config rules show` to inspect rules |
| `Database` | SQLite locked | Stop duplicate processes, remove WAL files |
| `Io` | File permissions | `chmod -R u+rw ~/.nabaos/` |
| `Json` | Syntax error | Validate with `python3 -m json.tool` |
| `Yaml` | Indentation error | Check for tabs vs spaces |
| `Wasm` | Module incompatible | `rm ~/.nabaos/cache.db` |
| `PermissionDenied` | Missing manifest permission | Add to `permissions` in manifest |

---

## Still Stuck?

If none of the fixes above resolve your issue:

1. **Enable debug logging** to get detailed output:

   ```bash
   RUST_LOG=debug nabaos ask "test"
   ```

   See [Debug Mode](debug-mode.md) for how to read the output.

2. **Search existing issues** on GitHub:

   ```bash
   gh issue list --repo nabaos/nabaos --search "your error message"
   ```

3. **Open a new issue** with the [bug report template](https://github.com/nabaos/nabaos/issues/new?template=bug_report.md). Include:
   - The full error message
   - Your OS and architecture (`uname -a`)
   - NabaOS version (`nabaos --version`)
   - Steps to reproduce
   - Debug log output (with secrets redacted)
