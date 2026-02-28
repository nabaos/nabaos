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
CONSTITUTION_URL="https://raw.githubusercontent.com/${REPO}/main/config/constitutions/general.toml"

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

# ─── Download default constitution if not present ───────────────────────────
ensure_default_constitution() {
    local dest="${DATA_DIR}/config/constitutions/general.toml"
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

# ─── PATH check with shell-specific advice ──────────────────────────────────
check_path() {
    if [[ ":${PATH}:" == *":${INSTALL_DIR}:"* ]]; then
        ok "${INSTALL_DIR} is already in your PATH"
        return
    fi

    warn "${INSTALL_DIR} is not in your PATH"
    printf "\n"
    printf "  Add it by running:\n"
    printf "\n"
    printf "    export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
    printf "\n"
    printf "  To make it permanent, add that line to your shell config:\n"
    printf "\n"

    local current_shell
    current_shell="$(basename "${SHELL:-/bin/bash}")"
    case "$current_shell" in
        zsh)
            printf "    echo 'export PATH=\"%s:\$PATH\"' >> ~/.zshrc\n" "$INSTALL_DIR"
            printf "    source ~/.zshrc\n"
            ;;
        fish)
            printf "    fish_add_path %s\n" "$INSTALL_DIR"
            ;;
        *)
            printf "    echo 'export PATH=\"%s:\$PATH\"' >> ~/.bashrc\n" "$INSTALL_DIR"
            printf "    source ~/.bashrc\n"
            ;;
    esac
    printf "\n"
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
    printf "  %sNext steps:%s\n" "${BOLD}" "${RESET}"
    printf "\n"
    printf "    %s1.%s  Run the setup wizard:\n" "${CYAN}" "${RESET}"
    printf "        %s setup\n" "$BINARY_NAME"
    printf "\n"
    printf "    %s2.%s  Set your LLM API key:\n" "${CYAN}" "${RESET}"
    printf "        export NABA_LLM_API_KEY=\"your-key-here\"\n"
    printf "\n"
    printf "    %s3.%s  Browse the agent catalog:\n" "${CYAN}" "${RESET}"
    printf "        %s catalog list\n" "$BINARY_NAME"
    printf "\n"
    printf "    %s4.%s  Start the daemon:\n" "${CYAN}" "${RESET}"
    printf "        %s daemon\n" "$BINARY_NAME"
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

    printf "\n"
    check_path
    success_banner
}

main "$@"
