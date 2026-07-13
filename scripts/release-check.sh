#!/usr/bin/env sh
set -eu

requested="${1:-}"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"

if [ -z "$version" ]; then
  echo "release check: could not read the package version from Cargo.toml" >&2
  exit 1
fi

if [ -n "$requested" ]; then
  requested="${requested#v}"
  if [ "$requested" != "$version" ]; then
    echo "release check: requested v$requested but Cargo.toml is v$version" >&2
    exit 1
  fi
fi

lock_version="$(awk '
  $0 == "name = \"agentdeck\"" { found = 1; next }
  found && /^version = / { gsub(/^version = \"|\"$/, ""); print; exit }
' Cargo.lock)"
if [ "$lock_version" != "$version" ]; then
  echo "release check: Cargo.lock is v$lock_version but Cargo.toml is v$version" >&2
  echo "Run cargo check to update Cargo.lock." >&2
  exit 1
fi

cargo fmt --all -- --check
cargo test --locked
cargo build --release --locked

echo "release check: AgentDeck v$version is ready to tag"
