# AgentDeck

Always-on terminal dashboard for:

- latest AI news, translated through local Codex by default
- weather from Open-Meteo
- client calendar events from ICS URLs or files
- local Codex and Claude Code process/session/usage status
- OpenAI and Anthropic service status pages
- CPU, memory, disk, and top resource processes
- Docker containers grouped by project or Kubernetes
- listening TCP ports

## Build

```bash
cargo build --release
```

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
- `Tab`: cycle views
- `1`: Overview
- `2`: News
- `3`: Agent
- `4`: Ops
- `5`: Docker
- Mouse click on a tab: switch views
- Mouse click on a news headline: open the source article
- Mouse click on a Docker group: expand or collapse containers

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
