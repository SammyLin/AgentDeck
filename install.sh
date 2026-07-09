#!/usr/bin/env sh
set -eu

REPO="${AGENTDECK_REPO:-SammyLin/AgentDeck}"
BIN_NAME="agentdeck"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BASE_URL="https://github.com/$REPO"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "agentdeck installer: missing required command: $1" >&2
    exit 1
  fi
}

detect_platform() {
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os" in
    darwin) os="darwin" ;;
    linux) os="linux" ;;
    *)
      echo "agentdeck installer: unsupported OS: $os" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="aarch64" ;;
    *)
      echo "agentdeck installer: unsupported architecture: $arch" >&2
      exit 1
      ;;
  esac

  echo "$os-$arch"
}

install_from_cargo() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "agentdeck installer: no matching release asset and Rust cargo is not installed." >&2
    echo "Install Rust from https://rustup.rs, or create a GitHub Release for this platform." >&2
    exit 1
  fi

  echo "Installing AgentDeck with cargo from $BASE_URL..."
  cargo install --git "$BASE_URL.git" --locked --force
}

install_from_release() {
  platform="$1"
  version="${AGENTDECK_VERSION:-latest}"

  if [ "$version" = "latest" ]; then
    url="$BASE_URL/releases/latest/download/agentdeck-$platform.tar.gz"
  else
    url="$BASE_URL/releases/download/$version/agentdeck-$platform.tar.gz"
  fi

  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT INT TERM

  echo "Downloading AgentDeck release for $platform..."
  if ! curl -fsSL "$url" -o "$tmp_dir/agentdeck.tar.gz"; then
    return 1
  fi

  tar -xzf "$tmp_dir/agentdeck.tar.gz" -C "$tmp_dir"
  mkdir -p "$INSTALL_DIR"
  mv "$tmp_dir/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
  chmod 755 "$INSTALL_DIR/$BIN_NAME"

  echo "AgentDeck installed to $INSTALL_DIR/$BIN_NAME"
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
      echo "Note: $INSTALL_DIR is not in PATH."
      echo "Add this to your shell profile:"
      echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
      ;;
  esac
}

main() {
  need uname
  need curl
  need tar
  need mktemp

  platform="$(detect_platform)"
  if ! install_from_release "$platform"; then
    echo "Release binary not found for $platform; falling back to cargo install."
    install_from_cargo
  fi
}

main "$@"
