#!/usr/bin/env bash
set -euo pipefail

# Download ONNX models for nabaos local inference.
#
# Usage:
#   ./scripts/download-models.sh              # Download all models
#   ./scripts/download-models.sh --webbert    # Download WebBERT only
#   ./scripts/download-models.sh --setfit     # Download SetFit only

MODEL_DIR="${NABA_MODEL_PATH:-./models}"
mkdir -p "$MODEL_DIR"

BOLD='\033[1m'
GREEN='\033[32m'
CYAN='\033[36m'
DIM='\033[2m'
RESET='\033[0m'

# ---- Helpers ----------------------------------------------------------------

check_hf_cli() {
    if ! command -v hf &>/dev/null; then
        echo -e "${BOLD}hf CLI not found.${RESET}"
        echo "Install it with:  pip install huggingface_hub[cli]"
        echo "Then run:         hf auth login"
        exit 1
    fi
}

download_file() {
    local repo="$1"
    local file="$2"
    local dest="$3"

    if [ -f "$dest" ]; then
        echo -e "  ${DIM}Already exists: ${dest}${RESET}"
        return 0
    fi

    echo -e "  ${CYAN}Downloading ${file}...${RESET}"
    hf download "$repo" "$file" --local-dir "$MODEL_DIR" --quiet
    echo -e "  ${GREEN}OK${RESET}"
}

# ---- WebBERT (Layer 2 browser action classifier) ----------------------------

download_webbert() {
    local repo="biztiger/webbert-action-classifier"
    echo -e "${BOLD}WebBERT Action Classifier${RESET} (~256 MB)"
    echo -e "${DIM}  Repo: https://huggingface.co/${repo}${RESET}"

    download_file "$repo" "webbert.onnx" "$MODEL_DIR/webbert.onnx"
    download_file "$repo" "webbert-tokenizer.json" "$MODEL_DIR/webbert-tokenizer.json"
    download_file "$repo" "webbert-classes.json" "$MODEL_DIR/webbert-classes.json"

    echo -e "  ${GREEN}WebBERT ready.${RESET}"
    echo
}

# ---- SetFit W5H2 (intent classification) ------------------------------------

download_setfit() {
    echo -e "${BOLD}SetFit W5H2 Intent Classifier${RESET}"
    local setfit_dir="$MODEL_DIR/setfit-w5h2"
    mkdir -p "$setfit_dir"

    if [ -f "$setfit_dir/model.onnx" ]; then
        echo -e "  ${DIM}Already exists: ${setfit_dir}/model.onnx${RESET}"
    else
        echo -e "  ${DIM}SetFit model not yet available on HuggingFace.${RESET}"
        echo -e "  ${DIM}Place model.onnx manually in ${setfit_dir}/${RESET}"
    fi
    echo
}

# ---- Main -------------------------------------------------------------------

main() {
    local download_all=true
    local want_webbert=false
    local want_setfit=false

    for arg in "$@"; do
        case "$arg" in
            --webbert) want_webbert=true; download_all=false ;;
            --setfit)  want_setfit=true;  download_all=false ;;
            --help|-h)
                echo "Usage: $0 [--webbert] [--setfit]"
                echo "  --webbert   Download WebBERT action classifier only"
                echo "  --setfit    Download SetFit W5H2 model only"
                echo "  (no flags)  Download all models"
                exit 0
                ;;
        esac
    done

    echo -e "${BOLD}${CYAN}Nyaya Agent — Model Download${RESET}"
    echo -e "${DIM}Model directory: ${MODEL_DIR}${RESET}"
    echo

    check_hf_cli

    if $download_all || $want_webbert; then
        download_webbert
    fi

    if $download_all || $want_setfit; then
        download_setfit
    fi

    echo -e "${GREEN}${BOLD}Model setup complete.${RESET}"
}

main "$@"
