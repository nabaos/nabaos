# Plugin Manifest

Plugins extend NabaOS's ability catalog with external capabilities. There are
three types of plugin abilities: native plugins (shared libraries),
subprocess abilities (external CLI tools), and cloud abilities (remote HTTP
endpoints).

Resolution order when an ability is invoked: built-in > plugin > subprocess >
cloud > error.

## Plugin Manifest Schema

Each plugin is a directory containing a `manifest.yaml`:

```yaml
# Identity
name: string              # Plugin name (required)
version: string           # Semantic version (required)
author: string            # Author name (optional)
license: string           # License identifier (optional)

# Trust level
trust_level: string       # LOCAL | COMMUNITY | VERIFIED | OFFICIAL [default: LOCAL]

# The ability this plugin provides
ability:
  name: string            # Fully qualified ability name, e.g. "files.read_psd" (required)
  description: string     # Human-readable description (required)
  permission_tier: u8     # Required permission tier 0-4 [default: 0]

# Input/output schemas
input:                    # Parameter definitions
  param_name:
    type: string          # Parameter type: string, int, bool, filepath, etc.
    default: value        # Default value (optional)
    required: bool        # Whether required (optional)
    auto: string          # Auto-generate pattern for output paths (optional)

output:                   # Output field descriptions
  field_name: string      # Type description

# Audit trail
receipt_fields:           # Fields to include in the execution receipt
  - string

# Security constraints
security:
  filesystem_access: string   # none | read_only | read_write [default: none]
  network_access: bool        # Whether network access is allowed [default: false]
  memory_limit: string        # Memory limit, e.g. "512MB" (optional)
  timeout: string             # Execution timeout, e.g. "30s" (optional)
  read_paths:                 # Allowed filesystem read paths (optional)
    - string
  write_paths:                # Allowed filesystem write paths (optional)
    - string
```

### Trust Levels

| Level | Value | Description |
|-------|-------|-------------|
| `LOCAL` | 0 | User's own plugin. User's responsibility. |
| `COMMUNITY` | 1 | Community-written, unreviewed. User must explicitly accept risk. |
| `VERIFIED` | 2 | Community-written, NabaOS-reviewed. Mostly trusted. |
| `OFFICIAL` | 3 | NabaOS team authored/audited. Fully trusted. |

Trust levels are ordered: `LOCAL < COMMUNITY < VERIFIED < OFFICIAL`.

### Ability Name Convention

Ability names use a dot-separated namespace: `category.action`. Examples:

- `files.read_psd` -- Read PSD files
- `media.transcode` -- Transcode media files
- `nlp.translate` -- Translate text

The ability name must contain only alphanumeric characters, dots, hyphens,
and underscores. Path traversal characters are rejected.

### Complete Plugin Example

```yaml
name: psd_reader
version: 1.0.0
author: nyaya-community
license: MIT
trust_level: VERIFIED

ability:
  name: files.read_psd
  description: "Read Adobe PSD files, extract layers and metadata"
  permission_tier: 2

input:
  path:
    type: string
    required: true
  extract_layers:
    type: bool
    default: true

output:
  layers: "array"
  width: "int"
  height: "int"

receipt_fields:
  - file_hash
  - layers_count
  - dimensions

security:
  filesystem_access: read_only
  network_access: false
  memory_limit: 512MB
  timeout: 30s
```

## Subprocess Abilities

Subprocess abilities wrap existing CLI tools (ffmpeg, tesseract, imagemagick,
etc.) as NabaOS abilities. They are defined in a YAML config file and
registered with `nyaya plugin register-subprocess`.

### Subprocess Config Schema

The config file is a YAML dictionary where each key is the ability name:

```yaml
ability_name:
  type: subprocess        # Must be "subprocess"
  command: string         # Command template with {param} placeholders
  description: string     # Human-readable description (optional)
  params:                 # Parameter definitions
    param_name:
      type: string        # Parameter type: string, int, bool, filepath
      default: value      # Default value (optional)
      required: bool      # Whether required (optional)
  sandbox:                # Security constraints
    read_paths: [string]
    write_paths: [string]
    network_access: bool
    timeout: string
    memory_limit: string
  receipt_fields: [string]
```

### Command Template

The `command` string supports `{param}` placeholders that are replaced
with parameter values at execution time. The command is split on whitespace
and executed directly -- no shell (`sh -c`) is involved.

**Security**: Parameter values are validated against shell metacharacter
injection. The following characters are blocked in all parameter values:

```
; | & ` $ \n \r \0 ( ) < > { } ' " \ (space) (tab)
```

If any parameter value contains a blocked character, execution is rejected.

The subprocess runs with a cleared environment (`env_clear()`), with only
`PATH=/usr/bin:/bin` set.

### Subprocess Example

```yaml
media.transcode:
  type: subprocess
  command: "ffmpeg -i {input} -vf scale={width}:{height} {output}"
  description: "Transcode video using ffmpeg"
  params:
    input:
      type: filepath
      required: true
    width:
      type: int
      default: 1920
    height:
      type: int
      default: 1080
    output:
      type: filepath
      auto: "{input}.mp4"
  sandbox:
    read_paths: ["/tmp/input"]
    write_paths: ["/tmp/output"]
    network_access: false
    timeout: 300s
    memory_limit: 2GB
  receipt_fields:
    - input_hash
    - output_hash
    - duration
    - exit_code
```

Register with:

```bash
nyaya plugin register-subprocess ./subprocess-abilities.yaml
```

### Timeout Enforcement

Subprocess timeout is enforced by polling the child process. If the process
does not complete within the configured timeout, it is killed. The timeout
string supports: `s` (seconds), `m` (minutes), `h` (hours). Default: 60
seconds.

## Cloud Abilities

Cloud abilities delegate to remote HTTP endpoints.

### Cloud Config Schema

```yaml
endpoint: string          # HTTPS URL (required, must use HTTPS)
method: string            # HTTP method: GET, POST, PUT [default: POST]
headers:                  # Request headers
  Header-Name: "value"
params:                   # Parameter definitions
  param_name:
    type: string
    required: bool
timeout_secs: u64         # Request timeout in seconds [default: 30]
description: string       # Human-readable description (optional)
receipt_fields: [string]  # Fields to include in the receipt
```

### Cloud Example

```yaml
endpoint: "https://api.example.com/v1/generate"
method: POST
headers:
  Authorization: "Bearer ${API_KEY}"
  Content-Type: "application/json"
params:
  prompt:
    type: string
    required: true
  max_tokens:
    type: int
    default: 1024
timeout_secs: 60
description: "Generate text via cloud LLM API"
receipt_fields:
  - request_id
  - generation_time
```

### SSRF Protection

Cloud abilities enforce strict SSRF (Server-Side Request Forgery)
protections:

- **HTTPS required**: Only `https://` endpoints are allowed.
- **Blocked hosts**: `localhost`, `127.0.0.1`, `0.0.0.0`, `[::1]`,
  `169.254.169.254`, `metadata.google.internal`.
- **Blocked networks**: Private IP ranges `10.0.0.0/8`,
  `192.168.0.0/16`, `172.16.0.0/12`.
- **No redirects**: The HTTP client follows zero redirects.

## Plugin Installation

Install a plugin from its manifest:

```bash
nyaya plugin install ./my-plugin/manifest.yaml
```

This copies the manifest and any associated shared library
(`lib<name>.so`) into the plugin directory
(`$NABA_DATA_DIR/plugins/<name>/`).

List installed plugins:

```bash
nyaya plugin list
```

Remove a plugin:

```bash
nyaya plugin remove psd_reader
```

Plugin names are validated against path traversal attacks. Names containing
`/`, `\`, or `..` are rejected.
