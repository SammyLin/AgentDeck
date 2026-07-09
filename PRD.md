# AgentDeck PRD

## 1. 背景

使用者希望有一個可以長時間開著的 terminal dashboard，用 TUI 方式集中監控日常工作所需資訊：最新 AI 新聞與翻譯、天氣、客戶端行事曆、Codex 與 Claude Code agent 狀態、本機系統資源、Docker containers，以及 ports 狀態。

這個工具的定位不是聊天機器人，而是常駐式工作台。它需要低資源占用、資訊密度高、可快速掃描，並且在外部服務或權限失敗時不應中斷。

## 2. 目標

- 建立一個 Rust 實作的 TUI dashboard，可在 macOS / Linux terminal 長時間運行。
- 定期更新 AI 新聞並翻譯成繁體中文。
- 顯示指定地點天氣與短期預報。
- 顯示客戶端行事曆接下來幾天的事件。
- 監控 Codex 與 Claude Code agent 的本機 process 狀態，以及 OpenAI / Anthropic 服務狀態。
- 監控 CPU、memory、disk、top processes。
- 顯示 Docker container 狀態與 port mapping。
- 顯示本機 listening TCP ports。

## 3. 非目標

- 不在 MVP 內做完整 calendar OAuth 登入流程。
- 不在 MVP 內實作完整新聞推薦或摘要模型。
- 不在 MVP 內控制、啟停、刪除 Docker containers。
- 不在 MVP 內主動操作 Codex 或 Claude Code，只監控狀態。
- 不在 MVP 內提供 web UI。

## 4. 主要使用情境

- 使用者每天開機後執行 `agentdeck`，放在 terminal 或 tmux pane 內常駐。
- 使用者快速確認目前 AI 新聞、天氣、客戶會議、系統負載、Docker 服務與 ports。
- 使用者在跑 Codex / Claude Code 時，可以看到本機 agent process 是否存在、PID、CPU、memory。
- 使用者在開發本機服務時，可以立即看到 Docker containers 與 listening ports 是否符合預期。

## 5. 使用者故事

- 身為使用者，我想看到最新 AI 新聞並翻譯成繁中，讓我不用切 browser 就能知道重點。
- 身為使用者，我想看到天氣，讓我可以在工作台內掌握出門或通勤狀況。
- 身為使用者，我想看到客戶端行事曆，讓我不漏掉近期會議。
- 身為使用者，我想知道 Codex / Claude Code 是否正在跑，讓我能確認 agent 任務是否還活著。
- 身為使用者，我想看到 CPU / memory / disk / top processes，讓我能快速判斷機器是否卡住。
- 身為使用者，我想看到 Docker containers 和 ports，讓我能排查本機服務是否正常。

## 6. MVP 功能需求

### 6.1 TUI Dashboard

- 啟動後進入全螢幕 terminal dashboard。
- 使用分頁式資訊架構，避免窄 terminal 同時塞滿所有 panel：
  - Overview：較大的 Codex / Claude session 與用量框 + 天氣 / 系統摘要 + 主要新聞
  - News：AI News + Weather + Calendar
  - Agent：Codex / Claude session、用量、服務狀態 + system 摘要
  - Ops：System + Ports
  - Docker：依 project / Kubernetes 分組的 containers tree
- 支援快捷鍵：
  - `q`: 離開
  - `r`: 立即刷新所有 panel
  - `Tab`: 切換分頁
  - `1` / `2` / `3` / `4` / `5`: 直接跳到指定分頁
  - 滑鼠點擊 tab：切換分頁
  - 滑鼠點擊新聞標題：開啟原文連結
  - 滑鼠點擊 Docker group：展開 / 收合 containers
- 80 欄 terminal 下，Overview 只顯示高優先資訊；詳細監控放到各自分頁。

### 6.2 AI News

- 支援設定 RSS URLs。
- 預設來源包含 TechCrunch AI、VentureBeat AI、AI News。
- 使用者可自行加入 Google News AI 搜尋 RSS 或其他 RSS 來源。
- 顯示新聞標題、來源、時間。
- 新聞原始連結不可直接顯示成裸 URL；標題需可用滑鼠點擊開啟。
- 新聞需寫入本機 cache，啟動時優先讀 cache；只有超過 refresh interval 才重抓。
- 支援設定顯示數量。
- 預設使用本機 Codex (`codex exec`) 將標題翻譯成繁體中文。
- 翻譯 provider 支援 `codex` / `codex_acp` / `openai` / `none`。
- Codex 翻譯失敗時，顯示原文與 Codex 錯誤提示，不提示 `OPENAI_API_KEY`。

### 6.3 Weather

- 使用 Open-Meteo API。
- 透過 config 設定地點名稱、latitude、longitude、timezone。
- 顯示目前溫度、體感溫度、濕度、風速。
- 顯示未來數日最高 / 最低溫與降雨機率。

### 6.4 Calendar

- 支援 `.ics` URL。
- 支援本機 `.ics` file。
- 顯示未來 `lookahead_days` 內的事件。
- 事件欄位包含時間、標題、地點。
- 未設定 calendar 時，panel 顯示設定提示。

### 6.5 Codex / Claude Agent Status

- 透過 process table 監控本機狀態。
- 預設 Codex keywords：
  - `codex`
- 預設 Claude keywords：
  - `claude`
  - `claude-code`
- 顯示：
  - 是否有本機 process
  - process 數量
  - PID
  - CPU 使用量
  - memory 使用量
  - 最新 session 更新時間
  - Codex token usage / rate limit 摘要
  - Claude Code token / cost / message / tool 摘要
- Overview 需要顯示 Codex / Claude Code 使用量摘要。
- 同時讀取服務狀態頁：
  - OpenAI status summary
  - Anthropic status summary
- 狀態頁不可用時，顯示錯誤但不中斷 dashboard。

### 6.6 System Status

- 顯示 load average 或 uptime。
- 顯示 memory 使用量。
- 顯示 root disk 使用量。
- 顯示 top N processes，依 CPU 使用量排序。
- 支援設定 top process 數量。

### 6.7 Docker Containers

- 使用 `docker ps -a` 取得 running 與 stopped container 狀態。
- 依 Docker Compose project label 分組。
- Kubernetes container 需統一歸入 `Kubernetes` group。
- 無 compose label 時，從 container name 推斷 group。
- Docker tab 預設所有 group 收合，點擊 group row 後展開。
- 顯示：
  - group / service name
  - container name
  - image
  - running / stopped / healthy / warning 狀態
  - status text
  - published ports
- Docker daemon 不可用或 socket 無權限時，panel 顯示錯誤。
- MVP 僅讀取，不執行 container 操作。

### 6.8 Ports

- 優先使用 `lsof -nP -iTCP -sTCP:LISTEN`。
- 若 `lsof` 不可用，fallback 到 `netstat -an`。
- 顯示：
  - port
  - command
  - PID
  - user
- 支援設定顯示數量。

## 7. 設定需求

工具會讀取以下設定位置，依序套用第一個存在的檔案：

- `./config.json`
- `~/.config/agentdeck/config.json`

設定項目包含：

- refresh interval
- news RSS URLs
- translation provider / model
- weather location / coordinates
- calendar ICS URLs / files
- Codex / Claude process keywords
- OpenAI / Anthropic status URLs
- Docker 顯示數量
- ports 顯示數量
- top process 顯示數量

## 8. 技術需求

- 使用 Rust 實作。
- TUI 使用 `ratatui` / `crossterm`，提供色彩、layout、widgets、鍵盤事件與 terminal raw mode。
- HTTP 可透過系統 `curl` 執行。
- 資料取得與系統監控邏輯需和 TUI renderer 分離，避免視覺調整影響收集器。
- 必須提供：
- `cargo run`
  - `cargo run -- --once`
  - `cargo build --release`
- `--once` 模式需收集一次所有資訊並輸出純文字，方便 debug 或排程檢查。

## 9. 錯誤處理

- 任一 panel 更新失敗時，只影響該 panel。
- 新聞來源需容忍單一 RSS 失敗；只有全部新聞來源失敗時才顯示 panel error。
- panel 顯示錯誤訊息第一行，避免佔滿畫面。
- 網路失敗、DNS 失敗、API 失敗、Docker socket 無權限、calendar URL 無法讀取，都不可讓程式崩潰。
- TUI 離開時需恢復 terminal 狀態。

## 10. 安全與隱私

- 不將 calendar 內容、process list、Docker 資訊送到第三方，除非使用者啟用翻譯。
- 翻譯 MVP 僅送新聞標題，不送 calendar 或本機狀態。
- API key 只從環境變數讀取，不寫入 config。
- Docker 與 process 監控僅讀取，不做破壞性操作。

## 11. 成功指標

- `cargo check` 通過。
- `cargo run -- --once` 在無網路或 Docker 無權限時仍能完成執行。
- TUI 可連續運行至少 8 小時不中斷。
- 常態 refresh 下 CPU 使用量低，沒有明顯 memory leak。
- 使用者能在一個畫面內看見新聞、天氣、行事曆、agent、本機系統、Docker、ports。

## 12. 後續版本

- 使用 `reqwest` / `serde_json` / `quick-xml` 取代手寫解析。
- 加入 OAuth calendar connector，例如 Google Calendar。
- 加入新聞摘要與重要性排序。
- 加入 Codex / Claude session log 掃描。
- 加入 Docker container healthcheck 狀態。
- 加入 port allowlist / expected services，標示異常開關。
- 加入 notification，例如 terminal bell、Slack、email。
- 加入 config reload，不需重啟程式。

## 13. 開放問題

- 客戶端行事曆來源是 Google Calendar、Outlook，還是固定 `.ics`？
- 天氣預設地點要使用台北，還是使用者目前所在城市？
- AI 新聞來源要只看英文，還是加入中文來源？
- Codex / Claude 是否需要監控 session log、任務名稱、token 使用量，還是 MVP 只看 process 和服務狀態？
- Docker 是否需要顯示 stopped containers，或只顯示 running containers？
- Ports 是否需要設定「預期應該開的 port」與「不應該開的 port」？
