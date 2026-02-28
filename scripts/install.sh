#!/bin/bash
#
# NabaOS installer — https://github.com/nabaos/nabaos
#
# Usage:
#   bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)
#
# NOTE: This script requires bash (not sh/dash). The curl|sh shorthand
# in the README pipes to bash explicitly.
#
set -euo pipefail

# ─── Configuration ──────────────────────────────────────────────────────────
REPO="nabaos/nabaos"
VERSION="${NABA_VERSION:-latest}"
INSTALL_DIR="${NABA_INSTALL_DIR:-$HOME/.local/bin}"
DATA_DIR="${NABA_DATA_DIR:-$HOME/.nabaos}"
BINARY_NAME="nabaos"
DOC_URL="https://nabaos.github.io/nabaos/"
CONSTITUTION_URL="https://raw.githubusercontent.com/${REPO}/main/config/constitutions/default.yaml"
MODELS_URL="https://github.com/${REPO}/releases/download/${VERSION:-latest}/models-setfit-w5h2.tar.gz"

# ─── Color helpers ──────────────────────────────────────────────────────────
if [ -t 1 ] && command -v tput &>/dev/null && [ "$(tput colors 2>/dev/null || echo 0)" -ge 8 ]; then
    BOLD="$(tput bold)"
    RESET="$(tput sgr0)"
    RED="$(tput setaf 1)"
    GREEN="$(tput setaf 2)"
    YELLOW="$(tput setaf 3)"
    BLUE="$(tput setaf 4)"
    CYAN="$(tput setaf 6)"
else
    BOLD="" RESET="" RED="" GREEN="" YELLOW="" BLUE="" CYAN=""
fi

info()  { printf "%s[info]%s  %s\n" "${BLUE}${BOLD}" "${RESET}" "$*"; }
ok()    { printf "%s[ ok ]%s  %s\n" "${GREEN}${BOLD}" "${RESET}" "$*"; }
warn()  { printf "%s[warn]%s  %s\n" "${YELLOW}${BOLD}" "${RESET}" "$*" >&2; }
fail()  { printf "%s[FAIL]%s  %s\n" "${RED}${BOLD}" "${RESET}" "$*" >&2; }

# ─── Banner ─────────────────────────────────────────────────────────────────
banner() {
    printf "\n"
    printf "%s" "${CYAN}${BOLD}"
    cat <<'ART'
    _   __      __          ____  _____
   / | / /___ _/ /_  ____ _/ __ \/ ___/
  /  |/ / __ `/ __ \/ __ `/ / / /\__ \
 / /|  / /_/ / /_/ / /_/ / /_/ /___/ /
/_/ |_/\__,_/_.___/\__,_/\____//____/
ART
    printf "%s" "${RESET}"
    printf "\n%s              NabaOS  —  Installer%s\n\n" "${BOLD}" "${RESET}"
}

# ─── Detect OS and arch ────────────────────────────────────────────────────
detect_platform() {
    local raw_os raw_arch
    raw_os="$(uname -s)"
    raw_arch="$(uname -m)"

    case "$raw_os" in
        Linux)  OS="linux"  ;;
        Darwin) OS="darwin" ;;
        *)
            fail "Unsupported operating system: $raw_os"
            exit 1
            ;;
    esac

    case "$raw_arch" in
        x86_64|amd64)   ARCH="amd64"  ;;
        aarch64|arm64)  ARCH="arm64"  ;;
        *)
            fail "Unsupported architecture: $raw_arch"
            exit 1
            ;;
    esac

    info "Detected platform: ${OS} / ${ARCH}"
}

# ─── Resolve latest version from GitHub API ─────────────────────────────────
resolve_version() {
    if [ "$VERSION" = "latest" ]; then
        info "Resolving latest release from GitHub..."
        local api_url="https://api.github.com/repos/${REPO}/releases/latest"
        if command -v curl &>/dev/null; then
            VERSION="$(curl -fsSL "$api_url" 2>/dev/null | grep '"tag_name"' | head -1 | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/')" || true
        elif command -v wget &>/dev/null; then
            VERSION="$(wget -qO- "$api_url" 2>/dev/null | grep '"tag_name"' | head -1 | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/')" || true
        fi
        if [ -z "${VERSION:-}" ] || [ "$VERSION" = "latest" ]; then
            warn "Could not resolve latest version from GitHub API"
            VERSION=""
            return 1
        fi
        ok "Latest release: ${VERSION}"
    else
        info "Using requested version: ${VERSION}"
    fi
    return 0
}

# ─── Download a URL to a file (curl or wget) ───────────────────────────────
download() {
    local url="$1" dest="$2"
    if command -v curl &>/dev/null; then
        curl -fsSL -o "$dest" "$url"
    elif command -v wget &>/dev/null; then
        wget -qO "$dest" "$url"
    else
        fail "Neither curl nor wget found. Cannot download files."
        return 1
    fi
}

# ─── Try to install a pre-built binary from GitHub Releases ─────────────────
try_binary_install() {
    resolve_version || return 1

    local archive="${BINARY_NAME}-${OS}-${ARCH}.tar.gz"
    local base_url="https://github.com/${REPO}/releases/download/${VERSION}"
    local download_url="${base_url}/${archive}"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp_dir'" EXIT

    info "Downloading ${archive} ..."
    if ! download "$download_url" "${tmp_dir}/${archive}"; then
        warn "Binary download failed (${download_url})"
        rm -rf "$tmp_dir"
        trap - EXIT
        return 1
    fi
    ok "Downloaded ${archive}"

    # ── SHA256 verification ──
    local sums_url="${base_url}/SHA256SUMS"
    if download "$sums_url" "${tmp_dir}/SHA256SUMS" 2>/dev/null; then
        info "Verifying SHA256 checksum..."
        local expected actual
        expected="$(grep "${archive}" "${tmp_dir}/SHA256SUMS" | awk '{print $1}')"
        if [ -n "$expected" ]; then
            if command -v sha256sum &>/dev/null; then
                actual="$(sha256sum "${tmp_dir}/${archive}" | awk '{print $1}')"
            elif command -v shasum &>/dev/null; then
                actual="$(shasum -a 256 "${tmp_dir}/${archive}" | awk '{print $1}')"
            else
                warn "No sha256sum or shasum available — skipping checksum verification"
                actual="$expected"
            fi
            if [ "$expected" != "$actual" ]; then
                fail "Checksum mismatch!"
                fail "  Expected: ${expected}"
                fail "  Got:      ${actual}"
                rm -rf "$tmp_dir"
                trap - EXIT
                return 1
            fi
            ok "Checksum verified"
        else
            warn "Archive not found in SHA256SUMS — skipping verification"
        fi
    else
        warn "SHA256SUMS not available — skipping checksum verification"
    fi

    # ── Extract ──
    info "Extracting to ${INSTALL_DIR} ..."
    mkdir -p "$INSTALL_DIR"
    tar -xzf "${tmp_dir}/${archive}" -C "$tmp_dir"

    # The tarball may contain the binary at the top level or in a subdirectory.
    local binary_path
    binary_path="$(find "$tmp_dir" -name "$BINARY_NAME" -type f | head -1)"
    if [ -z "$binary_path" ]; then
        warn "Binary not found in archive"
        rm -rf "$tmp_dir"
        trap - EXIT
        return 1
    fi

    install -m 755 "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"
    ok "Installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"

    rm -rf "$tmp_dir"
    trap - EXIT
    return 0
}

# ─── Fallback: build from source with cargo ─────────────────────────────────
try_cargo_install() {
    if ! command -v cargo &>/dev/null; then
        return 1
    fi
    info "Building from source with cargo (this may take a few minutes)..."
    cargo install --git "https://github.com/${REPO}.git" --root "$HOME/.local"
    ok "Built and installed ${BINARY_NAME} via cargo"
    return 0
}

# ─── Fatal: nothing worked ──────────────────────────────────────────────────
install_failed() {
    printf "\n"
    fail "Could not install ${BINARY_NAME} via binary download or cargo."
    printf "\n"
    printf "  %sOption 1 — Install Rust, then re-run:%s\n" "${BOLD}" "${RESET}"
    printf "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n"
    printf "    source \"\$HOME/.cargo/env\"\n"
    printf "    bash <(curl -fsSL https://raw.githubusercontent.com/%s/main/scripts/install.sh)\n" "$REPO"
    printf "\n"
    printf "  %sOption 2 — Run via Docker:%s\n" "${BOLD}" "${RESET}"
    printf "    docker run --rm -it ghcr.io/%s:latest\n" "$REPO"
    printf "\n"
    printf "  Docs: %s\n\n" "$DOC_URL"
    exit 1
}

# ─── Create data directories ────────────────────────────────────────────────
create_directories() {
    info "Creating data directories under ${DATA_DIR} ..."
    local dirs=(
        agents
        plugins
        catalog
        models
        config/constitutions
        logs
    )
    for d in "${dirs[@]}"; do
        mkdir -p "${DATA_DIR}/${d}"
    done
    ok "Data directories ready"
}

# ─── Download agent catalog ───────────────────────────────────────────────────
ensure_agent_catalog() {
    local catalog_dir="${DATA_DIR}/catalog"
    local catalog_url="https://github.com/${REPO}/archive/refs/heads/main.tar.gz"

    # Skip if catalog already has content
    if [ -n "$(ls -A "${catalog_dir}" 2>/dev/null)" ]; then
        ok "Agent catalog already populated ($(ls "${catalog_dir}" | wc -l) agents)"
        return
    fi

    info "Downloading agent catalog..."
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    if download "$catalog_url" "${tmp_dir}/repo.tar.gz" 2>/dev/null; then
        tar -xzf "${tmp_dir}/repo.tar.gz" -C "$tmp_dir" --wildcards '*/catalog/*' 2>/dev/null || true
        local extracted_catalog
        extracted_catalog="$(find "$tmp_dir" -type d -name "catalog" | head -1)"
        if [ -n "$extracted_catalog" ] && [ -d "$extracted_catalog" ]; then
            cp -r "${extracted_catalog}"/* "${catalog_dir}/" 2>/dev/null || true
            local count
            count="$(ls "${catalog_dir}" | wc -l)"
            ok "Agent catalog installed (${count} agents)"
        else
            warn "Could not extract agent catalog"
        fi
    else
        warn "Could not download agent catalog (you can add agents later)"
    fi
    rm -rf "$tmp_dir"
}

# ─── Download ML models for local classification ─────────────────────────────
download_models() {
    local models_dir="${DATA_DIR}/models/setfit-w5h2"
    if [ -f "${models_dir}/model.onnx" ]; then
        ok "ML models already present"
        return
    fi

    # Need a resolved version for the download URL
    if [ -z "${VERSION:-}" ] || [ "$VERSION" = "latest" ]; then
        warn "Skipping model download (version not resolved)"
        return
    fi

    local models_url="https://github.com/${REPO}/releases/download/${VERSION}/models-setfit-w5h2.tar.gz"
    info "Downloading W5H2 intent classifier models (~80 MB)..."
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    if download "$models_url" "${tmp_dir}/models.tar.gz" 2>/dev/null; then
        mkdir -p "${models_dir}"
        tar -xzf "${tmp_dir}/models.tar.gz" -C "${DATA_DIR}/models/"
        ok "ML models installed to ${models_dir}"
    else
        warn "Could not download ML models (local classification disabled)"
        info "Download manually from: ${models_url}"
    fi
    rm -rf "$tmp_dir"
}

# ─── Install ONNX Runtime (for local AI classification) ──────────────────────
install_onnx_runtime() {
    # Skip if already installed (check actual file, not just ldconfig cache)
    if [ -f "/usr/local/lib/libonnxruntime.so" ] || [ -f "${DATA_DIR}/lib/libonnxruntime.so" ]; then
        ok "ONNX Runtime already installed"
        return
    fi

    info "Installing ONNX Runtime 1.23.0 (for local AI classification)..."
    local ort_version="1.23.0"
    local ort_arch
    case "$ARCH" in
        amd64) ort_arch="x64" ;;
        arm64) ort_arch="aarch64" ;;
        *) warn "No ONNX Runtime binary for $ARCH — local classification disabled"; return ;;
    esac

    local ort_url="https://github.com/microsoft/onnxruntime/releases/download/v${ort_version}/onnxruntime-linux-${ort_arch}-${ort_version}.tgz"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    if download "$ort_url" "${tmp_dir}/ort.tgz" 2>/dev/null; then
        tar -xzf "${tmp_dir}/ort.tgz" -C "$tmp_dir"
        local lib_dir
        lib_dir="$(find "$tmp_dir" -type d -name "lib" | head -1)"
        if [ -n "$lib_dir" ]; then
            if [ -w /usr/local/lib ]; then
                cp "${lib_dir}"/libonnxruntime.so* /usr/local/lib/
                ldconfig 2>/dev/null || true
                ok "ONNX Runtime ${ort_version} installed to /usr/local/lib"
            else
                # No root access — install to user lib dir
                mkdir -p "${DATA_DIR}/lib"
                cp "${lib_dir}"/libonnxruntime.so* "${DATA_DIR}/lib/"
                ok "ONNX Runtime ${ort_version} installed to ${DATA_DIR}/lib"
                warn "Add to your shell: export LD_LIBRARY_PATH=\"${DATA_DIR}/lib:\$LD_LIBRARY_PATH\""
            fi
        fi
    else
        warn "Could not download ONNX Runtime — local classification disabled"
        info "Install manually: https://onnxruntime.ai/docs/install/"
    fi
    rm -rf "$tmp_dir"
}

# ─── Download default constitution if not present ───────────────────────────
ensure_default_constitution() {
    local dest="${DATA_DIR}/config/constitutions/default.yaml"
    if [ -f "$dest" ]; then
        ok "Default constitution already exists — skipping download"
        return
    fi
    info "Downloading default constitution..."
    if download "$CONSTITUTION_URL" "$dest" 2>/dev/null; then
        ok "Default constitution saved to ${dest}"
    else
        warn "Could not download default constitution (you can add one later)"
    fi
}

# ─── Ensure INSTALL_DIR is in PATH (auto-configure) ─────────────────────────
ensure_path() {
    if [[ ":${PATH}:" == *":${INSTALL_DIR}:"* ]]; then
        ok "${INSTALL_DIR} is already in your PATH"
        return
    fi

    info "Adding ${INSTALL_DIR} to your PATH..."

    local current_shell rc_file export_line
    current_shell="$(basename "${SHELL:-/bin/bash}")"
    export_line="export PATH=\"${INSTALL_DIR}:\$PATH\""

    case "$current_shell" in
        zsh)  rc_file="$HOME/.zshrc" ;;
        fish) rc_file="" ;;  # fish uses a different mechanism
        *)    rc_file="$HOME/.bashrc" ;;
    esac

    local model_line="export NABA_MODEL_PATH=\"${DATA_DIR}/models/setfit-w5h2\""

    if [ "$current_shell" = "fish" ]; then
        # fish doesn't use export PATH=..., it has its own command
        fish -c "fish_add_path ${INSTALL_DIR}" 2>/dev/null || true
        ok "Added ${INSTALL_DIR} to fish PATH"
    elif [ -n "$rc_file" ]; then
        # Only add if not already present in the rc file
        if ! grep -qF "$INSTALL_DIR" "$rc_file" 2>/dev/null; then
            printf '\n# Added by NabaOS installer\n%s\n' "$export_line" >> "$rc_file"
            ok "Added PATH entry to ${rc_file}"
        else
            ok "PATH entry already in ${rc_file}"
        fi
        # Add model path if models were downloaded
        if [ -f "${DATA_DIR}/models/setfit-w5h2/model.onnx" ]; then
            if ! grep -qF "NABA_MODEL_PATH" "$rc_file" 2>/dev/null; then
                printf '%s\n' "$model_line" >> "$rc_file"
                ok "Added NABA_MODEL_PATH to ${rc_file}"
            fi
        fi
        # Add ORT_DYLIB_PATH for ONNX Runtime dynamic loading
        if [ -f "/usr/local/lib/libonnxruntime.so" ]; then
            if ! grep -qF "ORT_DYLIB_PATH" "$rc_file" 2>/dev/null; then
                printf 'export ORT_DYLIB_PATH="/usr/local/lib/libonnxruntime.so"\n' >> "$rc_file"
                ok "Added ORT_DYLIB_PATH to ${rc_file}"
            fi
        fi
    fi

    # Make it available for the rest of this script
    export PATH="${INSTALL_DIR}:$PATH"
    export NABA_MODEL_PATH="${DATA_DIR}/models/setfit-w5h2"
    ok "${BINARY_NAME} is now available in this session"
}

# ─── Success banner with next steps ─────────────────────────────────────────
success_banner() {
    printf "\n"
    printf "%s" "${GREEN}${BOLD}"
    printf "  ╔══════════════════════════════════════════════╗\n"
    printf "  ║       Installation Complete!                 ║\n"
    printf "  ╚══════════════════════════════════════════════╝\n"
    printf "%s" "${RESET}"
    printf "\n"

    # Remind user to reload their shell if PATH was just added
    local current_shell
    current_shell="$(basename "${SHELL:-/bin/bash}")"
    if ! command -v "$BINARY_NAME" &>/dev/null 2>&1; then
        case "$current_shell" in
            zsh)  printf "  %sReload your shell:%s  source ~/.zshrc\n\n" "${YELLOW}" "${RESET}" ;;
            fish) printf "  %sReload your shell:%s  exec fish\n\n" "${YELLOW}" "${RESET}" ;;
            *)    printf "  %sReload your shell:%s  source ~/.bashrc  (or open a new terminal)\n\n" "${YELLOW}" "${RESET}" ;;
        esac
    fi

    printf "  %sGet started:%s\n" "${BOLD}" "${RESET}"
    printf "\n"
    printf "    %s1.%s  Run the setup wizard (configures your LLM key + constitution):\n" "${CYAN}" "${RESET}"
    printf "        %s setup\n" "$BINARY_NAME"
    printf "\n"
    printf "    %s2.%s  Start the agent:\n" "${CYAN}" "${RESET}"
    printf "        %s start\n" "$BINARY_NAME"
    printf "\n"
    printf "  %sDocs:%s  %s\n" "${BOLD}" "${RESET}" "$DOC_URL"
    printf "\n"
}

# ─── Main ───────────────────────────────────────────────────────────────────
main() {
    banner

    info "Install directory : ${INSTALL_DIR}"
    info "Data directory    : ${DATA_DIR}"
    printf "\n"

    detect_platform

    printf "\n"
    info "Attempting pre-built binary install..."
    if try_binary_install; then
        : # binary installed
    else
        printf "\n"
        info "Pre-built binary not available — trying cargo install..."
        if ! try_cargo_install; then
            install_failed
        fi
    fi

    printf "\n"
    create_directories
    ensure_default_constitution
    ensure_agent_catalog
    download_models

    # Install ONNX Runtime for local classification (Linux only)
    if [ "$OS" = "linux" ]; then
        printf "\n"
        install_onnx_runtime
    fi

    printf "\n"
    ensure_path

    # Check if binary was built without BERT and print guidance
    if command -v "$BINARY_NAME" &>/dev/null || [ -x "${INSTALL_DIR}/${BINARY_NAME}" ]; then
        local bin_path
        bin_path="$(command -v "$BINARY_NAME" 2>/dev/null || echo "${INSTALL_DIR}/${BINARY_NAME}")"
        if "$bin_path" admin classify "test" 2>&1 | grep -qi "without BERT"; then
            printf "\n"
            info "This binary was built without local AI classification (Tiers 1-2)."
            info "To enable it, install ONNX Runtime:"
            info "  https://onnxruntime.ai/docs/install/"
        fi
    fi

    success_banner
}

main "$@"
