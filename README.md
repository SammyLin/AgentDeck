# AgentDeck

[![CI](https://github.com/SammyLin/AgentDeck/actions/workflows/ci.yml/badge.svg)](https://github.com/SammyLin/AgentDeck/actions/workflows/ci.yml)
[![Security audit](https://github.com/SammyLin/AgentDeck/actions/workflows/security.yml/badge.svg)](https://github.com/SammyLin/AgentDeck/actions/workflows/security.yml)
[![Release](https://github.com/SammyLin/AgentDeck/actions/workflows/release.yml/badge.svg)](https://github.com/SammyLin/AgentDeck/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/SammyLin/AgentDeck)](https://github.com/SammyLin/AgentDeck/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/SammyLin/AgentDeck/total)](https://github.com/SammyLin/AgentDeck/releases)
[![Rust](https://img.shields.io/badge/built_with-Rust-dca282?logo=rust)](https://www.rust-lang.org/)

Always-on terminal dashboard for:

- latest AI news, translated through local Codex by default
- weather from Open-Meteo
- client calendar events from ICS URLs or files
- local Codex and Claude Code process/session/usage status
- OpenAI and Anthropic service status pages
- CPU, memory, disk, and top resource processes
- Docker containers grouped by project or Kubernetes
- listening TCP ports

## Install

### One-line installer

```bash
curl -fsSL https://raw.githubusercontent.com/SammyLin/AgentDeck/main/install.sh | sh
```

The installer downloads the matching GitHub Release archive and verifies its
published SHA-256 checksum before installing it. Review the script first if you
do not want to pipe a remote script directly to a shell:

```bash
curl -fsSLO https://raw.githubusercontent.com/SammyLin/AgentDeck/main/install.sh
less install.sh
sh install.sh
```

By default this installs `agentdeck` to `~/.local/bin`. Override the install
location with `INSTALL_DIR`:

```bash
curl -fsSL https://raw.githubusercontent.com/SammyLin/AgentDeck/main/install.sh | INSTALL_DIR=/usr/local/bin sh
```

### Cargo

Users who already have Rust installed can build directly from the repository:

```bash
cargo install --git https://github.com/SammyLin/AgentDeck --locked
```

## Security

- Release archives are verified with SHA-256 before installation.
- GitHub generates build provenance attestations for release archives.
- Every change is tested on Linux and macOS in GitHub Actions.
- Dependencies are scanned weekly against the RustSec advisory database.
- Dependabot monitors both Cargo crates and GitHub Actions.

Please report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).
Badges above link to their live results; they are status indicators, not a
substitute for reviewing the source and installer.

With the GitHub CLI installed, a downloaded release archive can also be checked
against its build provenance:

```bash
gh attestation verify agentdeck-<os>-<arch>.tar.gz --repo SammyLin/AgentDeck
```

## Build

```bash
cargo build --release
```

## Release

Follow the complete [release checklist](RELEASING.md). In short, update the
version in `Cargo.toml`, run the release check, then create a matching tag:

```bash
./scripts/release-check.sh 0.1.0
git tag v0.1.0
git push origin v0.1.0
```

The GitHub Actions release workflow uploads `agentdeck-<os>-<arch>.tar.gz`
assets. The install script downloads those assets first, then falls back to
`cargo install --git` if a release asset is not available for the user's
platform.

## Run

```bash
cargo run
```

Or run the release binary:

```bash
./target/release/agentdeck
```

Keys:

- `q` or `Esc`: quit
- `r`: refresh all panels immediately
- `u`: install an available update, then restart AgentDeck
- `Tab`: cycle views
- `1`: Overview
- `2`: News
- `3`: Agent
- `4`: Ops
- `5`: Docker
- Mouse click on a tab: switch views
- Mouse click on a news headline: open the source article
- Mouse click on a Docker group: expand or collapse containers

AgentDeck checks GitHub Releases in the background at most once every 24 hours.
When a newer version is available, the header shows an update notice. Updates
are never installed without confirmation. You can also check or update from the
command line:

```bash
agentdeck update --check
agentdeck update
```

The default Overview gives the Codex / Claude session and usage panel more
space, then keeps weather, news, and system health visible. Detailed system,
ports, Docker, and agent data live in their own views so an 80-column terminal
stays readable.

AI news keeps source links clickable without rendering raw URLs. Results are
cached in `~/.cache/agentdeck/news.txt` and are reused on launch until the
configured news refresh interval expires.

For a non-interactive health check:

```bash
cargo run -- --once
```

## Configure

Copy `config.example.json` to either `./config.json` or
`~/.config/agentdeck/config.json`, then edit it.

```bash
cp config.example.json config.json
```

AI headlines are translated through local Codex by default:

```bash
cargo run
```

Default translation config:

```json
"translation": {
  "provider": "codex",
  "model": ""
}
```

Set `model` only if you want to force a specific Codex model. Set `provider`
to `none` to disable translation, or `openai` to use `OPENAI_API_KEY` directly.

Calendar supports public/private `.ics` URLs, such as Google Calendar secret
ICS links, or local `.ics` files. Weather uses latitude/longitude so it does not
need an API key.

## Notes

- The interactive dashboard uses `ratatui` and `crossterm` for layout, color,
  keyboard handling, and terminal raw mode.
- HTTP still uses the system `curl`, so `curl` must be available on the machine.
- Docker status uses `docker ps -a`; if Docker is not running the panel shows
  the command error.
- Port status uses `lsof` first, then falls back to `netstat`.
- Agent monitoring combines local process status, Codex session JSONL usage,
  Claude Code usage/cache files, and vendor status-page summaries.
