# 實作與驗收紀錄

狀態：開發預覽，**spec.md 尚未全部完成**。完整待辦見 [roadmap.md](roadmap.md)。

## 已實作

- Rust CLI、穩定英文 JSON 欄位、`sanc`／npm `sessanchor` 別名。
- 明確 host/user/port/key 設定；OpenSSH adapter 使用 `-F none`，不依賴 SSH config。
- 嚴格 known_hosts 驗證；不接受未知／變更金鑰，不自動安裝 helper。
- rusqlite + bundled SQLite（非 Turso）：裝置、連線快照／事件、session 說明、任務意圖。
- SQLite immediate transaction 去重、參數衝突拒絕、單次 claim、跨程序重送；不宣稱 exactly-once。
- 本機 background worker：提交端正常退出後繼續，stdout／stderr 分開保存，UTF-8／hex 有界分頁。
- worker 心跳與 boot ID：失聯超過 30 秒或跨開機重新讀取時標示 unknown，不自動重派。
- 核心控制權原子取得／釋放與租約；尚未接 human shell／互動接手介面。
- Unix 本機監測 daemon：私有目錄、file lock 單例、Unix socket、當次開機已成功連線裝置、定期短連線探測、預設 2 小時休眠。
- MCP stdio：預設唯讀；`--allow-exec` 明確啟用 session 寫入／執行工具，共用 CLI 核心。
- Codex／Claude Code PreToolUse adapter 與精簡 skill／設定範例。沒有修改使用者 agent 設定。
- npm 原生 binary 包裝與平台／SHA-256 檢查；不在安裝時編譯或下載執行檔，禁止誤發布。

## 硬性規則

命令去除前導空白後以 `sudo` 為首詞，包含接 shell 分隔符，核心回 `approval_required`。
CLI／MCP 要求 STOP 並交由使用者，不重試或改寫。無 override。這不是完整 shell parser；
`env sudo`、腳本內 sudo、引號拼接等不在字面前綴規則保證內。

## 已量測

- 本機 Rust 測試、格式檢查、Clippy warnings-as-errors：通過；包含 CLI、SQLite、跨連線 claim／boot 恢復、sudo、背景 worker、binary 輸出、MCP、hook、daemon 單例／休眠。
- skill-creator validator：通過。指引刻意保持精簡，按需查 CLI，不注入完整手冊。
- npm launcher 測試與 darwin-arm64 tarball 隔離安裝：成功，兩個別名可執行。
- Pi 5 probe：成功，既有指紋驗證通過。
- Pi 5 第一個 background 測試：空輸出、最終 unknown，未重派。
- 改為獨立 process group 後，以不同 session/request 執行新的唯讀測試：輸出 `Linux\naarch64\n`，exit 0。沒有 sudo／安裝或遠端檔案修改。

## 不可誤認為完成的部分

- 真正的 SSH multiplex/reconnect／遠端持久化仍未完成。目前 `remote_persistence:false`，斷線可能終止遠端工作。SSH 255 保守視為 unknown。
- daemon 目前做短連線探測，`persistent_connections:false`；不是已完成的長連線池。探測尚為串行，多台失聯會延後 IPC 回應；動態裝置加入、退避與每裝置休眠覆寫仍待補。
- session 目前是任務分組／序列化，不是持久 shell；cwd/env、PTY、唯讀選單、取消與人類接手 UI 未完成。
- 7 天／1 GiB 全域 retention 未完成。暫採每 stream 64 MiB cap，溢位標示 truncated，不聲稱保存完整輸出。
- 密碼／私鑰密語輸入、多跳、首次指紋確認工作流程未完成。
- 常用排序、置頂與 OS cache 未完成。檔案傳輸／同步／大型續傳未完成。
- Unix 資料目錄權限已檢查；Windows ACL／boot ID 尚未實作。其他平台編譯／執行與跨平台 binary 包未驗收。
- MCP／hook 已做合成協定測試，未在 Claude Code／Codex 的真實 agent session 完成載入、權限與 context-mode 共存驗收。
- npm registry 尚未發布；package private=true。本機 tarball 只含打包機平台；校驗不是簽章或供應鏈證明。
- 本機 SQLite 時鐘回退、schema migration 的並行初次啟動與全域 retention 故障注入仍待完整驗證。

## 參考

透過 Context7 查核 Rust、rusqlite、clap、Serde JSON、OpenSSH、npm 與 MCP 文件。
Agent hooks 與 context-mode 來源見 [integrations.md](integrations.md)。文件抓取工具缺少 turndown 時改查官方頁面，未修改工具安裝。
