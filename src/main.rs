use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, Tabs, Wrap,
};
use ratatui::{Frame, Terminal};
use unicode_width::UnicodeWidthChar;

const APP_NAME: &str = "agentdeck";
const DISPLAY_NAME: &str = "AgentDeck";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES_API_URL: &str = "https://api.github.com/repos/SammyLin/AgentDeck/releases/latest";
const RELEASES_URL: &str = "https://github.com/SammyLin/AgentDeck/releases";
const UPDATE_CHECK_INTERVAL_SECS: u64 = 86_400;

#[derive(Clone)]
struct Config {
    refresh: Refresh,
    news: NewsConfig,
    translation: TranslationConfig,
    weather: WeatherConfig,
    calendar: CalendarConfig,
    agents: AgentConfig,
    docker_limit: usize,
    port_limit: usize,
    top_processes: usize,
}

#[derive(Clone)]
struct Refresh {
    news: u64,
    weather: u64,
    calendar: u64,
    agents: u64,
    system: u64,
    docker: u64,
    ports: u64,
}

#[derive(Clone)]
struct NewsConfig {
    rss_urls: Vec<String>,
    limit: usize,
    translate_to: String,
}

#[derive(Clone)]
struct TranslationConfig {
    provider: String,
    model: String,
}

#[derive(Clone)]
struct WeatherConfig {
    location: String,
    latitude: f64,
    longitude: f64,
    timezone: String,
}

#[derive(Clone)]
struct CalendarConfig {
    lookahead_days: i64,
    ics_urls: Vec<String>,
    ics_files: Vec<String>,
}

#[derive(Clone)]
struct AgentConfig {
    codex_keywords: Vec<String>,
    claude_keywords: Vec<String>,
    openai_status_url: String,
    anthropic_status_url: String,
}

#[derive(Clone)]
struct Panel {
    title: &'static str,
    lines: Vec<String>,
    error: Option<String>,
    updated: Option<String>,
    loading: bool,
}

type SharedPanels = Arc<Mutex<BTreeMap<&'static str, Panel>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum ClickAction {
    OpenUrl(String),
    ToggleDockerGroup(String),
    SwitchTab(usize),
}

#[derive(Clone)]
struct ClickZone {
    rect: Rect,
    action: ClickAction,
}

#[derive(Default)]
struct DashboardHistory {
    last_sample: Option<Instant>,
    cpu: VecDeque<u64>,
    memory: VecDeque<u64>,
    disk: VecDeque<u64>,
    codex_5h: VecDeque<u64>,
    codex_weekly: VecDeque<u64>,
    claude_5h: VecDeque<u64>,
    claude_weekly: VecDeque<u64>,
}

#[derive(Default)]
struct UiState {
    expanded_docker_groups: BTreeSet<String>,
}

#[derive(Clone)]
struct UpdateInfo {
    version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttentionItem {
    label: String,
    detail: String,
    tab: usize,
    critical: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DashboardSummary {
    active_agents: usize,
    running_services: usize,
    total_services: usize,
    listening_ports: usize,
    attention: Vec<AttentionItem>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AgentLimits {
    session_used: Option<u64>,
    weekly_used: Option<u64>,
}

const NEWS_LINK_PREFIX: &str = "@@link ";
const TAB_COUNT: usize = 5;
const HISTORY_LIMIT: usize = 72;
const AGENT_SERVICE_STATUS_INTERVAL_SECS: u64 = 300;
const CLAUDE_USAGE_INTERVAL_SECS: u64 = 60;
const TAB_LABELS: [&str; TAB_COUNT] = [
    "1  OVERVIEW",
    "2  NEWS + DAY",
    "3  AGENTS",
    "4  SYSTEM",
    "5  SERVICES",
];
const COLOR_SIGNAL: Color = Color::Rgb(96, 214, 170);
const COLOR_CYAN: Color = Color::Rgb(103, 194, 207);
const COLOR_AMBER: Color = Color::Rgb(232, 171, 92);
const COLOR_DANGER: Color = Color::Rgb(235, 105, 116);

fn default_config() -> Config {
    Config {
        refresh: Refresh {
            news: 900,
            weather: 900,
            calendar: 300,
            agents: 5,
            system: 3,
            docker: 8,
            ports: 20,
        },
        news: NewsConfig {
            rss_urls: vec![
                "https://techcrunch.com/category/artificial-intelligence/feed/".to_string(),
                "https://venturebeat.com/category/ai/feed/".to_string(),
                "https://www.artificialintelligence-news.com/feed/".to_string(),
            ],
            limit: 8,
            translate_to: "Traditional Chinese".to_string(),
        },
        translation: TranslationConfig {
            provider: "codex".to_string(),
            model: "".to_string(),
        },
        weather: WeatherConfig {
            location: "Taipei".to_string(),
            latitude: 25.033,
            longitude: 121.5654,
            timezone: "auto".to_string(),
        },
        calendar: CalendarConfig {
            lookahead_days: 7,
            ics_urls: Vec::new(),
            ics_files: Vec::new(),
        },
        agents: AgentConfig {
            codex_keywords: vec!["codex".to_string()],
            claude_keywords: vec!["claude".to_string(), "claude-code".to_string()],
            openai_status_url: "https://status.openai.com/api/v2/summary.json".to_string(),
            anthropic_status_url: "https://status.anthropic.com/api/v2/summary.json".to_string(),
        },
        docker_limit: 80,
        port_limit: 16,
        top_processes: 8,
    }
}

fn default_config_json() -> &'static str {
    r#"{
  "news": {
    "rss_urls": [
      "https://techcrunch.com/category/artificial-intelligence/feed/",
      "https://venturebeat.com/category/ai/feed/",
      "https://www.artificialintelligence-news.com/feed/"
    ],
    "limit": 8,
    "translate_to": "Traditional Chinese"
  },
  "translation": {
    "provider": "codex",
    "model": ""
  },
  "weather": {
    "location": "Taipei",
    "latitude": 25.033,
    "longitude": 121.5654,
    "timezone": "auto"
  },
  "calendar": {
    "lookahead_days": 7,
    "ics_urls": [],
    "ics_files": []
  },
  "agents": {
    "process_keywords": {
      "codex": ["codex"],
      "claude": ["claude", "claude-code"]
    },
    "status_urls": {
      "openai": "https://status.openai.com/api/v2/summary.json",
      "anthropic": "https://status.anthropic.com/api/v2/summary.json"
    }
  },
  "ports": {
    "limit": 16
  },
  "docker": {
    "limit": 80
  },
  "system": {
    "top_processes": 8
  }
}"#
}

fn section<'a>(json: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("\"{}\"", name);
    let start = json.find(&needle)?;
    let after_key = start + needle.len();
    let colon = json[after_key..].find(':')? + after_key + 1;
    let value = json[colon..].trim_start();
    if !value.starts_with('{') {
        return None;
    }
    let brace = json.len() - value.len();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in json[brace..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(&json[brace..brace + offset + 1]);
            }
        }
    }
    None
}

fn json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)?;
    let colon = json[start..].find(':')? + start;
    let quote = json[colon..].find('"')? + colon + 1;
    let mut out = String::new();
    let mut escaped = false;
    for ch in json[quote..].chars() {
        if escaped {
            out.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn json_number(json: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)?;
    let colon = json[start..].find(':')? + start + 1;
    let rest = json[colon..].trim_start();
    let len = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-'))
        .count();
    rest.get(..len)?.parse().ok()
}

fn json_string_array(json: &str, key: &str) -> Option<Vec<String>> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)?;
    let open = json[start..].find('[')? + start;
    let close = json[open..].find(']')? + open;
    let body = &json[open + 1..close];
    let mut values = Vec::new();
    let mut i = 0usize;
    while let Some(pos) = body[i..].find('"') {
        let start_quote = i + pos + 1;
        let mut out = String::new();
        let mut escaped = false;
        let mut consumed = start_quote;
        for (offset, ch) in body[start_quote..].char_indices() {
            consumed = start_quote + offset + ch.len_utf8();
            if escaped {
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                values.push(out);
                break;
            } else {
                out.push(ch);
            }
        }
        i = consumed;
        if i >= body.len() {
            break;
        }
    }
    Some(values)
}

fn load_config(path: Option<String>) -> (Config, String) {
    let mut config = default_config();
    let candidates = if let Some(path) = path {
        vec![path]
    } else {
        let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        vec![
            "config.json".to_string(),
            format!("{}/.config/{}/config.json", home, APP_NAME),
        ]
    };

    for candidate in candidates {
        let Ok(json) = fs::read_to_string(&candidate) else {
            continue;
        };
        if let Some(news) = section(&json, "news") {
            if let Some(v) = json_string_array(news, "rss_urls") {
                config.news.rss_urls = v;
            }
            if let Some(v) = json_number(news, "limit") {
                config.news.limit = v as usize;
            }
            if let Some(v) = json_string(news, "translate_to") {
                config.news.translate_to = v;
            }
        }
        if let Some(translation) = section(&json, "translation") {
            if let Some(v) = json_string(translation, "provider") {
                config.translation.provider = v;
            }
            if let Some(v) = json_string(translation, "model") {
                config.translation.model = v;
            }
        }
        if let Some(weather) = section(&json, "weather") {
            if let Some(v) = json_string(weather, "location") {
                config.weather.location = v;
            }
            if let Some(v) = json_number(weather, "latitude") {
                config.weather.latitude = v;
            }
            if let Some(v) = json_number(weather, "longitude") {
                config.weather.longitude = v;
            }
            if let Some(v) = json_string(weather, "timezone") {
                config.weather.timezone = v;
            }
        }
        if let Some(calendar) = section(&json, "calendar") {
            if let Some(v) = json_number(calendar, "lookahead_days") {
                config.calendar.lookahead_days = v as i64;
            }
            if let Some(v) = json_string_array(calendar, "ics_urls") {
                config.calendar.ics_urls = v;
            }
            if let Some(v) = json_string_array(calendar, "ics_files") {
                config.calendar.ics_files = v;
            }
        }
        if let Some(agents) = section(&json, "agents") {
            if let Some(process) = section(agents, "process_keywords") {
                if let Some(v) = json_string_array(process, "codex") {
                    config.agents.codex_keywords = v;
                }
                if let Some(v) = json_string_array(process, "claude") {
                    config.agents.claude_keywords = v;
                }
            }
            if let Some(status) = section(agents, "status_urls") {
                if let Some(v) = json_string(status, "openai") {
                    config.agents.openai_status_url = v;
                }
                if let Some(v) = json_string(status, "anthropic") {
                    config.agents.anthropic_status_url = v;
                }
            }
        }
        if let Some(docker) = section(&json, "docker") {
            if let Some(v) = json_number(docker, "limit") {
                config.docker_limit = v as usize;
            }
        }
        if let Some(ports) = section(&json, "ports") {
            if let Some(v) = json_number(ports, "limit") {
                config.port_limit = v as usize;
            }
        }
        if let Some(system) = section(&json, "system") {
            if let Some(v) = json_number(system, "top_processes") {
                config.top_processes = v as usize;
            }
        }
        return (config, candidate);
    }
    (config, "defaults".to_string())
}

fn run_command(program: &str, args: &[&str], timeout_secs: u64) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("{}: {}", program, err))?;
    let start = Instant::now();
    loop {
        if let Some(_status) = child.try_wait().map_err(|err| err.to_string())? {
            let output = child.wait_with_output().map_err(|err| err.to_string())?;
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("{} exited with {}", program, output.status)
            } else {
                stderr
            });
        }
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            let _ = child.kill();
            return Err(format!("{} timed out after {}s", program, timeout_secs));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("{}: {}", program, err))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("{} exited with {}", program, output.status)
    } else {
        stderr
    })
}

fn run_command_with_stdin(
    program: &str,
    args: &[String],
    input: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("{}: {}", program, err))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|err| format!("{} stdin: {}", program, err))?;
    }

    let start = Instant::now();
    loop {
        if let Some(_status) = child.try_wait().map_err(|err| err.to_string())? {
            let output = child.wait_with_output().map_err(|err| err.to_string())?;
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("{} exited with {}", program, output.status)
            } else {
                stderr
            });
        }
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            let _ = child.kill();
            return Err(format!("{} timed out after {}s", program, timeout_secs));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn http_get(url: &str, timeout_secs: u64) -> Result<String, String> {
    run_command(
        "curl",
        &[
            "-fsSL",
            "--max-time",
            &timeout_secs.to_string(),
            "-A",
            "agentdeck/0.1",
            url,
        ],
        timeout_secs + 2,
    )
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let command = ("open", vec![url]);
    #[cfg(target_os = "linux")]
    let command = ("xdg-open", vec![url]);
    #[cfg(target_os = "windows")]
    let command = ("cmd", vec!["/C", "start", "", url]);

    let _ = Command::new(command.0).args(command.1).spawn();
}

fn decode_entities(input: &str) -> String {
    let mut out = input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");
    while let Some(start) = out.find("&#") {
        let Some(end_offset) = out[start..].find(';') else {
            break;
        };
        let end = start + end_offset;
        let entity = &out[start + 2..end];
        let parsed = if let Some(hex) = entity
            .strip_prefix('x')
            .or_else(|| entity.strip_prefix('X'))
        {
            u32::from_str_radix(hex, 16).ok()
        } else {
            entity.parse::<u32>().ok()
        };
        let Some(ch) = parsed.and_then(char::from_u32) else {
            break;
        };
        out.replace_range(start..=end, &ch.to_string());
    }
    out
}

fn source_from_link(link: &str) -> String {
    link.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("unknown source")
        .trim_start_matches("www.")
        .to_string()
}

fn strip_html(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_entities(out.split_whitespace().collect::<Vec<_>>().join(" ").trim())
}

fn tag(body: &str, name: &str) -> Option<String> {
    let open = format!("<{}", name);
    let start = body.find(&open)?;
    let after_open = body[start..].find('>')? + start + 1;
    let close = format!("</{}>", name);
    let end = body[after_open..].find(&close)? + after_open;
    let mut value = body[after_open..end].trim().to_string();
    if value.starts_with("<![CDATA[") && value.ends_with("]]>") {
        value = value
            .trim_start_matches("<![CDATA[")
            .trim_end_matches("]]>")
            .to_string();
    }
    Some(strip_html(&value))
}

fn cache_dir() -> PathBuf {
    if let Ok(dir) = env::var("XDG_CACHE_HOME") {
        PathBuf::from(dir).join(APP_NAME)
    } else {
        home_file(".cache").join(APP_NAME)
    }
}

fn update_cache_path() -> PathBuf {
    cache_dir().join("update-check.txt")
}

fn normalized_version(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(|ch: char| !ch.is_ascii_digit())
        .take(3)
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(3)
        .collect()
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    normalized_version(candidate) > normalized_version(current)
}

fn read_cached_update() -> Option<String> {
    let text = fs::read_to_string(update_cache_path()).ok()?;
    let mut lines = text.lines();
    let checked_at = lines.next()?.parse::<u64>().ok()?;
    if now_secs().saturating_sub(checked_at) >= UPDATE_CHECK_INTERVAL_SECS {
        return None;
    }
    lines
        .next()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn write_update_cache(version: &str) {
    let dir = cache_dir();
    if fs::create_dir_all(&dir).is_ok() {
        let _ = fs::write(
            update_cache_path(),
            format!("{}\n{}\n", now_secs(), version),
        );
    }
}

fn latest_release_version(force: bool) -> Result<String, String> {
    if !force {
        if let Some(version) = read_cached_update() {
            return Ok(version);
        }
    }
    let json = run_command(
        "curl",
        &[
            "-fsSL",
            "--max-time",
            "15",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: agentdeck-update-check",
            RELEASES_API_URL,
        ],
        20,
    )?;
    let version = json_string(&json, "tag_name")
        .ok_or_else(|| "latest GitHub release did not contain a tag_name".to_string())?;
    write_update_cache(&version);
    Ok(version)
}

fn available_update(force: bool) -> Result<Option<UpdateInfo>, String> {
    let version = latest_release_version(force)?;
    Ok(is_newer_version(&version, VERSION).then_some(UpdateInfo { version }))
}

fn release_platform() -> Result<&'static str, String> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "x86_64") => Ok("darwin-x86_64"),
        ("macos", "aarch64") => Ok("darwin-aarch64"),
        ("linux", "x86_64") => Ok("linux-x86_64"),
        ("linux", "aarch64") => Ok("linux-aarch64"),
        (os, arch) => Err(format!("updates are not available for {}-{}", os, arch)),
    }
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let value = path.to_string_lossy();
    let output = if Command::new("sha256sum").arg("--version").output().is_ok() {
        run_command("sha256sum", &[&value], 20)?
    } else {
        run_command("shasum", &["-a", "256", &value], 20)?
    };
    output
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| "checksum command returned no digest".to_string())
}

fn perform_update() -> Result<String, String> {
    let Some(update) = available_update(true)? else {
        return Ok(format!("AgentDeck {} is already up to date.", VERSION));
    };
    let platform = release_platform()?;
    let asset = format!("agentdeck-{}.tar.gz", platform);
    let base = format!("{}/download/{}/{}", RELEASES_URL, update.version, asset);
    let temp_dir = env::temp_dir().join(format!("agentdeck-update-{}", std::process::id()));
    let archive = temp_dir.join(&asset);
    fs::create_dir_all(&temp_dir).map_err(|err| format!("create update directory: {}", err))?;
    let archive_path = archive.to_string_lossy().to_string();
    run_command(
        "curl",
        &["-fsSL", "--max-time", "120", "-o", &archive_path, &base],
        125,
    )?;
    let checksum = run_command(
        "curl",
        &["-fsSL", "--max-time", "30", &format!("{}.sha256", base)],
        35,
    )?;
    let expected = checksum
        .split_whitespace()
        .next()
        .ok_or_else(|| "release checksum was empty".to_string())?;
    let actual = file_sha256(&archive)?;
    if actual != expected {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err("release checksum verification failed; update aborted".to_string());
    }
    let temp_path = temp_dir.to_string_lossy().to_string();
    run_command("tar", &["-xzf", &archive_path, "-C", &temp_path], 30)?;
    let downloaded = temp_dir.join(APP_NAME);
    let current =
        env::current_exe().map_err(|err| format!("locate current executable: {}", err))?;
    if current.to_string_lossy().contains("/Cellar/") {
        return Err("this copy is managed by Homebrew; run `brew upgrade agentdeck`".to_string());
    }
    let staged = current.with_file_name(format!(".agentdeck-update-{}", std::process::id()));
    fs::copy(&downloaded, &staged).map_err(|err| {
        format!(
            "cannot write next to {}: {} (try the one-line installer)",
            current.display(),
            err
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .map_err(|err| format!("set update permissions: {}", err))?;
    }
    fs::rename(&staged, &current).map_err(|err| format!("replace current executable: {}", err))?;
    let _ = fs::remove_dir_all(&temp_dir);
    Ok(format!(
        "Updated AgentDeck {} -> {}. Restart AgentDeck to use the new version.",
        VERSION, update.version
    ))
}

fn news_cache_path() -> PathBuf {
    cache_dir().join("news.txt")
}

fn read_news_cache_if_fresh(config: &Config) -> Option<Vec<String>> {
    let path = news_cache_path();
    let modified = modified_secs(&path)?;
    let max_age = config.refresh.news.max(60);
    if now_secs().saturating_sub(modified) > max_age {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let lines = text
        .lines()
        .filter(|line| !is_news_runtime_metadata(line))
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    (!lines.is_empty()).then_some(lines)
}

fn read_news_cache_stale() -> Option<Vec<String>> {
    let text = fs::read_to_string(news_cache_path()).ok()?;
    let lines = text
        .lines()
        .filter(|line| !is_news_runtime_metadata(line))
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    (!lines.is_empty()).then_some(lines)
}

fn write_news_cache(lines: &[String]) {
    let dir = cache_dir();
    if fs::create_dir_all(&dir).is_ok() {
        let cache_lines = lines
            .iter()
            .filter(|line| !is_news_runtime_metadata(line))
            .cloned()
            .collect::<Vec<_>>();
        let _ = fs::write(news_cache_path(), cache_lines.join("\n"));
    }
}

fn is_news_runtime_metadata(line: &str) -> bool {
    line.starts_with("news refresh:") || line.starts_with("news source:")
}

fn duration_label(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        if minutes == 0 {
            format!("{}h", hours)
        } else {
            format!("{}h{}m", hours, minutes)
        }
    }
}

fn clock_from_epoch(epoch_secs: u64) -> String {
    #[cfg(target_os = "macos")]
    {
        return run_command("date", &["-r", &epoch_secs.to_string(), "+%H:%M:%S"], 2)
            .unwrap_or_else(|_| "--:--:--".to_string())
            .trim()
            .to_string();
    }
    #[cfg(target_os = "linux")]
    {
        let date_arg = format!("@{}", epoch_secs);
        return run_command("date", &["-d", &date_arg, "+%H:%M:%S"], 2)
            .unwrap_or_else(|_| "--:--:--".to_string())
            .trim()
            .to_string();
    }
    #[allow(unreachable_code)]
    "--:--:--".to_string()
}

fn append_news_refresh_metadata(
    mut lines: Vec<String>,
    config: &Config,
    source: &str,
) -> Vec<String> {
    lines.retain(|line| !is_news_runtime_metadata(line));
    let interval = config.refresh.news.max(1);
    let next_epoch = now_secs().saturating_add(interval);
    lines.push(format!(
        "news refresh: next check {} (in {}, interval {})",
        clock_from_epoch(next_epoch),
        duration_label(interval),
        duration_label(interval)
    ));
    lines.push(format!("news source: {}", source));
    lines
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn translated_or_original(lines: &[String], text: &str) -> Option<Vec<String>> {
    let translated: Vec<String> = text
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches("- ")
                .trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '.' || ch == ')')
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect();
    (translated.len() == lines.len()).then_some(translated)
}

fn translate_with_codex(lines: &[String], config: &Config) -> Result<Vec<String>, String> {
    let prompt = format!(
        "Translate each newline-delimited AI news headline into {}.\nKeep company names and product names intact.\nReturn only the translated lines, exactly one line per input line, with no numbering, no bullets, and no explanation.\n\n{}",
        config.news.translate_to,
        lines.join("\n")
    );
    let output_file = env::temp_dir().join(format!(
        "agentdeck-codex-translate-{}-{}.txt",
        std::process::id(),
        now_clock().replace(':', "")
    ));
    let output_path = output_file.to_string_lossy().to_string();
    let mut args = vec![
        "exec".to_string(),
        "--skip-git-repo-check".to_string(),
        "--ephemeral".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "--output-last-message".to_string(),
        output_path.clone(),
        "-".to_string(),
    ];
    if !config.translation.model.trim().is_empty() {
        args.splice(
            1..1,
            [
                "-m".to_string(),
                config.translation.model.trim().to_string(),
            ],
        );
    }
    let _ = run_command_with_stdin("codex", &args, &prompt, 90)?;
    let text = fs::read_to_string(&output_file).map_err(|err| format!("codex output: {}", err))?;
    let _ = fs::remove_file(&output_file);
    translated_or_original(lines, &text)
        .ok_or_else(|| "codex returned an unexpected line count".to_string())
}

fn translate_with_openai(lines: &[String], config: &Config) -> Result<Vec<String>, String> {
    let api_key =
        env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY is not set".to_string())?;
    let model = if config.translation.model.trim().is_empty() {
        "gpt-4o-mini"
    } else {
        config.translation.model.trim()
    };
    let prompt = format!(
        "Translate each newline-delimited AI news headline into {}. Keep company names and product names intact. Return exactly the same number of lines.\n\n{}",
        config.news.translate_to,
        lines.join("\n")
    );
    let payload = format!(
        r#"{{"model":"{}","input":[{{"role":"user","content":"{}"}}]}}"#,
        json_escape(model),
        json_escape(&prompt)
    );
    let json = run_command(
        "curl",
        &[
            "-fsSL",
            "--max-time",
            "30",
            "-H",
            &format!("Authorization: Bearer {}", api_key),
            "-H",
            "Content-Type: application/json",
            "-d",
            &payload,
            "https://api.openai.com/v1/responses",
        ],
        35,
    )?;
    let mut text = String::new();
    let mut search = json.as_str();
    while let Some(pos) = search.find("\"text\"") {
        search = &search[pos + 6..];
        if let Some(value) = json_string(search, "") {
            text.push_str(&value);
        } else {
            break;
        }
        if let Some(next) = search.find("\"type\"") {
            search = &search[next..];
        } else {
            break;
        }
    }
    if text.is_empty() {
        if let Some(pos) = json.find("\"output_text\"") {
            if let Some(value) = json_string(&json[pos..], "") {
                text = value;
            }
        }
    }
    translated_or_original(lines, &text)
        .ok_or_else(|| "OpenAI returned an unexpected line count".to_string())
}

fn translate_lines(lines: &[String], config: &Config) -> (Vec<String>, Option<String>) {
    if lines.is_empty() || config.translation.provider == "none" {
        return (lines.to_vec(), None);
    }
    let provider = config.translation.provider.as_str();
    let result = match provider {
        "codex" | "codex_acp" => translate_with_codex(lines, config),
        "openai" => translate_with_openai(lines, config),
        other => Err(format!("unknown provider {}", other)),
    };
    match result {
        Ok(translated) => (translated, Some(format!("translation: {}", provider))),
        Err(err) => (
            lines.to_vec(),
            Some(format!("translation: {} unavailable ({})", provider, err)),
        ),
    }
}

fn collect_news(config: &Config) -> Result<Vec<String>, String> {
    if let Some(lines) = read_news_cache_if_fresh(config) {
        return Ok(append_news_refresh_metadata(lines, config, "cache hit"));
    }

    let mut items = Vec::<(String, String, String, String)>::new();
    let mut errors = Vec::<String>::new();
    for url in &config.news.rss_urls {
        let rss = match http_get(url, 25) {
            Ok(value) => value,
            Err(err) => {
                errors.push(format!("{}: {}", url, err));
                continue;
            }
        };
        let mut rest = rss.as_str();
        while let Some(start) = rest.find("<item") {
            rest = &rest[start..];
            let Some(open_end) = rest.find('>') else {
                break;
            };
            let Some(end) = rest.find("</item>") else {
                break;
            };
            let item = &rest[open_end + 1..end];
            let title = tag(item, "title").unwrap_or_else(|| "(untitled)".to_string());
            let link = tag(item, "link").unwrap_or_default();
            let source = tag(item, "source").unwrap_or_else(|| source_from_link(&link));
            let date = tag(item, "pubDate").unwrap_or_else(|| "unknown date".to_string());
            items.push((title, source, date, link));
            rest = &rest[end + 7..];
        }
    }
    if items.is_empty() && !errors.is_empty() {
        if let Some(mut cached) = read_news_cache_stale() {
            cached.push(format!(
                "news cache: stale; refresh failed ({})",
                errors.join("; ")
            ));
            return Ok(append_news_refresh_metadata(
                cached,
                config,
                "stale cache; refresh failed",
            ));
        }
        return Err(errors.join("; "));
    }
    items.truncate(config.news.limit);
    let titles = items.iter().map(|item| item.0.clone()).collect::<Vec<_>>();
    let (translated, note) = translate_lines(&titles, config);
    let mut lines = Vec::new();
    for ((_, source, date, link), title) in items.into_iter().zip(translated.into_iter()) {
        lines.push(title);
        lines.push(format!("  {} | {}", source, date));
        if !link.is_empty() {
            lines.push(format!("{}{}", NEWS_LINK_PREFIX, link));
        }
    }
    if let Some(note) = note {
        lines.push(note);
    }
    let lines = if lines.is_empty() {
        vec!["No news loaded.".to_string()]
    } else {
        lines
    };
    write_news_cache(&lines);
    Ok(append_news_refresh_metadata(lines, config, "fetched"))
}

fn json_value_after<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)?;
    let colon = json[start..].find(':')? + start + 1;
    Some(json[colon..].trim_start())
}

fn json_first_number(json: &str, key: &str) -> Option<f64> {
    json_number(json, key)
}

fn json_number_array(json: &str, key: &str) -> Vec<f64> {
    let Some(rest) = json_value_after(json, key) else {
        return Vec::new();
    };
    let Some(open) = rest.find('[') else {
        return Vec::new();
    };
    let Some(close) = rest[open..].find(']') else {
        return Vec::new();
    };
    rest[open + 1..open + close]
        .split(',')
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect()
}

fn json_string_array_direct(json: &str, key: &str) -> Vec<String> {
    json_string_array(json, key).unwrap_or_default()
}

fn collect_weather(config: &Config) -> Result<Vec<String>, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,apparent_temperature,precipitation,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max&timezone={}&forecast_days=4",
        config.weather.latitude, config.weather.longitude, config.weather.timezone
    );
    let json = http_get(&url, 12)?;
    let current = section(&json, "current").unwrap_or("");
    let daily = section(&json, "daily").unwrap_or("");
    let temp = json_first_number(current, "temperature_2m").unwrap_or(0.0);
    let feels = json_first_number(current, "apparent_temperature").unwrap_or(0.0);
    let humidity = json_first_number(current, "relative_humidity_2m").unwrap_or(0.0);
    let wind = json_first_number(current, "wind_speed_10m").unwrap_or(0.0);
    let days = json_string_array_direct(daily, "time");
    let highs = json_number_array(daily, "temperature_2m_max");
    let lows = json_number_array(daily, "temperature_2m_min");
    let rain = json_number_array(daily, "precipitation_probability_max");
    let mut lines = vec![
        config.weather.location.clone(),
        format!(
            "Now: {:.1}C, feels {:.1}C, humidity {:.0}%, wind {:.1} km/h",
            temp, feels, humidity, wind
        ),
    ];
    for i in 0..days.len().min(4) {
        lines.push(format!(
            "{}: {:.1}-{:.1}C, rain {:.0}%",
            days.get(i).cloned().unwrap_or_default(),
            lows.get(i).copied().unwrap_or(0.0),
            highs.get(i).copied().unwrap_or(0.0),
            rain.get(i).copied().unwrap_or(0.0)
        ));
    }
    Ok(lines)
}

fn unfold_ics(text: &str) -> Vec<String> {
    let mut lines = Vec::<String>::new();
    for raw in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if raw.starts_with(' ') || raw.starts_with('\t') {
            if let Some(last) = lines.last_mut() {
                last.push_str(&raw[1..]);
            }
        } else {
            lines.push(raw.to_string());
        }
    }
    lines
}

fn parse_ics_time(value: &str) -> Option<i64> {
    let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    let year: i32 = digits.get(0..4)?.parse().ok()?;
    let month: i32 = digits.get(4..6)?.parse().ok()?;
    let day: i32 = digits.get(6..8)?.parse().ok()?;
    let hour: i32 = digits.get(8..10).unwrap_or("00").parse().unwrap_or(0);
    let minute: i32 = digits.get(10..12).unwrap_or("00").parse().unwrap_or(0);
    Some(rough_epoch_seconds(year, month, day, hour, minute))
}

fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as i64
}

fn rough_epoch_seconds(year: i32, month: i32, day: i32, hour: i32, minute: i32) -> i64 {
    days_from_civil(year, month, day) * 86_400 + (hour as i64) * 3600 + (minute as i64) * 60
}

fn format_event_time(epoch: i64) -> String {
    let now = Command::new("date")
        .args(["-r", &epoch.to_string(), "+%m/%d %H:%M"])
        .output();
    if let Ok(output) = now {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    epoch.to_string()
}

fn collect_calendar(config: &Config) -> Result<Vec<String>, String> {
    let mut text = String::new();
    for url in &config.calendar.ics_urls {
        text.push_str(&http_get(url, 12)?);
        text.push('\n');
    }
    for path in &config.calendar.ics_files {
        text.push_str(&fs::read_to_string(path).map_err(|err| format!("{}: {}", path, err))?);
        text.push('\n');
    }
    if text.trim().is_empty() {
        return Ok(vec![
            "No calendar configured.".to_string(),
            "Add Google/Outlook/private ICS URLs or files in config.json.".to_string(),
        ]);
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let end = now + config.calendar.lookahead_days * 86_400;
    let mut events = Vec::<(i64, String, String)>::new();
    let mut inside = false;
    let mut summary = String::new();
    let mut location = String::new();
    let mut start = None;
    for line in unfold_ics(&text) {
        if line == "BEGIN:VEVENT" {
            inside = true;
            summary.clear();
            location.clear();
            start = None;
        } else if line == "END:VEVENT" {
            if let Some(ts) = start {
                if ts >= now && ts <= end {
                    events.push((ts, summary.clone(), location.clone()));
                }
            }
            inside = false;
        } else if inside {
            if let Some(value) = line.strip_prefix("SUMMARY:") {
                summary = value.replace("\\,", ",");
            } else if let Some(value) = line.strip_prefix("LOCATION:") {
                location = value.replace("\\,", ",");
            } else if line.starts_with("DTSTART") {
                if let Some((_, value)) = line.split_once(':') {
                    start = parse_ics_time(value);
                }
            }
        }
    }
    events.sort_by_key(|event| event.0);
    if events.is_empty() {
        return Ok(vec!["No upcoming calendar events.".to_string()]);
    }
    Ok(events
        .into_iter()
        .take(10)
        .map(|(ts, title, loc)| {
            if loc.is_empty() {
                format!("{} {}", format_event_time(ts), title)
            } else {
                format!("{} {} @ {}", format_event_time(ts), title, loc)
            }
        })
        .collect())
}

#[derive(Clone)]
struct ProcessRow {
    pid: String,
    command: String,
    cpu: f64,
    mem: f64,
}

fn list_processes() -> Vec<ProcessRow> {
    let Ok(output) = run_command_output("/bin/ps", &["-axo", "pid,comm,%cpu,%mem"]) else {
        return Vec::new();
    };
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            Some(ProcessRow {
                pid: parts[0].to_string(),
                command: parts[1].to_string(),
                cpu: parts[2].parse().unwrap_or(0.0),
                mem: parts[3].parse().unwrap_or(0.0),
            })
        })
        .collect()
}

fn collect_status_page(label: &str, url: &str) -> Vec<String> {
    let Ok(json) = http_get(url, 8) else {
        return vec![format!("{}: status unavailable", label)];
    };
    let desc = if let Some(pos) = json.find("\"status\":{") {
        let status = section(&json[pos..], "status").unwrap_or("");
        json_string(status, "description").unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    };
    let mut lines = vec![format!("{}: {}", label, desc)];
    for chunk in json.split('{') {
        let name = json_string(chunk, "name").unwrap_or_default();
        if name.to_lowercase().contains("codex")
            || name.to_lowercase().contains("agent")
            || name.to_lowercase().contains("api")
            || name.to_lowercase().contains("claude")
        {
            let status = json_string(chunk, "status").unwrap_or_else(|| "unknown".to_string());
            lines.push(format!("  {}: {}", name, status));
        }
        if lines.len() >= 8 {
            break;
        }
    }
    lines
}

fn home_file(path: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(path)
}

fn modified_secs(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn age_label(modified: u64) -> String {
    let age = now_secs().saturating_sub(modified);
    if age < 60 {
        format!("{}s ago", age)
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else if age < 86_400 {
        format!("{}h ago", age / 3600)
    } else {
        format!("{}d ago", age / 86_400)
    }
}

fn session_state_from_age(modified: u64) -> &'static str {
    let age = now_secs().saturating_sub(modified);
    if age <= 180 {
        "working"
    } else if age <= 900 {
        "recent"
    } else {
        "idle"
    }
}

fn compact_num(value: f64) -> String {
    if value.abs() >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if value.abs() >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        format!("{:.0}", value)
    }
}

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn newest_file(root: &Path, suffix: &str) -> Option<(PathBuf, u64)> {
    let mut files = Vec::new();
    collect_files(root, &mut files);
    files
        .into_iter()
        .filter(|path| path.to_string_lossy().ends_with(suffix))
        .filter_map(|path| modified_secs(&path).map(|mtime| (path, mtime)))
        .max_by_key(|(_, mtime)| *mtime)
}

fn codex_session_usage(path: &Path) -> Option<(u64, u64, f64, f64, f64, String)> {
    let text = fs::read_to_string(path).ok()?;
    let mut session_id = String::from("unknown");
    let mut total_tokens = 0u64;
    let mut context_window = 0u64;
    let mut primary = 0.0;
    let mut secondary = 0.0;
    for line in text.lines() {
        if line.contains("\"session_meta\"") {
            if let Some(id) = json_string(line, "session_id").or_else(|| json_string(line, "id")) {
                session_id = id.chars().take(8).collect();
            }
        }
        if line.contains("\"token_count\"") {
            total_tokens = json_number(line, "total_tokens").unwrap_or(total_tokens as f64) as u64;
            context_window =
                json_number(line, "model_context_window").unwrap_or(context_window as f64) as u64;
            let percents = line
                .match_indices("\"used_percent\"")
                .filter_map(|(index, _)| json_number(&line[index..], "used_percent"))
                .collect::<Vec<_>>();
            primary = percents.first().copied().unwrap_or(primary);
            secondary = percents.get(1).copied().unwrap_or(secondary);
        }
    }
    Some((
        total_tokens,
        context_window,
        primary,
        secondary,
        fs::metadata(path).ok()?.len() as f64,
        session_id,
    ))
}

fn codex_usage_lines() -> Vec<String> {
    let root = home_file(".codex/sessions");
    let Some((latest, mtime)) = newest_file(&root, ".jsonl") else {
        return vec!["codex usage: no session files found".to_string()];
    };

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let since = now_secs().saturating_sub(86_400);
    let mut recent_sessions = 0usize;
    let mut recent_tokens = 0u64;
    for path in files
        .iter()
        .filter(|path| path.to_string_lossy().ends_with(".jsonl"))
    {
        let Some(modified) = modified_secs(path) else {
            continue;
        };
        if modified >= since {
            if let Some((tokens, _, _, _, _, _)) = codex_session_usage(path) {
                recent_sessions += 1;
                recent_tokens = recent_tokens.saturating_add(tokens);
            }
        }
    }

    let Some((tokens, context, primary, secondary, _, session)) = codex_session_usage(&latest)
    else {
        return vec!["codex usage: unable to parse latest session".to_string()];
    };
    vec![
        format!(
            "codex session: {} updated {}, {} tok / {} ctx",
            session,
            age_label(mtime),
            compact_num(tokens as f64),
            compact_num(context as f64)
        ),
        format!(
            "codex limits: {}% 5h, {}% weekly",
            primary.round(),
            secondary.round()
        ),
        format!(
            "codex 24h: {} sessions, {} tok",
            recent_sessions,
            compact_num(recent_tokens as f64)
        ),
    ]
}

fn codex_recent_session_lines(limit: usize) -> Vec<String> {
    let root = home_file(".codex/sessions");
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let mut sessions = files
        .into_iter()
        .filter(|path| path.to_string_lossy().ends_with(".jsonl"))
        .filter_map(|path| modified_secs(&path).map(|mtime| (path, mtime)))
        .collect::<Vec<_>>();
    sessions.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));

    sessions
        .into_iter()
        .take(limit)
        .filter_map(|(path, mtime)| {
            let (tokens, context, _, _, _, session) = codex_session_usage(&path)?;
            Some(format!(
                "agent session: codex id={} state={} age={} mtime={} tok={} ctx={}",
                session,
                session_state_from_age(mtime),
                age_label(mtime).replace(' ', ""),
                mtime,
                compact_num(tokens as f64),
                compact_num(context as f64)
            ))
        })
        .collect()
}

fn claude_cum_usage(path: &Path) -> Option<(u64, u64, u64, f64)> {
    let text = fs::read_to_string(path).ok()?;
    let tok = section(&text, "tok")
        .and_then(|value| json_number(value, "total"))
        .unwrap_or(0.0) as u64;
    let tok_in = section(&text, "tokIn")
        .and_then(|value| json_number(value, "total"))
        .unwrap_or(0.0) as u64;
    let tok_out = section(&text, "tokOut")
        .and_then(|value| json_number(value, "total"))
        .unwrap_or(0.0) as u64;
    let cost = section(&text, "cost")
        .and_then(|value| json_number(value, "total"))
        .unwrap_or(0.0);
    Some((tok, tok_in, tok_out, cost))
}

fn claude_recent_session_lines(limit: usize) -> Vec<String> {
    let root = home_file(".claude");
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let mut sessions = files
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("cc-cum-") && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .filter_map(|path| modified_secs(&path).map(|mtime| (path, mtime)))
        .collect::<Vec<_>>();
    sessions.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));

    sessions
        .into_iter()
        .take(limit)
        .filter_map(|(path, mtime)| {
            let (tok, tok_in, tok_out, cost) = claude_cum_usage(&path)?;
            let session = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("cc-cum")
                .trim_start_matches("cc-cum-")
                .chars()
                .take(8)
                .collect::<String>();
            Some(format!(
                "agent session: claude id={} state={} age={} mtime={} tok={} in={} out={} cost=${:.2}",
                session,
                session_state_from_age(mtime),
                age_label(mtime).replace(' ', ""),
                mtime,
                compact_num(tok as f64),
                compact_num(tok_in as f64),
                compact_num(tok_out as f64),
                cost
            ))
        })
        .collect()
}

fn claude_stats_latest_line() -> Option<String> {
    let text = fs::read_to_string(home_file(".claude/stats-cache.json")).ok()?;
    let mut latest_date = String::new();
    let mut latest_msg = 0u64;
    let mut latest_sessions = 0u64;
    let mut latest_tools = 0u64;
    for chunk in text.split('{') {
        let Some(date) = json_string(chunk, "date") else {
            continue;
        };
        latest_date = date;
        latest_msg = json_number(chunk, "messageCount").unwrap_or(0.0) as u64;
        latest_sessions = json_number(chunk, "sessionCount").unwrap_or(0.0) as u64;
        latest_tools = json_number(chunk, "toolCallCount").unwrap_or(0.0) as u64;
    }
    (!latest_date.is_empty()).then_some(format!(
        "claude stats: {} {} msgs, {} tools, {} sessions",
        latest_date, latest_msg, latest_tools, latest_sessions
    ))
}

fn claude_usage_lines() -> Vec<String> {
    let root = home_file(".claude");
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let latest_cum = files
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("cc-cum-") && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .filter_map(|path| modified_secs(path).map(|mtime| (path.clone(), mtime)))
        .max_by_key(|(_, mtime)| *mtime);
    let Some((latest_cum, latest_mtime)) = latest_cum else {
        return vec!["claude usage: no usage files found".to_string()];
    };

    let since = now_secs().saturating_sub(86_400);
    let mut recent_sessions = 0usize;
    let mut recent_tokens = 0u64;
    let mut recent_cost = 0.0;
    for path in files.iter().filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("cc-cum-") && name.ends_with(".json"))
            .unwrap_or(false)
    }) {
        let Some(modified) = modified_secs(path) else {
            continue;
        };
        if modified >= since {
            if let Some((tok, _, _, cost)) = claude_cum_usage(path) {
                recent_sessions += 1;
                recent_tokens = recent_tokens.saturating_add(tok);
                recent_cost += cost;
            }
        }
    }

    let mut lines = Vec::new();
    if let Some((tok, tok_in, tok_out, cost)) = claude_cum_usage(&latest_cum) {
        let session = latest_cum
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("cc-cum")
            .trim_start_matches("cc-cum-")
            .chars()
            .take(8)
            .collect::<String>();
        lines.push(format!(
            "claude session: {} updated {}, {} tok (${:.2})",
            session,
            age_label(latest_mtime),
            compact_num(tok as f64),
            cost
        ));
        lines.push(format!(
            "claude latest: in {}, out {} tok",
            compact_num(tok_in as f64),
            compact_num(tok_out as f64)
        ));
    }
    lines.push(format!(
        "claude 24h: {} sessions, {} tok, ${:.2}",
        recent_sessions,
        compact_num(recent_tokens as f64),
        recent_cost
    ));
    if let Some(stats) = claude_stats_latest_line() {
        lines.push(stats);
    }
    lines
}

fn claude_limits_from_usage_json(json: &str) -> AgentLimits {
    let utilization = |period: &str| {
        section(json, period)
            .and_then(|value| json_number(value, "utilization"))
            .map(|value| value.round().clamp(0.0, 100.0) as u64)
    };
    AgentLimits {
        session_used: utilization("five_hour"),
        weekly_used: utilization("seven_day"),
    }
}

fn claude_oauth_access_token() -> Option<String> {
    if let Ok(token) = env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !token.trim().is_empty() {
            return Some(token);
        }
    }

    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let credentials = String::from_utf8(output.stdout).ok()?;
    let oauth = section(&credentials, "claudeAiOauth")?;
    json_string(oauth, "accessToken").filter(|token| !token.is_empty())
}

fn bearer_json(url: &str, token: &str, timeout_secs: u64) -> Option<String> {
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--fail",
            "--max-time",
            &timeout_secs.to_string(),
            "--header",
            "@-",
            "--header",
            "Content-Type: application/json",
            url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child
        .stdin
        .take()?
        .write_all(format!("Authorization: Bearer {}\n", token).as_bytes())
        .ok()?;
    let output = child.wait_with_output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn collect_claude_limits() -> Vec<String> {
    let Some(token) = claude_oauth_access_token() else {
        return Vec::new();
    };
    let Some(json) = bearer_json("https://api.anthropic.com/api/oauth/usage", &token, 8) else {
        return Vec::new();
    };
    let limits = claude_limits_from_usage_json(&json);
    if limits == AgentLimits::default() {
        return Vec::new();
    }
    let session = limits
        .session_used
        .map(|value| format!("{}% 5h", value))
        .unwrap_or_else(|| "5h unavailable".to_string());
    let weekly = limits
        .weekly_used
        .map(|value| format!("{}% weekly", value))
        .unwrap_or_else(|| "weekly unavailable".to_string());
    vec![format!("claude limits: {}, {}", session, weekly)]
}

fn collect_local_agents(config: &Config) -> Result<Vec<String>, String> {
    let processes = list_processes();
    let mut lines = Vec::new();
    for (label, keywords) in [
        ("codex", &config.agents.codex_keywords),
        ("claude", &config.agents.claude_keywords),
    ] {
        let matches: Vec<&ProcessRow> = processes
            .iter()
            .filter(|proc| {
                keywords
                    .iter()
                    .any(|needle| proc.command.to_lowercase().contains(&needle.to_lowercase()))
            })
            .collect();
        if matches.is_empty() {
            lines.push(format!("{}: no local process found", label));
        } else {
            let cpu: f64 = matches.iter().map(|proc| proc.cpu).sum();
            let mem: f64 = matches.iter().map(|proc| proc.mem).sum();
            let pids = matches
                .iter()
                .take(5)
                .map(|proc| proc.pid.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "{}: running ({} proc, cpu {:.1}%, mem {:.1}%, pid {})",
                label,
                matches.len(),
                cpu,
                mem,
                pids
            ));
        }
    }
    lines.extend(codex_usage_lines());
    lines.extend(claude_usage_lines());
    lines.push("agent sessions:".to_string());
    let codex_sessions = codex_recent_session_lines(4);
    let claude_sessions = claude_recent_session_lines(4);
    let session_count = codex_sessions.len().max(claude_sessions.len());
    for index in 0..session_count {
        if let Some(line) = codex_sessions.get(index) {
            lines.push(line.clone());
        }
        if let Some(line) = claude_sessions.get(index) {
            lines.push(line.clone());
        }
    }
    Ok(lines)
}

fn collect_agent_service_status(config: &Config) -> Vec<String> {
    let mut lines = collect_status_page("openai", &config.agents.openai_status_url);
    lines.extend(collect_status_page(
        "anthropic",
        &config.agents.anthropic_status_url,
    ));
    lines
}

fn collect_agents(config: &Config) -> Result<Vec<String>, String> {
    let mut lines = collect_local_agents(config)?;
    lines.extend(collect_claude_limits());
    lines.extend(collect_agent_service_status(config));
    Ok(lines)
}

fn human_bytes(value: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut num = value;
    for unit in units {
        if num < 1024.0 || unit == "TB" {
            if unit == "B" {
                return format!("{:.0}{}", num, unit);
            }
            return format!("{:.1}{}", num, unit);
        }
        num /= 1024.0;
    }
    format!("{:.1}TB", num)
}

fn memory_line() -> Option<String> {
    if cfg!(target_os = "macos") {
        let total: f64 = run_command("sysctl", &["-n", "hw.memsize"], 5)
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let pagesize: f64 = run_command("pagesize", &[], 5).ok()?.trim().parse().ok()?;
        let vm = run_command("vm_stat", &[], 5).ok()?;
        let mut free_pages = 0.0;
        for line in vm.lines() {
            if line.starts_with("Pages free:")
                || line.starts_with("Pages inactive:")
                || line.starts_with("Pages speculative:")
            {
                if let Some(num) = line
                    .chars()
                    .filter(|ch| ch.is_ascii_digit())
                    .collect::<String>()
                    .parse::<f64>()
                    .ok()
                {
                    free_pages += num;
                }
            }
        }
        let used = (total - free_pages * pagesize).max(0.0);
        Some(format!(
            "Memory: {} / {} ({:.0}%)",
            human_bytes(used),
            human_bytes(total),
            used / total * 100.0
        ))
    } else {
        let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
        let mut total = 0.0;
        let mut available = 0.0;
        for line in meminfo.lines() {
            if line.starts_with("MemTotal:") {
                total = line.split_whitespace().nth(1)?.parse::<f64>().ok()? * 1024.0;
            } else if line.starts_with("MemAvailable:") {
                available = line.split_whitespace().nth(1)?.parse::<f64>().ok()? * 1024.0;
            }
        }
        let used = total - available;
        Some(format!(
            "Memory: {} / {} ({:.0}%)",
            human_bytes(used),
            human_bytes(total),
            used / total * 100.0
        ))
    }
}

fn first_percent_value(text: &str) -> Option<f64> {
    let end = text.find('%')?;
    text[..end]
        .split_whitespace()
        .last()
        .and_then(|value| value.parse::<f64>().ok())
}

fn refined_process_command(proc: &ProcessRow) -> String {
    let fallback = proc.command.trim();
    run_command_output("/bin/ps", &["-p", &proc.pid, "-ww", "-o", "args="])
        .ok()
        .map(|output| output.trim().to_string())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn cpu_usage_percent(processes: &[ProcessRow]) -> f64 {
    if cfg!(target_os = "macos") {
        if let Ok(top) = run_command("top", &["-l", "1", "-n", "0"], 5) {
            for line in top.lines() {
                if !line.contains("CPU usage:") {
                    continue;
                }
                for part in line.split(',') {
                    if part.contains("idle") {
                        if let Some(idle) = first_percent_value(part) {
                            return (100.0 - idle).clamp(0.0, 100.0);
                        }
                    }
                }
            }
        }
    }

    let total_cpu: f64 = processes.iter().map(|proc| proc.cpu).sum();
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as f64;
    (total_cpu / cores).clamp(0.0, 100.0)
}

fn collect_system(config: &Config) -> Result<Vec<String>, String> {
    let load = run_command("uptime", &[], 5).unwrap_or_else(|_| "load unavailable".to_string());
    let disk_path = if cfg!(target_os = "macos") {
        "/System/Volumes/Data"
    } else {
        "/"
    };
    let disk = run_command("df", &["-k", disk_path], 5).unwrap_or_default();
    let processes = list_processes();
    let total_cpu: f64 = processes.iter().map(|proc| proc.cpu).sum();
    let cpu_pressure = cpu_usage_percent(&processes);
    let mut lines = vec![
        load.trim().to_string(),
        format!("CPU pressure: {:.0}%", cpu_pressure),
        format!("Process CPU sum: {:.1}%", total_cpu),
    ];
    if let Some(mem) = memory_line() {
        lines.push(mem);
    }
    if let Some(row) = disk.lines().nth(1) {
        let parts: Vec<&str> = row.split_whitespace().collect();
        if parts.len() >= 5 {
            let total = parts[1].parse::<f64>().unwrap_or(0.0) * 1024.0;
            let used = parts[2].parse::<f64>().unwrap_or(0.0) * 1024.0;
            let available = parts[3].parse::<f64>().unwrap_or(0.0) * 1024.0;
            lines.push(format!(
                "Disk {}: {} free / {} ({} used, {})",
                disk_path,
                human_bytes(available),
                human_bytes(total),
                human_bytes(used),
                parts[4]
            ));
        }
    }
    lines.push("Top processes:".to_string());
    let mut sorted = processes;
    let own_pid = std::process::id().to_string();
    sorted.retain(|proc| {
        let label = basename_label(&proc.command, 64).to_lowercase();
        let command = proc.command.to_lowercase();
        proc.pid != own_pid
            && label != "top"
            && label != "ps"
            && label != "tui"
            && label != APP_NAME
            && !label.starts_with("agentdeck")
            && !command.contains("/agentdeck")
    });
    sorted.sort_by(|a, b| {
        b.cpu
            .partial_cmp(&a.cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for proc in sorted.into_iter().take(config.top_processes) {
        let command = refined_process_command(&proc);
        let label = process_display_name(&command, 18);
        lines.push(format!(
            "  {:>6} {:<18} cpu {:>5.1}% mem {:>4.1}% cmd {}",
            proc.pid,
            label,
            proc.cpu,
            proc.mem,
            truncate_chars(&command, 96)
        ));
    }
    Ok(lines)
}

#[derive(Clone, Debug)]
struct DockerContainer {
    group: String,
    service: String,
    name: String,
    image: String,
    status: String,
    ports: String,
    state: String,
}

#[derive(Clone, Debug)]
struct DockerGroupMetric {
    name: String,
    total: usize,
    running: usize,
}

fn docker_state(status: &str) -> &'static str {
    let lower = status.to_lowercase();
    if lower.contains("unhealthy") || lower.contains("exited") || lower.contains("dead") {
        "stopped"
    } else if lower.starts_with("up") && lower.contains("healthy") {
        "healthy"
    } else if lower.starts_with("up") {
        "running"
    } else if lower.contains("created") || lower.contains("restarting") || lower.contains("paused")
    {
        "warning"
    } else {
        "unknown"
    }
}

fn infer_docker_group(name: &str, project: &str) -> String {
    let project = project.trim();
    if !project.is_empty() && project != "<no value>" {
        return project.to_string();
    }
    if name.starts_with("k8s_") {
        return "Kubernetes".to_string();
    }
    let separator = if name.contains('-') { '-' } else { '_' };
    name.split(separator)
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or("Standalone")
        .to_string()
}

fn infer_docker_service(name: &str, project: &str, service: &str) -> String {
    let service = service.trim();
    if !service.is_empty() && service != "<no value>" {
        return service.to_string();
    }
    if name.starts_with("k8s_") {
        return name
            .split('_')
            .nth(1)
            .filter(|part| !part.is_empty())
            .unwrap_or(name)
            .to_string();
    }
    let project = project.trim();
    let stripped = if !project.is_empty() && project != "<no value>" {
        name.trim_start_matches(project)
            .trim_start_matches(['-', '_'])
            .to_string()
    } else {
        let group = infer_docker_group(name, "");
        name.trim_start_matches(&group)
            .trim_start_matches(['-', '_'])
            .to_string()
    };
    let without_replica = stripped
        .rsplit_once(['-', '_'])
        .and_then(|(prefix, suffix)| {
            suffix
                .chars()
                .all(|ch| ch.is_ascii_digit())
                .then_some(prefix)
        })
        .unwrap_or(&stripped);
    without_replica
        .split(['-', '_'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(name)
        .to_string()
}

fn format_docker_lines(mut containers: Vec<DockerContainer>) -> Vec<String> {
    if containers.is_empty() {
        return vec!["docker summary\t0\t0\t0".to_string()];
    }
    containers.sort_by(|a, b| {
        a.group
            .cmp(&b.group)
            .then_with(|| a.service.cmp(&b.service))
            .then_with(|| a.name.cmp(&b.name))
    });

    let total = containers.len();
    let running = containers
        .iter()
        .filter(|container| matches!(container.state.as_str(), "running" | "healthy"))
        .count();
    let mut groups = BTreeMap::<String, Vec<DockerContainer>>::new();
    for container in containers {
        groups
            .entry(container.group.clone())
            .or_default()
            .push(container);
    }

    let mut lines = vec![format!(
        "docker summary\t{}\t{}\t{}",
        total,
        running,
        groups.len()
    )];
    for (group, containers) in groups {
        let group_running = containers
            .iter()
            .filter(|container| matches!(container.state.as_str(), "running" | "healthy"))
            .count();
        lines.push(format!(
            "docker group\t{}\t{}\t{}",
            group,
            containers.len(),
            group_running
        ));
        for container in containers {
            lines.push(format!(
                "docker container\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                container.group,
                container.service,
                container.image,
                container.status,
                container.ports,
                container.state,
                container.name
            ));
        }
    }
    lines
}

fn collect_docker(config: &Config) -> Result<Vec<String>, String> {
    let labeled_format = "{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}\t{{.Label \"com.docker.compose.project\"}}\t{{.Label \"com.docker.compose.service\"}}";
    let plain_format = "{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}";
    let output = run_command("docker", &["ps", "-a", "--format", labeled_format], 6)
        .or_else(|_| run_command("docker", &["ps", "-a", "--format", plain_format], 6))?;
    let mut containers = Vec::<DockerContainer>::new();
    for row in output.lines().take(config.docker_limit) {
        let parts: Vec<&str> = row.split('\t').collect();
        let name = parts.get(0).copied().unwrap_or("");
        let image = parts.get(1).copied().unwrap_or("");
        let status = parts.get(2).copied().unwrap_or("");
        let ports = parts.get(3).copied().unwrap_or("");
        let project = parts.get(4).copied().unwrap_or("");
        let service = parts.get(5).copied().unwrap_or("");
        containers.push(DockerContainer {
            group: infer_docker_group(name, project),
            service: infer_docker_service(name, project, service),
            name: name.to_string(),
            image: image.to_string(),
            status: status.to_string(),
            ports: if ports.trim().is_empty() {
                "no published ports".to_string()
            } else {
                ports.to_string()
            },
            state: docker_state(status).to_string(),
        });
    }
    Ok(format_docker_lines(containers))
}

fn collect_ports(config: &Config) -> Result<Vec<String>, String> {
    let output = run_command("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN"], 6)
        .or_else(|_| run_command("netstat", &["-an"], 6))?;
    let mut lines = Vec::new();
    if output.contains("COMMAND") {
        let mut seen = Vec::<String>::new();
        for line in output.lines().skip(1).take(config.port_limit) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 9 {
                let address_index = if parts.last() == Some(&"(LISTEN)") && parts.len() >= 2 {
                    parts.len() - 2
                } else {
                    parts.len() - 1
                };
                let address = parts[address_index];
                let port = address.rsplit(':').next().unwrap_or(address);
                let port = port.trim_matches(['[', ']']);
                let key = format!("{}:{}:{}", port, parts[0], parts[1]);
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                lines.push(format!(
                    ":{:<6} {:<16} pid {:<7} user {}",
                    port,
                    parts[0].chars().take(16).collect::<String>(),
                    parts[1],
                    parts[2]
                ));
            }
        }
    } else {
        for line in output
            .lines()
            .filter(|line| line.contains("LISTEN"))
            .take(config.port_limit)
        {
            lines.push(line.trim().to_string());
        }
    }
    Ok(if lines.is_empty() {
        vec!["No listening TCP ports found.".to_string()]
    } else {
        lines
    })
}

fn now_clock() -> String {
    run_command("date", &["+%H:%M:%S"], 2)
        .unwrap_or_else(|_| "--:--:--".to_string())
        .trim()
        .to_string()
}

fn new_panels() -> BTreeMap<&'static str, Panel> {
    BTreeMap::from([
        ("news", Panel::new("AI News")),
        ("weather", Panel::new("Weather")),
        ("calendar", Panel::new("Calendar")),
        ("agents", Panel::new("Codex / Claude")),
        ("system", Panel::new("System")),
        ("docker", Panel::new("Docker")),
        ("ports", Panel::new("Ports")),
    ])
}

impl Panel {
    fn new(title: &'static str) -> Self {
        Self {
            title,
            lines: Vec::new(),
            error: None,
            updated: None,
            loading: false,
        }
    }
}

fn update_panel<F>(panels: &SharedPanels, key: &'static str, collector: F)
where
    F: FnOnce() -> Result<Vec<String>, String>,
{
    {
        let mut guard = panels.lock().unwrap();
        if let Some(panel) = guard.get_mut(key) {
            panel.loading = true;
        }
    }
    let result = collector();
    let mut guard = panels.lock().unwrap();
    if let Some(panel) = guard.get_mut(key) {
        match result {
            Ok(lines) => {
                panel.lines = lines;
                panel.error = None;
            }
            Err(err) => {
                panel.lines.clear();
                panel.error = Some(err.lines().next().unwrap_or("unknown error").to_string());
            }
        }
        panel.updated = Some(now_clock());
        panel.loading = false;
    }
}

fn spawn_worker<F>(panels: SharedPanels, key: &'static str, interval: u64, collector: F)
where
    F: Fn() -> Result<Vec<String>, String> + Send + Sync + 'static,
{
    let collector = Arc::new(collector);
    thread::spawn(move || loop {
        let collector = collector.clone();
        update_panel(&panels, key, || collector());
        thread::sleep(Duration::from_secs(interval.max(1)));
    });
}

fn panel_block(panel: &Panel, accent: Color) -> Block<'static> {
    let stamp = panel.updated.as_deref().unwrap_or("--:--:--");
    let status = if panel.loading { " refreshing" } else { "" };
    let title = Line::from(vec![
        Span::styled(
            format!(" {} ", panel.title),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(status, Style::default().fg(Color::Yellow)),
        Span::styled(format!(" {} ", stamp), Style::default().fg(Color::DarkGray)),
    ]);

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(title)
}

fn line_style(line: &str) -> Style {
    let lower = line.to_lowercase();
    if lower.starts_with("error:") {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if lower.contains("limits:") {
        let high = percent_before_marker(line, "5h")
            .unwrap_or(0)
            .max(percent_before_marker(line, "weekly").unwrap_or(0));
        Style::default()
            .fg(percent_color(high))
            .add_modifier(Modifier::BOLD)
    } else if lower.starts_with("codex") {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if lower.starts_with("claude") {
        Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD)
    } else if lower.starts_with("news refresh:") || lower.starts_with("news source:") {
        Style::default().fg(Color::Yellow)
    } else if lower.contains(" tok") || lower.contains('$') {
        Style::default().fg(Color::LightMagenta)
    } else if lower.contains("operational") || lower.contains("running") || lower.contains("up ") {
        Style::default().fg(Color::Green)
    } else if lower.contains("degradation")
        || lower.contains("investigating")
        || lower.contains("unavailable")
        || lower.contains("no local")
    {
        Style::default().fg(Color::Yellow)
    } else if line.starts_with("  ") {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn hidden_news_link(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(link) = trimmed.strip_prefix(NEWS_LINK_PREFIX) {
        let link = link.trim();
        return (!link.is_empty()).then_some(link.to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }
    None
}

fn panel_lines(panel: &Panel) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(error) = &panel.error {
        lines.push(Line::styled(
            format!("ERROR: {}", error),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    for line in &panel.lines {
        if hidden_news_link(line).is_some() {
            continue;
        }
        lines.push(Line::styled(line.clone(), line_style(line)));
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "Waiting for data...",
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines
}

fn render_text_panel(frame: &mut Frame, area: Rect, panel: &Panel, accent: Color) {
    let paragraph = Paragraph::new(panel_lines(panel))
        .block(panel_block(panel, accent))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn line_has_news_link(lines: &[String], index: usize) -> Option<String> {
    for line in lines.iter().skip(index + 1) {
        if let Some(link) = hidden_news_link(line) {
            return Some(link);
        }
        if !line.starts_with("  ") && !line.trim().is_empty() {
            return None;
        }
    }
    None
}

fn is_news_title(line: &str) -> bool {
    !line.trim().is_empty()
        && !line.starts_with("  ")
        && !line.starts_with("translation:")
        && !line.starts_with("news cache:")
        && !line.starts_with("news refresh:")
        && !line.starts_with("news source:")
        && hidden_news_link(line).is_none()
}

fn wrap_display_line(value: &str, max_width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    let max_width = max_width.max(1);
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in value.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width > 0 && current_width.saturating_add(width) > max_width {
            wrapped.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width = current_width.saturating_add(width);
    }
    if !current.is_empty() {
        wrapped.push(current);
    }
    wrapped
}

fn render_news_panel(
    frame: &mut Frame,
    area: Rect,
    panel: &Panel,
    accent: Color,
    click_zones: &mut Vec<ClickZone>,
) {
    let block = panel_block(panel, accent);
    let inner = block.inner(area);
    let mut lines = Vec::new();
    let mut visible_row = 0u16;
    let line_width = inner.width.max(1) as usize;

    if let Some(error) = &panel.error {
        for part in wrap_display_line(&format!("ERROR: {}", error), line_width) {
            lines.push(Line::styled(
                part,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
            visible_row = visible_row.saturating_add(1);
        }
    }

    for (index, line) in panel.lines.iter().enumerate() {
        if hidden_news_link(line).is_some() {
            continue;
        }

        if is_news_title(line) {
            if let Some(link) = line_has_news_link(&panel.lines, index) {
                let wrapped = wrap_display_line(&format!("{}  [open →]", line), line_width);
                if visible_row < inner.height {
                    let available = inner.height.saturating_sub(visible_row);
                    click_zones.push(ClickZone {
                        rect: Rect::new(
                            inner.x,
                            inner.y.saturating_add(visible_row),
                            inner.width,
                            (wrapped.len() as u16).min(available),
                        ),
                        action: ClickAction::OpenUrl(link),
                    });
                }
                for part in wrapped {
                    lines.push(Line::styled(
                        part,
                        Style::default()
                            .fg(COLOR_SIGNAL)
                            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    ));
                    visible_row = visible_row.saturating_add(1);
                }
            } else {
                for part in wrap_display_line(line, line_width) {
                    lines.push(Line::styled(part, line_style(line)));
                    visible_row = visible_row.saturating_add(1);
                }
            }
        } else {
            for part in wrap_display_line(line, line_width) {
                lines.push(Line::styled(part, line_style(line)));
                visible_row = visible_row.saturating_add(1);
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::styled(
            "Waiting for data...",
            Style::default().fg(Color::DarkGray),
        ));
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn percent_from_line(line: &str) -> Option<u16> {
    let end = line.rfind('%')?;
    let prefix = &line[..end];
    let digits = prefix
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    digits.parse::<u16>().ok()
}

fn number_from_line(line: &str) -> Option<f64> {
    line.split(':')
        .nth(1)?
        .trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()
}

fn percent_before_marker(line: &str, marker: &str) -> Option<u64> {
    let marker_pos = line.find(marker)?;
    let prefix = &line[..marker_pos];
    let percent_pos = prefix.rfind('%')?;
    if !prefix[percent_pos + 1..].trim().is_empty() {
        return None;
    }
    let digits = prefix[..percent_pos]
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    digits.parse::<u64>().ok()
}

fn agent_limit_values(panel: Option<&Panel>, provider: &str) -> AgentLimits {
    let prefix = format!("{} limits:", provider);
    let Some(line) =
        panel.and_then(|panel| panel.lines.iter().find(|line| line.starts_with(&prefix)))
    else {
        return AgentLimits::default();
    };
    AgentLimits {
        session_used: percent_before_marker(line, "5h").map(|value| value.min(100)),
        weekly_used: percent_before_marker(line, "weekly").map(|value| value.min(100)),
    }
}

fn parse_compact_value(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (number, scale) = match trimmed.chars().last()? {
        'K' | 'k' => (&trimmed[..trimmed.len() - 1], 1_000.0),
        'M' | 'm' => (&trimmed[..trimmed.len() - 1], 1_000_000.0),
        'B' | 'b' => (&trimmed[..trimmed.len() - 1], 1_000_000_000.0),
        _ => (trimmed, 1.0),
    };
    Some(number.parse::<f64>().ok()? * scale)
}

fn tokens_before_marker(line: &str, marker: &str) -> Option<f64> {
    let marker_pos = line.find(marker)?;
    line[..marker_pos]
        .split_whitespace()
        .last()
        .and_then(parse_compact_value)
}

fn agent_24h_tokens(panel: Option<&Panel>, provider: &str) -> f64 {
    let prefix = format!("{} 24h:", provider);
    panel
        .and_then(|panel| panel.lines.iter().find(|line| line.starts_with(&prefix)))
        .and_then(|line| tokens_before_marker(line, " tok"))
        .unwrap_or(0.0)
}

fn agent_24h_cost(panel: Option<&Panel>, provider: &str) -> Option<f64> {
    let prefix = format!("{} 24h:", provider);
    let line = panel.and_then(|panel| panel.lines.iter().find(|line| line.starts_with(&prefix)))?;
    let dollar = line.find('$')?;
    let amount = line[dollar + 1..]
        .split([',', ' '])
        .next()
        .unwrap_or("")
        .trim();
    amount.parse::<f64>().ok()
}

#[derive(Clone)]
struct AgentSessionMetric {
    provider: String,
    id: String,
    state: String,
    age: String,
    tokens: String,
    detail: String,
}

fn key_value_token<'a>(tokens: &'a [&'a str], key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    tokens
        .iter()
        .find_map(|token| token.strip_prefix(&prefix))
        .filter(|value| !value.is_empty())
}

fn agent_session_metrics(panel: Option<&Panel>, limit: usize) -> Vec<AgentSessionMetric> {
    let Some(panel) = panel else {
        return Vec::new();
    };
    panel
        .lines
        .iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("agent session: ")?;
            let tokens = rest.split_whitespace().collect::<Vec<_>>();
            let provider = tokens.first()?.to_string();
            let id = key_value_token(&tokens, "id").unwrap_or("-").to_string();
            let modified =
                key_value_token(&tokens, "mtime").and_then(|value| value.parse::<u64>().ok());
            let state = modified
                .map(session_state_from_age)
                .or_else(|| key_value_token(&tokens, "state"))
                .unwrap_or("idle")
                .to_string();
            let age = modified
                .map(|value| age_label(value).replace(' ', ""))
                .or_else(|| key_value_token(&tokens, "age").map(str::to_string))
                .unwrap_or_else(|| "-".to_string());
            let tok = key_value_token(&tokens, "tok").unwrap_or("0").to_string();
            let detail = if provider == "claude" {
                key_value_token(&tokens, "cost").unwrap_or("").to_string()
            } else {
                key_value_token(&tokens, "ctx")
                    .map(|ctx| format!("ctx {}", ctx))
                    .unwrap_or_default()
            };
            Some(AgentSessionMetric {
                provider,
                id,
                state,
                age,
                tokens: tok,
                detail,
            })
        })
        .take(limit)
        .collect()
}

fn system_percent_values(panel: Option<&Panel>) -> (u64, u64, u64) {
    let cpu = panel
        .and_then(|panel| {
            panel
                .lines
                .iter()
                .find(|line| line.starts_with("CPU pressure:"))
        })
        .and_then(|line| number_from_line(line))
        .map(|value| value.round().clamp(0.0, 100.0) as u64)
        .unwrap_or(0);
    let memory = panel
        .and_then(|panel| panel.lines.iter().find(|line| line.starts_with("Memory:")))
        .and_then(|line| percent_from_line(line))
        .unwrap_or(0) as u64;
    let disk = panel
        .and_then(|panel| panel.lines.iter().find(|line| line.starts_with("Disk ")))
        .and_then(|line| percent_from_line(line))
        .unwrap_or(0) as u64;
    (cpu, memory, disk)
}

fn push_history(series: &mut VecDeque<u64>, value: u64) {
    if series.len() >= HISTORY_LIMIT {
        series.pop_front();
    }
    series.push_back(value.min(100));
}

fn history_data(series: &VecDeque<u64>, fallback: u64) -> Vec<u64> {
    if series.is_empty() {
        vec![fallback.min(100); 18]
    } else {
        series.iter().copied().collect()
    }
}

fn sample_history(history: &mut DashboardHistory, snapshot: &BTreeMap<&'static str, Panel>) {
    let now = Instant::now();
    if history
        .last_sample
        .is_some_and(|last| now.duration_since(last) < Duration::from_secs(1))
    {
        return;
    }
    history.last_sample = Some(now);

    let (cpu, memory, disk) = system_percent_values(snapshot.get("system"));
    push_history(&mut history.cpu, cpu);
    push_history(&mut history.memory, memory);
    push_history(&mut history.disk, disk);

    let codex = agent_limit_values(snapshot.get("agents"), "codex");
    let claude = agent_limit_values(snapshot.get("agents"), "claude");
    if let Some(value) = codex.session_used {
        push_history(&mut history.codex_5h, value);
    }
    if let Some(value) = codex.weekly_used {
        push_history(&mut history.codex_weekly, value);
    }
    if let Some(value) = claude.session_used {
        push_history(&mut history.claude_5h, value);
    }
    if let Some(value) = claude.weekly_used {
        push_history(&mut history.claude_weekly, value);
    }
}

fn percent_color(value: u64) -> Color {
    if value >= 90 {
        Color::Red
    } else if value >= 70 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn render_gauge(frame: &mut Frame, area: Rect, label: &str, value: u16, accent: Color) {
    let gauge = Gauge::default()
        .block(Block::default().title(label))
        .gauge_style(Style::default().fg(accent).bg(Color::Black))
        .label(format!("{}%", value))
        .ratio((value.min(100) as f64) / 100.0);
    frame.render_widget(gauge, area);
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return value.chars().take(max_chars).collect();
    }
    format!(
        "{}...",
        value
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>()
    )
}

fn basename_label(command: &str, max_chars: usize) -> String {
    let executable = command.split_whitespace().next().unwrap_or(command);
    let label = executable
        .split('/')
        .filter(|part| !part.is_empty())
        .next_back()
        .unwrap_or(executable);
    truncate_chars(label, max_chars)
}

fn process_display_name(command: &str, max_chars: usize) -> String {
    let executable = command.split_whitespace().next().unwrap_or(command);
    if let Some(rest) = executable.strip_prefix("/Applications/") {
        if let Some(app_name) = rest.split(".app/").next() {
            return truncate_chars(app_name, max_chars);
        }
    }
    basename_label(command, max_chars)
}

fn trend_line(data: &[u64], max_width: usize) -> String {
    const LEVELS: [char; 8] = ['_', '.', ':', '-', '=', '+', '#', '@'];
    let width = max_width.max(1);
    let start = data.len().saturating_sub(width);
    data[start..]
        .iter()
        .map(|value| {
            let index = ((*value).min(100) as usize * (LEVELS.len() - 1)) / 100;
            LEVELS[index]
        })
        .collect()
}

fn render_sparkline(frame: &mut Frame, area: Rect, title: &str, data: Vec<u64>, color: Color) {
    let latest = data.last().copied().unwrap_or(0);
    let max = data.iter().copied().max().unwrap_or(latest);
    let chart_width = area.width.saturating_sub(2).max(1) as usize;
    let lines = vec![
        Line::styled(
            trend_line(&data, chart_width),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            format!("{:>3}% now · {:>3}% peak", latest, max),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().title(title)),
        area,
    );
}

#[derive(Clone)]
struct TopProcessMetric {
    pid: String,
    label: String,
    command: String,
    cpu: f64,
    mem: f64,
}

fn top_process_metrics(panel: &Panel, limit: usize) -> Vec<TopProcessMetric> {
    panel
        .lines
        .iter()
        .skip_while(|line| !line.starts_with("Top processes:"))
        .skip(1)
        .take(limit)
        .filter_map(|line| {
            let command = line
                .split(" cmd ")
                .nth(1)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("");
            let parts = line.split_whitespace().collect::<Vec<_>>();
            let cpu = parts
                .get(3)
                .and_then(|value| value.trim_end_matches('%').parse::<f64>().ok())?;
            let mem = parts
                .get(5)
                .and_then(|value| value.trim_end_matches('%').parse::<f64>().ok())
                .unwrap_or(0.0);
            Some(TopProcessMetric {
                pid: parts.first().copied().unwrap_or("").to_string(),
                label: process_display_name(parts.get(1).copied().unwrap_or("proc"), 22),
                command: truncate_chars(command, 52),
                cpu,
                mem,
            })
        })
        .collect()
}

fn inline_bar(value: f64, max_value: f64, width: usize) -> String {
    let ratio = if max_value <= 0.0 {
        0.0
    } else {
        (value / max_value).clamp(0.0, 1.0)
    };
    let filled = (ratio * width as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn agent_state_style(state: &str, provider: &str) -> Style {
    let color = match state {
        "working" => Color::Green,
        "recent" => Color::Yellow,
        "idle" => Color::DarkGray,
        _ if provider == "codex" => Color::Cyan,
        _ => Color::LightBlue,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn usage_color(used: u64) -> Color {
    if used >= 75 {
        Color::Red
    } else {
        Color::Blue
    }
}

fn render_usage_meter(frame: &mut Frame, area: Rect, title: &str, used: Option<u64>, reset: &str) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(21), Constraint::Min(8)])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {}", reset), Style::default().fg(Color::DarkGray)),
        ])),
        columns[0],
    );

    if let Some(used) = used {
        let left = 100u64.saturating_sub(used.min(100));
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(usage_color(used)).bg(Color::Black))
            .label(format!("{}% left", left))
            .ratio((left as f64) / 100.0);
        frame.render_widget(gauge, columns[1]);
    } else {
        frame.render_widget(
            Paragraph::new("--  not reported").style(Style::default().fg(Color::DarkGray)),
            columns[1],
        );
    }
}

fn render_usage_stats(frame: &mut Frame, area: Rect, provider: &str, agents: Option<&Panel>) {
    let tokens = agent_24h_tokens(agents, provider);
    let cost = agent_24h_cost(agents, provider)
        .map(|value| format!("${:.2}", value))
        .unwrap_or_else(|| "--".to_string());
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Today",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} · {} tokens", cost, compact_num(tokens)),
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Yesterday",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("No data", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled(
                "Last 30 Days",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("No data", Style::default().fg(Color::DarkGray)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_agent_usage_card(
    frame: &mut Frame,
    area: Rect,
    provider: &str,
    agents: Option<&Panel>,
    history: &DashboardHistory,
) {
    let title = if provider == "codex" {
        "Codex Plus"
    } else {
        "Claude Code"
    };
    let accent = if provider == "codex" {
        Color::Cyan
    } else {
        Color::LightBlue
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(format!(" {} ", title));
    let inner = block.inner(area).inner(Margin {
        vertical: 0,
        horizontal: 1,
    });
    frame.render_widget(block, area);

    let limits = agent_limit_values(agents, provider);
    let chunks = split_with_gap(
        inner,
        Direction::Vertical,
        vec![
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(3),
        ],
    );
    render_usage_meter(
        frame,
        chunks[0],
        "Session",
        limits.session_used,
        "rolling 5h",
    );
    render_usage_meter(frame, chunks[1], "Weekly", limits.weekly_used, "rolling 7d");

    let extra = Paragraph::new(Line::from(vec![
        Span::styled(
            "Extra Usage",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("No data", Style::default().fg(Color::DarkGray)),
    ]));
    frame.render_widget(extra, chunks[2]);

    let trend_history = if provider == "codex" {
        &history.codex_5h
    } else {
        &history.claude_5h
    };
    if trend_history.is_empty() && limits.session_used.is_none() {
        frame.render_widget(
            Paragraph::new("No quota samples yet")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().title("Usage Trend")),
            chunks[3],
        );
    } else {
        render_sparkline(
            frame,
            chunks[3],
            "Usage Trend",
            history_data(trend_history, limits.session_used.unwrap_or(0)),
            Color::Blue,
        );
    }
    render_usage_stats(frame, chunks[4], provider, agents);
}

fn render_agent_sessions_table(
    frame: &mut Frame,
    area: Rect,
    agents: Option<&Panel>,
    limit: usize,
) {
    let sessions = agent_session_metrics(agents, limit);
    let rows = sessions.iter().map(|session| {
        Row::new(vec![
            Cell::from(session.provider.clone()),
            Cell::from(session.id.clone()),
            Cell::from(session.state.clone()),
            Cell::from(session.age.clone()),
            Cell::from(session.tokens.clone()),
            Cell::from(session.detail.clone()),
        ])
        .style(agent_state_style(&session.state, &session.provider))
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Min(8),
        ],
    )
    .header(
        Row::new(vec!["AI", "SESSION", "STATE", "AGE", "TOKENS", "DETAIL"]).style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Sessions"));

    if sessions.is_empty() {
        frame.render_widget(
            Paragraph::new("No recent agent sessions found.")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().title("Sessions")),
            area,
        );
    } else {
        frame.render_widget(table, area);
    }
}

fn render_agent_remaining_overview(frame: &mut Frame, area: Rect, agents: Option<&Panel>) {
    let chunks = split_with_gap(
        area,
        Direction::Horizontal,
        vec![Constraint::Percentage(50), Constraint::Percentage(50)],
    );
    render_agent_remaining_card(frame, chunks[0], "claude", agents);
    render_agent_remaining_card(frame, chunks[1], "codex", agents);
}

fn usage_left_color(left: u64) -> Color {
    if left <= 15 {
        Color::Red
    } else if left <= 35 {
        Color::Yellow
    } else {
        Color::Blue
    }
}

fn remaining_bar(left: u64, width: usize) -> String {
    let left = left.min(100);
    let filled = ((left as f64 / 100.0) * width as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn remaining_limit_line(label: &'static str, used: Option<u64>, width: usize) -> Line<'static> {
    let Some(used) = used else {
        return Line::from(vec![
            Span::styled(format!("{} ", label), Style::default().fg(Color::DarkGray)),
            Span::styled("·".repeat(width), Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(" --", Style::default().fg(Color::DarkGray)),
        ]);
    };
    let left = 100u64.saturating_sub(used.min(100));
    Line::from(vec![
        Span::styled(format!("{} ", label), Style::default().fg(Color::DarkGray)),
        Span::styled(
            remaining_bar(left, width),
            Style::default().fg(usage_left_color(left)),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:>3}%", left),
            Style::default()
                .fg(usage_left_color(left))
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn render_agent_remaining_card(
    frame: &mut Frame,
    area: Rect,
    provider: &str,
    agents: Option<&Panel>,
) {
    let limits = agent_limit_values(agents, provider);
    let (icon, label, accent) = if provider == "codex" {
        ("⬢", "Codex", Color::Cyan)
    } else {
        ("◇", "Claude", Color::LightBlue)
    };
    let bar_width = area.width.saturating_sub(18).clamp(8, 22) as usize;
    let lines = vec![
        Line::from(vec![
            Span::styled(
                icon,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                label,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        remaining_limit_line("S", limits.session_used, bar_width),
        remaining_limit_line("W", limits.weekly_used, bar_width),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        ),
        area,
    );
}

fn render_agent_visuals(
    frame: &mut Frame,
    area: Rect,
    agents: Option<&Panel>,
    history: &DashboardHistory,
) {
    let cards = split_with_gap(
        area,
        Direction::Vertical,
        vec![Constraint::Percentage(50), Constraint::Percentage(50)],
    );
    render_agent_usage_card(frame, cards[0], "claude", agents, history);
    render_agent_usage_card(frame, cards[1], "codex", agents, history);
}

fn render_agents_dashboard(
    frame: &mut Frame,
    area: Rect,
    panel: &Panel,
    _history: &DashboardHistory,
) {
    let block = panel_block(panel, Color::Cyan);
    let inner = block.inner(area).inner(Margin {
        vertical: 0,
        horizontal: 1,
    });
    frame.render_widget(block, area);
    let chunks = split_with_gap(
        inner,
        Direction::Vertical,
        vec![Constraint::Length(6), Constraint::Min(6)],
    );
    render_agent_remaining_overview(frame, chunks[0], Some(panel));
    let limit = chunks[1].height.saturating_sub(2).max(3) as usize;
    render_agent_sessions_table(frame, chunks[1], Some(panel), limit);
}

fn render_system_panel(frame: &mut Frame, area: Rect, panel: &Panel, history: &DashboardHistory) {
    let block = panel_block(panel, Color::Magenta);
    let inner = block.inner(area).inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    frame.render_widget(block, area);

    let chunks = split_with_gap(
        inner,
        Direction::Vertical,
        vec![
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Min(4),
        ],
    );

    let load = panel
        .lines
        .first()
        .cloned()
        .unwrap_or_else(|| "Waiting for system data...".to_string());
    let summary = Paragraph::new(load)
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: true });
    frame.render_widget(summary, chunks[0]);

    let (cpu, memory, disk) = system_percent_values(Some(panel));
    let gauges = split_with_gap(
        chunks[1],
        Direction::Horizontal,
        vec![
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ],
    );
    render_gauge(frame, gauges[0], "CPU", cpu as u16, percent_color(cpu));
    render_gauge(
        frame,
        gauges[1],
        "MEM",
        memory as u16,
        percent_color(memory),
    );
    render_gauge(frame, gauges[2], "DISK", disk as u16, percent_color(disk));

    let trends = split_with_gap(
        chunks[2],
        Direction::Horizontal,
        vec![
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ],
    );
    render_sparkline(
        frame,
        trends[0],
        "CPU trend",
        history_data(&history.cpu, cpu),
        Color::Red,
    );
    render_sparkline(
        frame,
        trends[1],
        "MEM trend",
        history_data(&history.memory, memory),
        Color::Yellow,
    );
    render_sparkline(
        frame,
        trends[2],
        "DISK trend",
        history_data(&history.disk, disk),
        Color::Cyan,
    );

    let processes = top_process_metrics(panel, 8);
    let max_proc_cpu = processes
        .iter()
        .map(|proc| proc.cpu)
        .fold(1.0_f64, f64::max);
    let show_command = chunks[3].width >= 104;
    let rows = processes.iter().enumerate().map(|(index, proc)| {
        let color = match index {
            0 => Color::LightRed,
            1 | 2 => Color::Yellow,
            _ => Color::Gray,
        };
        let mut cells = vec![
            Cell::from(proc.pid.clone()),
            Cell::from(proc.label.clone()),
            Cell::from(format!("{:.1}%", proc.cpu)),
            Cell::from(inline_bar(proc.cpu, max_proc_cpu, 10)),
            Cell::from(format!("{:.1}%", proc.mem)),
        ];
        if show_command {
            cells.push(Cell::from(proc.command.clone()));
        }
        Row::new(cells).style(Style::default().fg(color))
    });
    let mut widths = vec![
        Constraint::Length(7),
        Constraint::Length(22),
        Constraint::Length(7),
        Constraint::Length(12),
        Constraint::Length(6),
    ];
    if show_command {
        widths.push(Constraint::Min(24));
    }
    let mut headers = vec!["PID", "PROC", "CPU", "BAR", "MEM"];
    if show_command {
        headers.push("COMMAND");
    }
    let table = Table::new(rows, widths).header(
        Row::new(headers).style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    );
    frame.render_widget(table, chunks[3]);
}

fn attention_items(snapshot: &BTreeMap<&'static str, Panel>) -> Vec<AttentionItem> {
    let mut items = Vec::new();
    let panel_tabs = [
        ("news", 1usize),
        ("weather", 1),
        ("calendar", 1),
        ("agents", 2),
        ("system", 3),
        ("ports", 3),
        ("docker", 4),
    ];
    for (key, tab) in panel_tabs {
        if let Some(panel) = snapshot.get(key) {
            if let Some(error) = &panel.error {
                items.push(AttentionItem {
                    label: panel.title.to_uppercase(),
                    detail: truncate_chars(error, 58),
                    tab,
                    critical: true,
                });
            }
        }
    }

    let (cpu, memory, disk) = system_percent_values(snapshot.get("system"));
    for (label, value) in [("CPU pressure", cpu), ("Memory", memory), ("Disk", disk)] {
        if value >= 85 {
            items.push(AttentionItem {
                label: label.to_string(),
                detail: format!("{}% used · inspect system load", value),
                tab: 3,
                critical: value >= 95,
            });
        }
    }

    let (total, running, _) = docker_summary(snapshot.get("docker"));
    let stopped = total.saturating_sub(running);
    if stopped > 0 {
        items.push(AttentionItem {
            label: "Local services".to_string(),
            detail: format!("{} of {} containers need review", stopped, total),
            tab: 4,
            critical: stopped * 2 >= total.max(1),
        });
    }

    for provider in ["codex", "claude"] {
        let limits = agent_limit_values(snapshot.get("agents"), provider);
        let used = [limits.session_used, limits.weekly_used]
            .into_iter()
            .flatten()
            .max();
        if let Some(used) = used.filter(|value| *value >= 85) {
            items.push(AttentionItem {
                label: format!("{} quota", provider),
                detail: format!("{}% used · budget the next run", used),
                tab: 2,
                critical: used >= 95,
            });
        }
    }

    if let Some(agents) = snapshot.get("agents") {
        for provider in ["openai", "anthropic"] {
            let degraded = agents.lines.iter().find(|line| {
                let lower = line.to_lowercase();
                lower.starts_with(provider)
                    && (lower.contains("degraded")
                        || lower.contains("outage")
                        || lower.contains("investigating"))
            });
            if let Some(line) = degraded {
                items.push(AttentionItem {
                    label: format!("{} status", provider),
                    detail: truncate_chars(line, 58),
                    tab: 2,
                    critical: line.to_lowercase().contains("outage"),
                });
            }
        }
    }

    items.sort_by_key(|item| !item.critical);
    items
}

fn dashboard_summary(snapshot: &BTreeMap<&'static str, Panel>) -> DashboardSummary {
    let active_agents = agent_session_metrics(snapshot.get("agents"), usize::MAX)
        .iter()
        .filter(|session| session.state == "working")
        .count();
    let (total_services, running_services, _) = docker_summary(snapshot.get("docker"));
    let listening_ports = snapshot
        .get("ports")
        .map(|panel| {
            panel
                .lines
                .iter()
                .filter(|line| line.starts_with(':') || line.contains("LISTEN"))
                .count()
        })
        .unwrap_or(0);
    DashboardSummary {
        active_agents,
        running_services,
        total_services,
        listening_ports,
        attention: attention_items(snapshot),
    }
}

fn render_header(
    frame: &mut Frame,
    area: Rect,
    config_path: &str,
    snapshot: &BTreeMap<&'static str, Panel>,
    update: Option<&UpdateInfo>,
) {
    let summary = dashboard_summary(snapshot);
    let critical_count = summary
        .attention
        .iter()
        .filter(|item| item.critical)
        .count();
    let (health_label, health_color) = if critical_count > 0 {
        (format!("{} CRITICAL", critical_count), COLOR_DANGER)
    } else if !summary.attention.is_empty() {
        (
            format!("{} TO REVIEW", summary.attention.len()),
            COLOR_AMBER,
        )
    } else {
        ("ALL CLEAR".to_string(), COLOR_SIGNAL)
    };
    let mut top = vec![
        Span::styled(
            " AGENTDECK ",
            Style::default()
                .fg(Color::Black)
                .bg(COLOR_SIGNAL)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ● LIVE", Style::default().fg(COLOR_SIGNAL)),
    ];
    if area.width < 120 {
        top.push(Span::styled(
            format!(
                "  {} agents · {}/{} svc",
                summary.active_agents, summary.running_services, summary.total_services
            ),
            Style::default().fg(Color::Gray),
        ));
    } else {
        top.push(Span::styled(
            format!(
                "  {} active agents  ·  {}/{} services  ·  {} ports",
                summary.active_agents,
                summary.running_services,
                summary.total_services,
                summary.listening_ports
            ),
            Style::default().fg(Color::Gray),
        ));
    }
    top.extend([
        Span::raw("  "),
        Span::styled(
            format!(" {} ", health_label),
            Style::default()
                .fg(Color::Black)
                .bg(health_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    if let Some(update) = update {
        top.extend([
            Span::raw("  "),
            Span::styled(
                format!("{} READY", update.version),
                Style::default()
                    .fg(COLOR_SIGNAL)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    }
    let mut controls = Vec::new();
    if area.width >= 100 {
        controls.extend([
            Span::styled(" CONFIG ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_chars(config_path, area.width.saturating_sub(62) as usize),
                Style::default().fg(Color::Gray),
            ),
            Span::raw("   "),
        ]);
    }
    controls.extend([
        Span::styled(
            "q",
            Style::default()
                .fg(COLOR_AMBER)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" quit", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(
            "r",
            Style::default()
                .fg(COLOR_AMBER)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" refresh", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled("←/→", Style::default().fg(COLOR_AMBER)),
        Span::styled(" move", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled("?", Style::default().fg(COLOR_AMBER)),
        Span::styled(" keys", Style::default().fg(Color::DarkGray)),
    ]);
    if update.is_some() {
        controls.extend([
            Span::raw("  "),
            Span::styled("u", Style::default().fg(COLOR_SIGNAL)),
            Span::styled(" update", Style::default().fg(Color::DarkGray)),
        ]);
    }
    let header = Paragraph::new(vec![Line::from(top), Line::from(controls)]).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Color::DarkGray),
    );
    frame.render_widget(header, area);
}

fn render_tabs(
    frame: &mut Frame,
    area: Rect,
    selected_tab: usize,
    click_zones: &mut Vec<ClickZone>,
) {
    let tabs = Tabs::new(TAB_LABELS)
        .select(selected_tab)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(COLOR_CYAN)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled("    ", Style::default().fg(Color::DarkGray)))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Color::DarkGray),
        );
    frame.render_widget(tabs, area);

    let mut x = area.x;
    for (tab, label) in TAB_LABELS.iter().enumerate() {
        let label_width = label.chars().count() as u16;
        let width = if tab + 1 < TAB_COUNT {
            label_width.saturating_add(4)
        } else {
            area.right().saturating_sub(x)
        };
        click_zones.push(ClickZone {
            rect: Rect::new(x, area.y, width, area.height),
            action: ClickAction::SwitchTab(tab),
        });
        x = x.saturating_add(width);
    }
}

fn click_action_at(click_zones: &[ClickZone], column: u16, row: u16) -> Option<ClickAction> {
    click_zones
        .iter()
        .rev()
        .find(|zone| {
            column >= zone.rect.x
                && column < zone.rect.x.saturating_add(zone.rect.width)
                && row >= zone.rect.y
                && row < zone.rect.y.saturating_add(zone.rect.height)
        })
        .map(|zone| zone.action.clone())
}

fn render_metric_card(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    value: &str,
    detail: &str,
    accent: Color,
    destination: usize,
) {
    let lines = vec![
        Line::from(vec![
            Span::styled(title.to_string(), Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("  →{}", destination + 1),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ])
        .alignment(Alignment::Center),
        Line::styled(
            value.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Line::styled(detail.to_string(), Style::default().fg(Color::Gray)),
    ];
    let card = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(card, area);
}

fn register_tab_zone(click_zones: &mut Vec<ClickZone>, area: Rect, tab: usize) {
    click_zones.push(ClickZone {
        rect: area,
        action: ClickAction::SwitchTab(tab),
    });
}

fn render_attention_panel(
    frame: &mut Frame,
    area: Rect,
    snapshot: &BTreeMap<&'static str, Panel>,
    click_zones: &mut Vec<ClickZone>,
) {
    let attention = attention_items(snapshot);
    let critical_count = attention.iter().filter(|item| item.critical).count();
    let (title, accent) = if critical_count > 0 {
        (
            format!(" PRIORITY · {} CRITICAL ", critical_count),
            COLOR_DANGER,
        )
    } else if !attention.is_empty() {
        (
            format!(" PRIORITY · {} TO REVIEW ", attention.len()),
            COLOR_AMBER,
        )
    } else {
        (" STATUS · ALL CLEAR ".to_string(), COLOR_SIGNAL)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::styled(
            title,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    let mut lines = Vec::new();
    if attention.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(COLOR_SIGNAL)),
            Span::styled(
                "No action needed",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Agents, resources and local services are within healthy thresholds.",
                Style::default().fg(Color::Gray),
            ),
        ]));
    } else {
        for (row, item) in attention.iter().take(inner.height as usize).enumerate() {
            let color = if item.critical {
                COLOR_DANGER
            } else {
                COLOR_AMBER
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if item.critical { "! " } else { "▲ " },
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<16}", truncate_chars(&item.label, 16)),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(item.detail.clone(), Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("  OPEN →{}", item.tab + 1),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]));
            click_zones.push(ClickZone {
                rect: Rect::new(inner.x, inner.y.saturating_add(row as u16), inner.width, 1),
                action: ClickAction::SwitchTab(item.tab),
            });
        }
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn panel_line(panel: Option<&Panel>, index: usize) -> String {
    panel
        .and_then(|panel| panel.lines.get(index))
        .cloned()
        .unwrap_or_else(|| "Waiting".to_string())
}

fn split_with_gap(area: Rect, direction: Direction, constraints: Vec<Constraint>) -> Vec<Rect> {
    let mut expanded = Vec::new();
    for (index, constraint) in constraints.into_iter().enumerate() {
        if index > 0 {
            expanded.push(Constraint::Length(1));
        }
        expanded.push(constraint);
    }
    let parts = Layout::default()
        .direction(direction)
        .constraints(expanded)
        .split(area);
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, rect)| if index % 2 == 0 { Some(*rect) } else { None })
        .collect()
}

fn render_overview(
    frame: &mut Frame,
    area: Rect,
    snapshot: &BTreeMap<&'static str, Panel>,
    history: &DashboardHistory,
    click_zones: &mut Vec<ClickZone>,
) {
    let agents = snapshot.get("agents");
    let weather = snapshot.get("weather");
    let system = snapshot.get("system");
    let docker = snapshot.get("docker");
    let ports = snapshot.get("ports");
    let calendar = snapshot.get("calendar");
    let attention_height = attention_items(snapshot).len().clamp(1, 3) as u16 + 2;

    if area.width < 100 {
        let vertical = split_with_gap(
            area,
            Direction::Vertical,
            vec![
                Constraint::Length(attention_height),
                Constraint::Length(20),
                Constraint::Length(5),
                Constraint::Min(10),
            ],
        );

        render_attention_panel(frame, vertical[0], snapshot, click_zones);

        if let Some(panel) = agents {
            render_agents_dashboard(frame, vertical[1], panel, history);
        }
        register_tab_zone(click_zones, vertical[1], 2);
        render_metric_card(
            frame,
            vertical[2],
            "WEATHER",
            &panel_line(weather, 0),
            &panel_line(weather, 1),
            COLOR_AMBER,
            1,
        );
        register_tab_zone(click_zones, vertical[2], 1);
        if let Some(panel) = snapshot.get("news") {
            render_news_panel(frame, vertical[3], panel, COLOR_SIGNAL, click_zones);
        }
        return;
    }

    let vertical = split_with_gap(
        area,
        Direction::Vertical,
        vec![
            Constraint::Length(attention_height),
            Constraint::Length(20),
            Constraint::Min(10),
        ],
    );
    render_attention_panel(frame, vertical[0], snapshot, click_zones);
    let top = split_with_gap(
        vertical[1],
        Direction::Horizontal,
        vec![
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    );

    if let Some(panel) = agents {
        render_agents_dashboard(frame, top[0], panel, history);
    }
    register_tab_zone(click_zones, top[0], 2);

    let day_cards = split_with_gap(
        top[1],
        Direction::Vertical,
        vec![Constraint::Percentage(50), Constraint::Percentage(50)],
    );
    render_metric_card(
        frame,
        day_cards[0],
        "WEATHER",
        &panel_line(weather, 0),
        &panel_line(weather, 1),
        COLOR_AMBER,
        1,
    );
    register_tab_zone(click_zones, day_cards[0], 1);
    render_metric_card(
        frame,
        day_cards[1],
        "NEXT UP",
        &panel_line(calendar, 0),
        &panel_line(calendar, 1),
        COLOR_CYAN,
        1,
    );
    register_tab_zone(click_zones, day_cards[1], 1);

    let right_cards = split_with_gap(
        top[2],
        Direction::Vertical,
        vec![Constraint::Percentage(50), Constraint::Percentage(50)],
    );
    let memory = system
        .and_then(|panel| panel.lines.iter().find(|line| line.starts_with("Memory:")))
        .cloned()
        .unwrap_or_else(|| "Memory: waiting".to_string());
    let disk = system
        .and_then(|panel| panel.lines.iter().find(|line| line.starts_with("Disk ")))
        .cloned()
        .unwrap_or_else(|| "Disk: waiting".to_string());
    render_metric_card(
        frame,
        right_cards[0],
        "SYSTEM",
        &memory,
        &disk,
        Color::LightMagenta,
        3,
    );
    register_tab_zone(click_zones, right_cards[0], 3);

    let (docker_total, docker_running, _) = docker_summary(docker);
    let port_count = ports
        .map(|panel| {
            panel
                .lines
                .iter()
                .filter(|line| line.starts_with(':') || line.contains("LISTEN"))
                .count()
        })
        .unwrap_or(0);
    render_metric_card(
        frame,
        right_cards[1],
        "LOCAL SERVICES",
        &format!("{} / {} containers", docker_running, docker_total),
        &format!("{} listening ports", port_count),
        COLOR_DANGER,
        4,
    );
    register_tab_zone(click_zones, right_cards[1], 4);

    let content = split_with_gap(
        vertical[2],
        Direction::Horizontal,
        vec![Constraint::Percentage(58), Constraint::Percentage(42)],
    );

    if let Some(panel) = snapshot.get("news") {
        render_news_panel(frame, content[0], panel, Color::Green, click_zones);
    }
    if let Some(panel) = snapshot.get("system") {
        render_system_panel(frame, content[1], panel, history);
    }
}

fn render_agents_view(
    frame: &mut Frame,
    area: Rect,
    snapshot: &BTreeMap<&'static str, Panel>,
    history: &DashboardHistory,
) {
    if area.width >= 110 {
        let chunks = split_with_gap(
            area,
            Direction::Horizontal,
            vec![Constraint::Percentage(58), Constraint::Percentage(42)],
        );
        if let Some(panel) = snapshot.get("agents") {
            let left = split_with_gap(
                chunks[0],
                Direction::Vertical,
                vec![Constraint::Length(20), Constraint::Min(8)],
            );
            render_agents_dashboard(frame, left[0], panel, history);
            render_text_panel(frame, left[1], panel, Color::Cyan);
        }
        let side = split_with_gap(
            chunks[1],
            Direction::Vertical,
            vec![Constraint::Percentage(48), Constraint::Percentage(52)],
        );
        render_agent_visuals(frame, side[0], snapshot.get("agents"), history);
        if let Some(panel) = snapshot.get("system") {
            render_system_panel(frame, side[1], panel, history);
        }
    } else {
        let chunks = split_with_gap(
            area,
            Direction::Vertical,
            vec![
                Constraint::Percentage(42),
                Constraint::Percentage(28),
                Constraint::Percentage(30),
            ],
        );
        if let Some(panel) = snapshot.get("agents") {
            render_agents_dashboard(frame, chunks[0], panel, history);
        }
        render_agent_visuals(frame, chunks[1], snapshot.get("agents"), history);
        if let Some(panel) = snapshot.get("system") {
            render_system_panel(frame, chunks[2], panel, history);
        }
    }
}

fn render_news_view(
    frame: &mut Frame,
    area: Rect,
    snapshot: &BTreeMap<&'static str, Panel>,
    click_zones: &mut Vec<ClickZone>,
) {
    let chunks = if area.width >= 110 {
        split_with_gap(
            area,
            Direction::Horizontal,
            vec![Constraint::Percentage(68), Constraint::Percentage(32)],
        )
    } else {
        split_with_gap(
            area,
            Direction::Vertical,
            vec![Constraint::Percentage(62), Constraint::Percentage(38)],
        )
    };
    if let Some(panel) = snapshot.get("news") {
        render_news_panel(frame, chunks[0], panel, Color::Green, click_zones);
    }
    let side = split_with_gap(
        chunks[1],
        Direction::Vertical,
        vec![Constraint::Percentage(45), Constraint::Percentage(55)],
    );
    if let Some(panel) = snapshot.get("weather") {
        render_text_panel(frame, side[0], panel, Color::Yellow);
    }
    if let Some(panel) = snapshot.get("calendar") {
        render_text_panel(frame, side[1], panel, Color::LightBlue);
    }
}

fn render_ops_view(
    frame: &mut Frame,
    area: Rect,
    snapshot: &BTreeMap<&'static str, Panel>,
    history: &DashboardHistory,
) {
    let chunks = split_with_gap(
        area,
        Direction::Vertical,
        vec![Constraint::Percentage(58), Constraint::Percentage(42)],
    );
    if let Some(panel) = snapshot.get("system") {
        render_system_panel(frame, chunks[0], panel, history);
    }
    if let Some(panel) = snapshot.get("ports") {
        render_text_panel(frame, chunks[1], panel, Color::Blue);
    }
}

fn docker_summary(panel: Option<&Panel>) -> (usize, usize, usize) {
    panel
        .and_then(|panel| {
            panel.lines.iter().find_map(|line| {
                let parts = line.split('\t').collect::<Vec<_>>();
                if parts.first().copied() != Some("docker summary") {
                    return None;
                }
                Some((
                    parts
                        .get(1)
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                    parts
                        .get(2)
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                    parts
                        .get(3)
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                ))
            })
        })
        .unwrap_or((0, 0, 0))
}

fn docker_groups_from_panel(panel: &Panel) -> Vec<DockerGroupMetric> {
    panel
        .lines
        .iter()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.first().copied() != Some("docker group") {
                return None;
            }
            Some(DockerGroupMetric {
                name: parts.get(1).copied().unwrap_or("unknown").to_string(),
                total: parts
                    .get(2)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
                running: parts
                    .get(3)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
            })
        })
        .collect()
}

fn docker_containers_from_panel(panel: &Panel) -> Vec<DockerContainer> {
    panel
        .lines
        .iter()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.first().copied() != Some("docker container") {
                return None;
            }
            Some(DockerContainer {
                group: parts.get(1).copied().unwrap_or("unknown").to_string(),
                service: parts.get(2).copied().unwrap_or("container").to_string(),
                image: parts.get(3).copied().unwrap_or("").to_string(),
                status: parts.get(4).copied().unwrap_or("").to_string(),
                ports: parts.get(5).copied().unwrap_or("").to_string(),
                state: parts.get(6).copied().unwrap_or("unknown").to_string(),
                name: parts.get(7).copied().unwrap_or("").to_string(),
            })
        })
        .collect()
}

fn docker_state_style(state: &str) -> Style {
    let color = match state {
        "healthy" | "running" => Color::Green,
        "warning" => Color::Yellow,
        "stopped" => Color::Red,
        _ => Color::DarkGray,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn docker_state_dot(state: &str) -> &'static str {
    match state {
        "healthy" | "running" => "●",
        "warning" => "●",
        "stopped" => "●",
        _ => "○",
    }
}

fn render_docker_view(
    frame: &mut Frame,
    area: Rect,
    snapshot: &BTreeMap<&'static str, Panel>,
    ui_state: &UiState,
    click_zones: &mut Vec<ClickZone>,
) {
    if let Some(panel) = snapshot.get("docker") {
        let block = panel_block(panel, Color::LightRed);
        let inner = block.inner(area).inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        frame.render_widget(block, area);

        if let Some(error) = &panel.error {
            frame.render_widget(
                Paragraph::new(format!("ERROR: {}", error))
                    .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                inner,
            );
            return;
        }

        let (total, running, group_count) = docker_summary(Some(panel));
        let containers = docker_containers_from_panel(panel);
        if total == 0 || containers.is_empty() {
            frame.render_widget(
                Paragraph::new("No containers found.")
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center),
                inner,
            );
            return;
        }

        let chunks = split_with_gap(
            inner,
            Direction::Vertical,
            vec![Constraint::Length(3), Constraint::Min(6)],
        );
        let summary = Paragraph::new(Line::from(vec![
            Span::styled(
                "Containers",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} running / {} total", running, total),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} groups", group_count),
                Style::default().fg(Color::DarkGray),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(summary, chunks[0]);

        let groups = docker_groups_from_panel(panel);
        let mut rows = Vec::<Row>::new();
        let show_ports = chunks[1].width >= 96;
        let mut row_y = chunks[1].y.saturating_add(1);
        let ordered_groups = groups
            .iter()
            .filter(|group| group.running > 0)
            .chain(groups.iter().filter(|group| group.running == 0))
            .collect::<Vec<_>>();
        let mut stopped_header_added = false;
        for (index, group) in ordered_groups.iter().enumerate() {
            if group.running == 0 && !stopped_header_added {
                stopped_header_added = true;
                rows.push(
                    Row::new(vec![
                        Cell::from(""),
                        Cell::from("Stopped"),
                        Cell::from(""),
                        Cell::from(""),
                        Cell::from(""),
                    ])
                    .style(
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    ),
                );
                row_y = row_y.saturating_add(1);
            }
            let accent = match index % 5 {
                0 => Color::Magenta,
                1 => Color::Blue,
                2 => Color::Cyan,
                3 => Color::LightMagenta,
                _ => Color::Yellow,
            };
            let expanded = ui_state.expanded_docker_groups.contains(&group.name);
            click_zones.push(ClickZone {
                rect: Rect::new(chunks[1].x, row_y, chunks[1].width, 1),
                action: ClickAction::ToggleDockerGroup(group.name.clone()),
            });
            rows.push(
                Row::new(vec![
                    Cell::from(if expanded { "▾" } else { "›" }),
                    Cell::from(group.name.clone()),
                    Cell::from(format!(
                        "{} running · {} stopped · {} total",
                        group.running,
                        group.total.saturating_sub(group.running),
                        group.total
                    )),
                    Cell::from(""),
                    Cell::from(""),
                ])
                .style(Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            );
            row_y = row_y.saturating_add(1);
            if !expanded {
                continue;
            }
            for container in containers.iter().filter(|item| item.group == group.name) {
                rows.push(
                    Row::new(vec![
                        Cell::from(docker_state_dot(&container.state)),
                        Cell::from(format!(
                            "  {}\n  {}",
                            container.service,
                            truncate_chars(&container.name, 28)
                        )),
                        Cell::from(format!(
                            "{}\n{}",
                            truncate_chars(&container.image, 34),
                            if show_ports {
                                truncate_chars(&container.name, 34)
                            } else {
                                truncate_chars(&container.ports, 34)
                            }
                        )),
                        Cell::from(format!(
                            "{}\n{}",
                            container.state,
                            truncate_chars(&container.status, 28)
                        )),
                        Cell::from(truncate_chars(&container.ports, 44)),
                    ])
                    .height(2)
                    .style(docker_state_style(&container.state)),
                );
                row_y = row_y.saturating_add(2);
            }
        }

        let widths = if show_ports {
            vec![
                Constraint::Length(3),
                Constraint::Length(24),
                Constraint::Length(34),
                Constraint::Length(28),
                Constraint::Min(24),
            ]
        } else {
            vec![
                Constraint::Length(3),
                Constraint::Length(22),
                Constraint::Length(30),
                Constraint::Min(20),
                Constraint::Length(0),
            ]
        };
        let headers = if show_ports {
            vec!["", "GROUP / SERVICE", "IMAGE", "STATUS", "PORTS"]
        } else {
            vec!["", "GROUP / SERVICE", "IMAGE", "STATUS", ""]
        };
        let table = Table::new(rows, widths).header(
            Row::new(headers).style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
        );
        frame.render_widget(table, chunks[1]);
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn render_help_overlay(frame: &mut Frame, area: Rect, update_available: bool) {
    let popup = centered_rect(area, 72, if update_available { 17 } else { 16 });
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled(
            "MOVE",
            Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
        ),
        Line::from("  1–5              Jump directly to a workspace"),
        Line::from("  Tab / Shift+Tab  Move to next / previous workspace"),
        Line::from("  ← / → or h / l   Move left / right"),
        Line::from(""),
        Line::styled(
            "ACT",
            Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
        ),
        Line::from("  Click             Open cards, alerts, news and service groups"),
        Line::from("  r                 Refresh every data source now"),
    ];
    if update_available {
        lines.push(Line::from(
            "  u                 Install the available update",
        ));
    }
    lines.extend([
        Line::from("  ?                 Close this keyboard map"),
        Line::from("  q / Esc           Quit AgentDeck"),
        Line::from(""),
        Line::styled(
            "Tip: priority rows are severity-sorted shortcuts.",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(COLOR_CYAN))
            .title(Line::styled(
                " KEYBOARD MAP · ? TO CLOSE ",
                Style::default()
                    .fg(Color::Black)
                    .bg(COLOR_CYAN)
                    .add_modifier(Modifier::BOLD),
            )),
    );
    frame.render_widget(panel, popup);
}

#[allow(clippy::too_many_arguments)]
fn draw(
    frame: &mut Frame,
    panels: &SharedPanels,
    config_path: &str,
    selected_tab: usize,
    click_zones: &mut Vec<ClickZone>,
    history: &mut DashboardHistory,
    ui_state: &UiState,
    update: Option<&UpdateInfo>,
    show_help: bool,
) {
    click_zones.clear();
    let area = frame.area();
    frame.render_widget(Clear, area);
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);
    let snapshot = panels.lock().unwrap().clone();
    render_header(frame, root[0], config_path, &snapshot, update);
    render_tabs(frame, root[1], selected_tab, click_zones);

    let body = root[2].inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    sample_history(history, &snapshot);

    match selected_tab {
        0 => render_overview(frame, body, &snapshot, history, click_zones),
        1 => render_news_view(frame, body, &snapshot, click_zones),
        2 => render_agents_view(frame, body, &snapshot, history),
        3 => render_ops_view(frame, body, &snapshot, history),
        _ => render_docker_view(frame, body, &snapshot, ui_state, click_zones),
    }
    if show_help {
        render_help_overlay(frame, area, update.is_some());
    }
}

fn refresh_all(panels: &SharedPanels, config: &Config) {
    let cfg = config.clone();
    update_panel(panels, "news", || collect_news(&cfg));
    let cfg = config.clone();
    update_panel(panels, "weather", || collect_weather(&cfg));
    let cfg = config.clone();
    update_panel(panels, "calendar", || collect_calendar(&cfg));
    let cfg = config.clone();
    update_panel(panels, "agents", || collect_agents(&cfg));
    let cfg = config.clone();
    update_panel(panels, "system", || collect_system(&cfg));
    let cfg = config.clone();
    update_panel(panels, "docker", || collect_docker(&cfg));
    let cfg = config.clone();
    update_panel(panels, "ports", || collect_ports(&cfg));
}

fn run_once(config: &Config) {
    let panels = Arc::new(Mutex::new(new_panels()));
    refresh_all(&panels, config);
    for panel in panels.lock().unwrap().values() {
        println!("\n## {}", panel.title);
        if let Some(err) = &panel.error {
            println!("ERROR: {}", err);
        }
        for line in &panel.lines {
            if hidden_news_link(line).is_some() {
                continue;
            }
            println!("{}", line);
        }
    }
}

fn run_tui(config: Config, config_path: String) -> io::Result<()> {
    let panels = Arc::new(Mutex::new(new_panels()));
    let available = Arc::new(Mutex::new(None::<UpdateInfo>));
    let agent_service_status = Arc::new(Mutex::new(Vec::<String>::new()));
    let claude_limits = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let available = available.clone();
        thread::spawn(move || {
            if let Ok(update) = available_update(false) {
                *available.lock().unwrap() = update;
            }
        });
    }
    {
        let cfg = config.clone();
        spawn_worker(panels.clone(), "news", config.refresh.news, move || {
            collect_news(&cfg)
        });
    }
    {
        let cfg = config.clone();
        spawn_worker(
            panels.clone(),
            "weather",
            config.refresh.weather,
            move || collect_weather(&cfg),
        );
    }
    {
        let cfg = config.clone();
        spawn_worker(
            panels.clone(),
            "calendar",
            config.refresh.calendar,
            move || collect_calendar(&cfg),
        );
    }
    {
        let cfg = config.clone();
        let status = agent_service_status.clone();
        thread::spawn(move || loop {
            let lines = collect_agent_service_status(&cfg);
            *status.lock().unwrap() = lines;
            thread::sleep(Duration::from_secs(AGENT_SERVICE_STATUS_INTERVAL_SECS));
        });
    }
    {
        let limits = claude_limits.clone();
        thread::spawn(move || loop {
            let lines = collect_claude_limits();
            if !lines.is_empty() {
                *limits.lock().unwrap() = lines;
            }
            thread::sleep(Duration::from_secs(CLAUDE_USAGE_INTERVAL_SECS));
        });
    }
    {
        let cfg = config.clone();
        let status = agent_service_status.clone();
        spawn_worker(panels.clone(), "agents", config.refresh.agents, move || {
            let mut lines = collect_local_agents(&cfg)?;
            lines.extend(claude_limits.lock().unwrap().clone());
            lines.extend(status.lock().unwrap().clone());
            Ok(lines)
        });
    }
    {
        let cfg = config.clone();
        spawn_worker(panels.clone(), "system", config.refresh.system, move || {
            collect_system(&cfg)
        });
    }
    {
        let cfg = config.clone();
        spawn_worker(panels.clone(), "docker", config.refresh.docker, move || {
            collect_docker(&cfg)
        });
    }
    {
        let cfg = config.clone();
        spawn_worker(panels.clone(), "ports", config.refresh.ports, move || {
            collect_ports(&cfg)
        });
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut selected_tab = 0usize;
    let mut click_zones = Vec::<ClickZone>::new();
    let mut history = DashboardHistory::default();
    let mut ui_state = UiState::default();
    let mut update_requested = false;
    let mut show_help = false;
    let mut last_mouse_action_at = None::<Instant>;

    let result = loop {
        terminal.draw(|frame| {
            let update = available.lock().unwrap().clone();
            draw(
                frame,
                &panels,
                &config_path,
                selected_tab,
                &mut click_zones,
                &mut history,
                &ui_state,
                update.as_ref(),
                show_help,
            )
        })?;
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('?') => show_help = !show_help,
                    KeyCode::Esc if show_help => show_help = false,
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                    KeyCode::Tab => selected_tab = (selected_tab + 1) % TAB_COUNT,
                    KeyCode::BackTab => selected_tab = (selected_tab + TAB_COUNT - 1) % TAB_COUNT,
                    KeyCode::Right | KeyCode::Char('l') => {
                        selected_tab = (selected_tab + 1) % TAB_COUNT
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        selected_tab = (selected_tab + TAB_COUNT - 1) % TAB_COUNT
                    }
                    KeyCode::Char('1') => selected_tab = 0,
                    KeyCode::Char('2') => selected_tab = 1,
                    KeyCode::Char('3') => selected_tab = 2,
                    KeyCode::Char('4') => selected_tab = 3,
                    KeyCode::Char('5') => selected_tab = 4,
                    KeyCode::Char('r') => {
                        let panels = panels.clone();
                        let config = config.clone();
                        thread::spawn(move || refresh_all(&panels, &config));
                    }
                    KeyCode::Char('u') if available.lock().unwrap().is_some() => {
                        update_requested = true;
                        break Ok(());
                    }
                    _ => {}
                },
                Event::Mouse(mouse) if !show_help => {
                    if matches!(
                        mouse.kind,
                        MouseEventKind::Down(MouseButton::Left)
                            | MouseEventKind::Up(MouseButton::Left)
                    ) {
                        let action = click_action_at(&click_zones, mouse.column, mouse.row);
                        if let Some(action) = action {
                            let duplicate =
                                matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left))
                                    && last_mouse_action_at.is_some_and(|instant| {
                                        instant.elapsed() < Duration::from_millis(500)
                                    });
                            if duplicate {
                                continue;
                            }
                            last_mouse_action_at = Some(Instant::now());
                            match action {
                                ClickAction::OpenUrl(url) => open_url(&url),
                                ClickAction::SwitchTab(tab) => selected_tab = tab,
                                ClickAction::ToggleDockerGroup(group) => {
                                    if !ui_state.expanded_docker_groups.insert(group.clone()) {
                                        ui_state.expanded_docker_groups.remove(&group);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    };

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    if update_requested {
        match perform_update() {
            Ok(message) => println!("{}", message),
            Err(err) => eprintln!("Update failed: {}", err),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_panel(lines: &[&str]) -> Panel {
        Panel {
            title: "Test",
            lines: lines.iter().map(|line| line.to_string()).collect(),
            error: None,
            updated: None,
            loading: false,
        }
    }

    #[test]
    fn process_display_name_prefers_app_bundle_name() {
        let command = "/Applications/OrbStack.app/Contents/Frameworks/OrbStack Helper.app/Contents/MacOS/OrbStack Helper vmgr";
        assert_eq!(process_display_name(command, 22), "OrbStack");
    }

    #[test]
    fn process_display_name_uses_executable_basename() {
        let command =
            "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer -daemon";
        assert_eq!(process_display_name(command, 22), "WindowServer");
    }

    #[test]
    fn top_process_metrics_keeps_command_detail() {
        let panel = test_panel(&[
            "Top processes:",
            "   9500 OrbStack           cpu  20.9% mem  6.0% cmd /Applications/OrbStack.app/Contents/MacOS/OrbStack",
        ]);
        let rows = top_process_metrics(&panel, 5);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pid, "9500");
        assert_eq!(rows[0].label, "OrbStack");
        assert_eq!(rows[0].cpu, 20.9);
        assert!(rows[0].command.contains("OrbStack"));
    }

    #[test]
    fn agent_session_rows_are_parsed_for_sessions_table() {
        let panel = test_panel(&[
            "agent sessions:",
            "agent session: codex id=019f3f58 state=working age=30sago tok=36.3M ctx=258.4K",
            "agent session: claude id=81903f26 state=recent age=4mago tok=43.5K cost=$0.98",
        ]);
        let sessions = agent_session_metrics(Some(&panel), 5);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].provider, "codex");
        assert_eq!(sessions[0].state, "working");
        assert_eq!(sessions[0].detail, "ctx 258.4K");
        assert_eq!(sessions[1].provider, "claude");
        assert_eq!(sessions[1].detail, "$0.98");
    }

    #[test]
    fn agent_limits_preserve_unknown_instead_of_claiming_full_quota() {
        let no_limits = test_panel(&["claude latest: in 20K, out 4K tok"]);
        assert_eq!(
            agent_limit_values(Some(&no_limits), "claude"),
            AgentLimits::default()
        );

        let partial = test_panel(&["codex limits: 22% 5h, weekly unavailable"]);
        assert_eq!(
            agent_limit_values(Some(&partial), "codex"),
            AgentLimits {
                session_used: Some(22),
                weekly_used: None,
            }
        );
    }

    #[test]
    fn claude_oauth_usage_is_parsed_into_agent_limits() {
        let usage = r#"{
            "five_hour": {"utilization": 4.0, "resets_at": "soon"},
            "seven_day": {"utilization": 48.4, "resets_at": "later"}
        }"#;
        assert_eq!(
            claude_limits_from_usage_json(usage),
            AgentLimits {
                session_used: Some(4),
                weekly_used: Some(48),
            }
        );
    }

    #[test]
    fn claude_oauth_usage_preserves_missing_periods() {
        let usage = r#"{"five_hour": null, "seven_day": {"utilization": 61}}"#;
        assert_eq!(
            claude_limits_from_usage_json(usage),
            AgentLimits {
                session_used: None,
                weekly_used: Some(61),
            }
        );
    }

    #[test]
    fn agent_session_age_and_state_are_derived_live_from_mtime() {
        let modified = now_secs().saturating_sub(61);
        let panel = test_panel(&[&format!(
            "agent session: codex id=live state=idle age=9hago mtime={} tok=2.0M ctx=64K",
            modified
        )]);
        let sessions = agent_session_metrics(Some(&panel), 1);
        assert_eq!(sessions[0].state, "working");
        assert_eq!(sessions[0].age, "1mago");
    }

    #[test]
    fn local_agent_refresh_is_fast_by_default() {
        assert_eq!(default_config().refresh.agents, 5);
    }

    #[test]
    fn agent_cost_is_parsed_for_usage_card() {
        let panel = test_panel(&["claude 24h: 12 sessions, 4.0M tok, $1947.87"]);
        assert_eq!(agent_24h_tokens(Some(&panel), "claude"), 4_000_000.0);
        assert_eq!(agent_24h_cost(Some(&panel), "claude"), Some(1947.87));
        assert_eq!(agent_24h_cost(Some(&panel), "codex"), None);
    }

    #[test]
    fn trend_line_is_compact_and_width_limited() {
        let data = vec![0, 10, 30, 60, 90, 100];
        let line = trend_line(&data, 4);
        assert_eq!(line.chars().count(), 4);
        assert!(line.ends_with('@'));
    }

    #[test]
    fn news_refresh_metadata_is_runtime_only() {
        assert!(is_news_runtime_metadata(
            "news refresh: next check 10:00:00"
        ));
        assert!(is_news_runtime_metadata("news source: cache hit"));
        assert!(!is_news_runtime_metadata("translation: codex"));
    }

    #[test]
    fn display_wrapping_accounts_for_wide_cjk_characters() {
        assert_eq!(wrap_display_line("AI 中文", 4), vec!["AI ", "中文"]);
        assert_eq!(wrap_display_line("", 4), vec![""]);
    }

    #[test]
    fn wrapped_news_titles_keep_every_visual_row_clickable() {
        let backend = ratatui::backend::TestBackend::new(24, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let panel = test_panel(&[
            "這是一個很長而且會自動換行的新聞標題",
            "@@link https://example.com/first",
            "  example.com | now",
            "Second headline",
            "@@link https://example.com/second",
        ]);
        let mut click_zones = Vec::new();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_news_panel(frame, area, &panel, COLOR_SIGNAL, &mut click_zones);
            })
            .unwrap();

        assert_eq!(click_zones.len(), 2);
        assert!(click_zones[0].rect.height > 1);
        assert!(click_zones[1].rect.y > click_zones[0].rect.y);
        let continuation_row = click_zones[0].rect.y + click_zones[0].rect.height - 1;
        assert_eq!(
            click_action_at(&click_zones, click_zones[0].rect.x, continuation_row),
            Some(ClickAction::OpenUrl(
                "https://example.com/first".to_string()
            ))
        );
    }

    #[test]
    fn click_hit_testing_prefers_the_topmost_zone() {
        let zones = vec![
            ClickZone {
                rect: Rect::new(1, 1, 10, 4),
                action: ClickAction::SwitchTab(1),
            },
            ClickZone {
                rect: Rect::new(2, 2, 4, 2),
                action: ClickAction::SwitchTab(4),
            },
        ];
        assert_eq!(
            click_action_at(&zones, 3, 2),
            Some(ClickAction::SwitchTab(4))
        );
    }

    #[test]
    fn services_tab_hit_zone_reaches_the_right_edge() {
        let backend = ratatui::backend::TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut click_zones = Vec::new();
        terminal
            .draw(|frame| {
                render_tabs(frame, frame.area(), 0, &mut click_zones);
            })
            .unwrap();

        assert_eq!(
            click_action_at(&click_zones, 72, 1),
            Some(ClickAction::SwitchTab(4))
        );
        assert_eq!(
            click_action_at(&click_zones, 79, 1),
            Some(ClickAction::SwitchTab(4))
        );
    }

    #[test]
    fn append_news_refresh_metadata_replaces_old_values() {
        let config = default_config();
        let lines = append_news_refresh_metadata(
            vec![
                "Headline".to_string(),
                "news refresh: old".to_string(),
                "news source: old".to_string(),
            ],
            &config,
            "cache hit",
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with("news refresh:"))
                .count(),
            1
        );
        assert!(lines.iter().any(|line| line.contains("next check")));
        assert!(lines.iter().any(|line| line == "news source: cache hit"));
    }

    #[test]
    fn docker_group_and_service_are_inferred_like_desktop() {
        assert_eq!(infer_docker_group("ifrs-backend-1", ""), "ifrs");
        assert_eq!(infer_docker_service("ifrs-backend-1", "", ""), "backend");
        assert_eq!(
            infer_docker_group("k8s_postgres_prompts-postgres_abc", ""),
            "Kubernetes"
        );
        assert_eq!(docker_state("Up 2 hours (healthy)"), "healthy");
        assert_eq!(docker_state("Exited (0) 3 minutes ago"), "stopped");
    }

    #[test]
    fn docker_lines_keep_summary_groups_and_containers() {
        let lines = format_docker_lines(vec![
            DockerContainer {
                group: "ifrs".to_string(),
                service: "backend".to_string(),
                name: "ifrs-backend-1".to_string(),
                image: "ifrs-backend".to_string(),
                status: "Up 2 hours".to_string(),
                ports: "127.0.0.1:8080->8080/tcp".to_string(),
                state: "running".to_string(),
            },
            DockerContainer {
                group: "ifrs".to_string(),
                service: "frontend".to_string(),
                name: "ifrs-frontend-1".to_string(),
                image: "ifrs-frontend".to_string(),
                status: "Exited (0) 1 hour ago".to_string(),
                ports: "no published ports".to_string(),
                state: "stopped".to_string(),
            },
        ]);
        let panel = test_panel(&lines.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(docker_summary(Some(&panel)), (2, 1, 1));
        assert_eq!(docker_groups_from_panel(&panel)[0].name, "ifrs");
        assert_eq!(docker_containers_from_panel(&panel).len(), 2);
    }

    #[test]
    fn dashboard_summary_surfaces_actionable_health_at_a_glance() {
        let mut snapshot = new_panels();
        snapshot.insert(
            "agents",
            test_panel(&[
                "codex limits: 96% 5h, 45% weekly",
                "agent session: codex id=abc state=working age=10sago tok=2.1M ctx=64K",
                "agent session: claude id=def state=idle age=2hago tok=1.2M cost=$2.10",
            ]),
        );
        snapshot.insert(
            "system",
            test_panel(&[
                "CPU pressure: 22%",
                "Memory: 28GB / 32GB (88%)",
                "Disk /: 20GB free / 500GB (480GB used, 96%)",
            ]),
        );
        snapshot.insert(
            "docker",
            test_panel(&["docker summary\t4\t3\t2", "docker group\tapp\t4\t3"]),
        );
        snapshot.insert(
            "ports",
            test_panel(&[
                ":3000   node             pid 100 user sammy",
                ":5432   postgres         pid 101 user sammy",
            ]),
        );

        let summary = dashboard_summary(&snapshot);
        assert_eq!(summary.active_agents, 1);
        assert_eq!(summary.running_services, 3);
        assert_eq!(summary.total_services, 4);
        assert_eq!(summary.listening_ports, 2);
        assert!(summary
            .attention
            .iter()
            .any(|item| item.label == "codex quota" && item.tab == 2));
        assert!(summary
            .attention
            .iter()
            .any(|item| item.label == "Disk" && item.critical));
        assert!(summary.attention.first().is_some_and(|item| item.critical));
    }

    #[test]
    fn dashboard_attention_maps_panel_failures_to_the_right_workspace() {
        let mut snapshot = new_panels();
        let docker = snapshot.get_mut("docker").unwrap();
        docker.error = Some("Docker daemon unavailable".to_string());

        let attention = attention_items(&snapshot);
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].label, "DOCKER");
        assert_eq!(attention[0].tab, 4);
        assert!(attention[0].critical);
    }

    #[test]
    fn overview_renders_at_the_supported_eighty_column_width() {
        let backend = ratatui::backend::TestBackend::new(80, 55);
        let mut terminal = Terminal::new(backend).unwrap();
        let panels = Arc::new(Mutex::new(new_panels()));
        let mut click_zones = Vec::new();
        let mut history = DashboardHistory::default();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &panels,
                    "defaults",
                    0,
                    &mut click_zones,
                    &mut history,
                    &UiState::default(),
                    None,
                    false,
                )
            })
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("AGENTDECK"));
        assert!(rendered.contains("STATUS · ALL CLEAR"));
        assert!(rendered.contains("WEATHER  →2"));
        assert!(!click_zones.is_empty());
    }

    #[test]
    fn versions_are_compared_numerically() {
        assert!(is_newer_version("v0.10.0", "0.9.9"));
        assert!(is_newer_version("v1.0.1", "1.0.0"));
        assert!(!is_newer_version("v1.0.0", "1.0.0"));
        assert!(!is_newer_version("v0.9.9", "1.0.0"));
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let mut config_path = None;
    let mut once = false;
    let mut print_default_config = false;
    let mut update = false;
    let mut check_update = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config_path = args.next(),
            "--once" => once = true,
            "--print-default-config" => print_default_config = true,
            "--version" | "-V" => {
                println!("agentdeck {}", VERSION);
                return;
            }
            "update" => update = true,
            "--check" if update => check_update = true,
            "-h" | "--help" => {
                println!(
                    "{}\n\nUsage:\n  agentdeck [--config config.json] [--once]\n  agentdeck --print-default-config\n  agentdeck --version\n  agentdeck update [--check]",
                    DISPLAY_NAME
                );
                return;
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                std::process::exit(2);
            }
        }
    }
    if print_default_config {
        println!("{}", default_config_json());
        return;
    }
    if update {
        if check_update {
            match available_update(true) {
                Ok(Some(info)) => println!(
                    "AgentDeck {} is available (current {}).",
                    info.version, VERSION
                ),
                Ok(None) => println!("AgentDeck {} is already up to date.", VERSION),
                Err(err) => {
                    eprintln!("Update check failed: {}", err);
                    std::process::exit(1);
                }
            }
        } else {
            match perform_update() {
                Ok(message) => println!("{}", message),
                Err(err) => {
                    eprintln!("Update failed: {}", err);
                    std::process::exit(1);
                }
            }
        }
        return;
    }
    let (config, loaded_path) = load_config(config_path);
    if once {
        run_once(&config);
    } else if let Err(err) = run_tui(config, loaded_path) {
        eprintln!("tui error: {}", err);
        std::process::exit(1);
    }
}
