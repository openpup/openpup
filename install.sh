#!/usr/bin/env bash
# openPup one-line installer: curl -fsSL https://raw.githubusercontent.com/OWNER/REPO/main/install.sh | bash
# Or with custom repo: OPENPUP_REPO=yourname/openpup bash -c "$(curl -fsSL https://raw.githubusercontent.com/yourname/openpup/main/install.sh)"

set -euo pipefail

REPO="${OPENPUP_REPO:-openpup/openpup}"
INSTALL_DIR="${OPENPUP_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${OPENPUP_VERSION:-latest}"

# Resolve OS and arch for artifact name
detect_platform() {
  local os arch
  case "$(uname -s)" in
    Linux)   os=linux ;;
    Darwin)  os=darwin ;;
    *)       echo "Unsupported OS: $(uname -s)" >&2; exit 1 ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) echo "Unsupported arch: $(uname -m)" >&2; exit 1 ;;
  esac
  echo "${os}-${arch}"
}

PLATFORM=$(detect_platform)

# Latest release JSON from GitHub
fetch_release() {
  if [[ "$VERSION" == "latest" ]]; then
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest"
  else
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/tags/v${VERSION#v}"
  fi
}

# Download URL for a given asset name (e.g. openpup-0.1.0-linux-x86_64.tar.gz)
get_asset_url() {
  local name="$1"
  fetch_release | jq -r --arg name "$name" '.assets[] | select(.name == $name) | .browser_download_url'
}

main() {
  if ! command -v jq &>/dev/null; then
    echo "jq is required. Install it (e.g. brew install jq, apt install jq) and re-run." >&2
    exit 1
  fi

  echo "openPup installer (repo=$REPO, platform=$PLATFORM, install_dir=$INSTALL_DIR)"
  # Get version from release tag (e.g. v0.1.0 -> 0.1.0)
  RELEASE_JSON=$(fetch_release)
  TAG=$(echo "$RELEASE_JSON" | jq -r '.tag_name')
  VER="${TAG#v}"
  ASSET_NAME="openpup-${VER}-${PLATFORM}.tar.gz"
  DOWNLOAD_URL=$(echo "$RELEASE_JSON" | jq -r --arg name "$ASSET_NAME" '.assets[] | select(.name == $name) | .browser_download_url')

  if [[ -z "$DOWNLOAD_URL" || "$DOWNLOAD_URL" == "null" ]]; then
    echo "No prebuilt binary for $PLATFORM. Available assets:" >&2
    echo "$RELEASE_JSON" | jq -r '.assets[].name' >&2
    echo "Build from source: cargo install --git https://github.com/${REPO}.git openpup" >&2
    exit 1
  fi

  mkdir -p "$INSTALL_DIR"
  TMP=$(mktemp -d)
  trap "rm -rf $TMP" EXIT
  echo "Downloading $ASSET_NAME ..."
  curl -fsSL -o "$TMP/openpup.tar.gz" "$DOWNLOAD_URL"
  tar -xzf "$TMP/openpup.tar.gz" -C "$TMP"
  mv "$TMP/openpup" "$INSTALL_DIR/openpup"
  chmod +x "$INSTALL_DIR/openpup"

  if ! echo ":$PATH:" | grep -q ":$INSTALL_DIR:"; then
    echo "Added $INSTALL_DIR to PATH in profile (if writable)."
    for f in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
      if [[ -f "$f" ]] && ! grep -q "OPENPUP_INSTALL_DIR\|$INSTALL_DIR" "$f" 2>/dev/null; then
        echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$f"
        echo "Appended PATH to $f"
        break
      fi
    done
  fi

  echo "Installed openpup $VER to $INSTALL_DIR/openpup"
  "$INSTALL_DIR/openpup" --version 2>/dev/null || true
  echo "Run: openpup init && openpup persona init && openpup up"
}

main "$@"
