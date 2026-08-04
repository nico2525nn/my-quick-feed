pub mod ai;
pub mod config;
pub mod db;
pub mod discord;
pub mod errors;
pub mod fetcher;
pub mod pipeline;
pub mod scheduler;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::path::PathBuf;
use std::sync::LazyLock;
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    tray::TrayIconBuilder,
    Manager,
};
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use crate::errors::AppResult;
use config::{AppConfig, ConfigManager, TopicConfig};
use db::{Database, LogEntry};
use discord::DiscordClient;
use pipeline::Pipeline;
use scheduler::Scheduler;

/// 共有ログバッファ（tracing layer と AppState で共用）
static LOG_BUFFER: LazyLock<Arc<parking_lot::Mutex<Vec<LogEntry>>>> =
    LazyLock::new(|| Arc::new(parking_lot::Mutex::new(Vec::new())));

/// tracing のイベントをキャプチャして LOG_BUFFER に書き込む layer
struct LogCaptureLayer;

impl<S: tracing::Subscriber> Layer<S> for LogCaptureLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();

        // フォーマット済みメッセージを収集
        let mut msg = String::new();
        let mut topic = String::new();
        let mut visitor = LogFieldVisitor {
            message: &mut msg,
            topic: &mut topic,
        };
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: metadata.level().to_string(),
            topic: if topic.is_empty() {
                metadata.target().to_string()
            } else {
                topic
            },
            message: msg,
        };

        let mut buf = LOG_BUFFER.lock();
        buf.push(entry);
        if buf.len() > 1000 {
            buf.remove(0);
        }
    }
}

/// イベントフィールドを収集する Visitor
struct LogFieldVisitor<'a> {
    message: &'a mut String,
    topic: &'a mut String,
}

impl tracing::field::Visit for LogFieldVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => *self.message = format!("{:?}", value).trim_matches('"').to_string(),
            "topic" => *self.topic = format!("{:?}", value).trim_matches('"').to_string(),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => *self.message = value.to_string(),
            "topic" => *self.topic = value.to_string(),
            _ => {}
        }
    }
}

pub fn read_logs() -> Vec<LogEntry> {
    LOG_BUFFER.lock().clone()
}

/// アプリケーション状態
pub struct AppState {
    pub config_manager: Arc<ConfigManager>,
    pub db: Arc<Database>,
    pub scheduler: Arc<Scheduler>,
    pub running: AtomicBool,
    pub log_dir: PathBuf,
}

/// ===== Tauri IPC Commands =====

#[tauri::command]
async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config_manager.get())
}

#[tauri::command]
async fn update_config(
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<(), String> {
    state
        .config_manager
        .update(config)
        .map_err(|e| e.to_string())?;
    state.scheduler.stop_all().await;
    state.scheduler.start_all(false).await;
    Ok(())
}

#[tauri::command]
async fn get_stats(state: tauri::State<'_, AppState>) -> Result<db::DashboardStats, String> {
    let mut stats = state.db.get_stats().map_err(|e| e.to_string())?;
    let config = state.config_manager.get();
    stats.total_topics = config.topics.len();
    Ok(stats)
}

#[tauri::command]
async fn get_topics(state: tauri::State<'_, AppState>) -> Result<Vec<TopicConfig>, String> {
    let config = state.config_manager.get();
    Ok(config.topics)
}

#[tauri::command]
async fn get_topic_stats(state: tauri::State<'_, AppState>) -> Result<Vec<db::TopicStat>, String> {
    state.db.get_topic_stats().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_posts(
    state: tauri::State<'_, AppState>,
    topic_id: String,
    limit: i64,
) -> Result<Vec<db::PostSummary>, String> {
    if topic_id.is_empty() {
        state.db.get_all_posts(limit).map_err(|e| e.to_string())
    } else {
        state.db.get_posts(&topic_id, limit).map_err(|e| e.to_string())
    }
}

/// トピック詳細画面用: OMP セッション 1 件分のプレビュー
#[derive(serde::Serialize)]
struct TopicDetailSession {
    created_at: String,
    prompt_preview: String,
    response_preview: String,
    /// プロンプト全文
    prompt_full: String,
    /// 回答全文
    response_full: String,
    /// 思考ログ（--thinking が記録されていれば）
    thinking_full: String,
}

/// トピック詳細画面用のデータ（設定・投稿ニュース・セッション履歴）
#[derive(serde::Serialize)]
struct TopicDetail {
    config: TopicConfig,
    posts: Vec<db::PostSummary>,
    sessions: Vec<TopicDetailSession>,
}

#[tauri::command]
async fn get_topic_detail(
    state: tauri::State<'_, AppState>,
    topic_name: String,
) -> Result<TopicDetail, String> {
    let config = state.config_manager.get();
    let topic = config
        .topics
        .iter()
        .find(|t| t.name == topic_name)
        .cloned()
        .ok_or_else(|| format!("トピック '{}' が見つかりません", topic_name))?;
    // 投稿・セッションの読み込みは失敗してもアプリを止めない（空配列で返す）
    let posts = state.db.get_posts(&topic_name, 10).unwrap_or_default();
    let sessions = read_topic_sessions(&topic_name);
    Ok(TopicDetail { config: topic, posts, sessions })
}

/// %TEMP%\my-quick-feed\omp\omp-sessions\*.jsonl から該当トピックのセッション履歴を読み込む。
/// 1ファイル = 1セッション。最初の user メッセージをプロンプト、最後の assistant メッセージを回答とする。
/// プロンプトに「## トピック\n<topic_name>」が無いファイル（別トピック・プローブ等）は無視する。
fn read_topic_sessions(topic_name: &str) -> Vec<TopicDetailSession> {
    let dir = std::env::temp_dir()
        .join("my-quick-feed")
        .join("omp")
        .join("omp-sessions");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut found: Vec<(String, TopicDetailSession)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let mut first_user_ts: Option<String> = None;
        let mut first_user_prompt: Option<String> = None;
        let mut last_assistant: Option<String> = None;
        // 思考ログ（assistant の thinking パーツを連結）
        let mut thinking_parts: Vec<String> = Vec::new();
        for line in content.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if value.get("type").and_then(|t| t.as_str()) != Some("message") {
                continue;
            }
            let Some(message) = value.get("message") else { continue };
            let Some(role) = message.get("role").and_then(|r| r.as_str()) else {
                continue;
            };
            let ts = value
                .get("timestamp")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let text = message_text(message.get("content"));
            // thinking / reasoning パーツの抽出
            // （omp の thinking パーツは text ではなく thinking フィールドに本文が入る。
            //   Claude 形式（text フィールド）も併せて読む）
            if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                for part in content {
                    let kind = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if kind == "thinking" || kind == "reasoning" || kind == "thinking_delta" {
                        let t = part
                            .get("text")
                            .or_else(|| part.get("thinking"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if !t.trim().is_empty() {
                            thinking_parts.push(t.to_string());
                        }
                    }
                }
            }
            match role {
                "user" => {
                    if first_user_prompt.is_none() {
                        first_user_ts = Some(ts);
                        first_user_prompt = Some(text);
                    }
                }
                "assistant" => {
                    // ツール呼び出し等の途中メッセージを避け、テキストを持つ最後の回答を採用する
                    if !text.trim().is_empty() {
                        last_assistant = Some(text);
                    }
                }
                _ => {}
            }
        }
        let Some(prompt) = first_user_prompt else { continue };
        if !prompt_belongs_to_topic(&prompt, topic_name) {
            continue;
        }
        let created_at = first_user_ts
            .as_deref()
            .map(local_time_string)
            .unwrap_or_default();
        let response = last_assistant.unwrap_or_default();
        let thinking = thinking_parts.join("\n---\n");
        found.push((
            first_user_ts.unwrap_or_default(),
            TopicDetailSession {
                created_at,
                prompt_preview: truncate_preview(&prompt),
                response_preview: truncate_preview(&response),
                prompt_full: strip_file_wrapper(&prompt),
                response_full: response,
                thinking_full: thinking,
            },
        ));
    }
    // 日付順（新しい順）に最大10件
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.truncate(10);
    found.into_iter().map(|(_, s)| s).collect()
}

/// omp がプロンプトに付ける `<file name="...">` ラッパーを除去する
fn strip_file_wrapper(s: &str) -> String {
    let t = s.trim();
    if t.starts_with("<file name=") {
        if let Some(end) = t.find('>') {
            return t[end + 1..].trim().to_string();
        }
    }
    t.to_string()
}

/// message.content（文字列 or パーツ配列）からテキストを抽出する。
/// thinking / toolCall パーツは text フィールドを持たないため自動的に除外される。
fn message_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// プロンプトが「## トピック\n<topic_name>」で始まる行を含むか判定する（名前の前方一致誤爆防止）
fn prompt_belongs_to_topic(prompt: &str, topic_name: &str) -> bool {
    let needle = format!("## トピック\n{}", topic_name);
    let Some(idx) = prompt.find(&needle) else {
        return false;
    };
    match prompt[idx + needle.len()..].chars().next() {
        // トピック名の直後は改行（または文末）であること
        Some(c) => c == '\n' || c == '\r',
        None => true,
    }
}

/// omp のタイムスタンプ（ISO8601 UTC）をローカル時刻の表示用文字列に変換する
fn local_time_string(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| ts.to_string())
}

/// プレビュー用: omp が付ける <file name="..."> ラッパーを除いて先頭 200 文字程度に切り詰める
fn truncate_preview(text: &str) -> String {
    const MAX: usize = 200;
    let trimmed = text.trim_start();
    let body = if let Some(rest) = trimmed.strip_prefix("<file ") {
        if let Some((_, body)) = rest.split_once('\n') {
            body.trim_start()
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    let mut chars = body.chars();
    let head: String = chars.by_ref().take(MAX).collect();
    if chars.next().is_some() {
        format!("{}…", head)
    } else {
        head
    }
}

#[tauri::command]
async fn refresh_topic(
    state: tauri::State<'_, AppState>,
    topic_name: String,
) -> Result<String, String> {
    let ts = chrono::Local::now().format("%H:%M:%S").to_string();
    info!(topic = %topic_name, "Manual refresh triggered");
    state
        .scheduler
        .refresh_topic(&topic_name)
        .await
        .map_err(|e| {
            error!(topic = %topic_name, "Refresh failed: {}", e);
            format!("{}", e)
        })?;
    info!(topic = %topic_name, "Refresh completed");
    Ok(ts)
}

#[tauri::command]
async fn get_logs() -> Result<Vec<LogEntry>, String> {
    Ok(read_logs())
}

#[tauri::command]
async fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    if enabled {
        autostart.enable().map_err(|e| e.to_string())?;
    } else {
        autostart.disable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct AppStatus {
    running: bool,
    /// トピック名 → 次回実行予定時刻（ISO8601、実行なしは null）
    topic_next_fetch: std::collections::HashMap<String, String>,
    /// 現在パイプライン実行中のトピック名一覧
    topic_running: Vec<String>,
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<AppStatus, String> {
    let running = state.running.load(std::sync::atomic::Ordering::SeqCst);
    let next_runs = state.scheduler.get_next_runs();
    let topic_next_fetch = next_runs
        .into_iter()
        .map(|(k, v)| (k, v.to_rfc3339()))
        .collect();
    let topic_running = state.scheduler.get_running_topics().await;
    Ok(AppStatus { running, topic_next_fetch, topic_running })
}

#[tauri::command]
async fn clear_logs() -> Result<(), String> {
    LOG_BUFFER.lock().clear();
    Ok(())
}


#[tauri::command]
async fn export_logs(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let logs = read_logs();
    if logs.is_empty() {
        return Err("No logs to export".into());
    }

    std::fs::create_dir_all(&state.log_dir).map_err(|e| e.to_string())?;

    let filename = format!("mqf_{}.log", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let path = state.log_dir.join(&filename);

    let mut content = String::new();
    for entry in &logs {
        content.push_str(&format!(
            "[{}] [{}] [{}] {}\n",
            entry.timestamp, entry.level, entry.topic, entry.message
        ));
    }

    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}
/// ===== App Entry Point =====

/// 起動オプション（main.rs でパースして渡す）
#[derive(Debug, Clone, Default)]
pub struct CliArgs {
    /// false なら Discord 投稿・DB 保存をしない（--no-post）
    pub post_enabled: bool,
    /// ログをコンソールにも出力（--console）
    pub console: bool,
    /// スケジューラを起動せず、パイプラインを 1 回実行して終了（--run-once）
    pub run_once: bool,
    /// --run-once 時に実行するトピック名（省略時は全トピック）
    pub topic_filter: Option<String>,
    /// 起動時の即実行をスキップする（--no-run）
    pub no_initial_run: bool,
    /// 設定ファイルパス（--config、省略時は %APPDATA% のデフォルト）
    pub config_path: Option<std::path::PathBuf>,
    /// ログレベルを debug に上げる（--verbose）
    pub verbose: bool,
}

/// 設定ファイルの `cli:` セクションを読み込む（起動オプションのデフォルト用・main.rs から呼ぶ）。
/// ファイルが無い・壊れている場合はデフォルト（全て off）を返す。
pub fn load_config_cli(path: &std::path::Path) -> AppResult<config::CliConfig> {
    Ok(config::AppConfig::load(&path.to_path_buf())?.cli)
}

pub fn run(cli: CliArgs) {
    // tracing subscriber: stdout + ファイル + LogCaptureLayer
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_level(true);
    let capture_layer = LogCaptureLayer;

    // ログをファイルにも常時書き出す（%APPDATA%\com.myquickfeed.app\logs\app.log）
    // 注意: non_blocking はバッファ満杯でブロックしアプリ全体が止まるため、同期書き込みを使う
    let log_dir = std::env::var("APPDATA")
        .map(|a| std::path::PathBuf::from(a).join("com.myquickfeed.app").join("logs"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    std::fs::create_dir_all(&log_dir).ok();
    // ログファイルを開けない場合（ロック・パーミッション等）も起動を継続する
    let file_layer = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("app.log"))
        .ok()
        .map(|file| {
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_writer(file)
        });
    if file_layer.is_none() {
        eprintln!("[my-quick-feed] WARN: failed to open log file, continuing without file logging");
    }

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .with(capture_layer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                if cli.verbose {
                    "my_quick_feed=debug".into()
                } else {
                    "my_quick_feed=info".into()
                }
            }),
        )
        .init();

    info!("Log file: {}", log_dir.join("app.log").display());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // 多重起動防止: 2つ目のインスタンスは即終了し、既存ウィンドウを表示
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                window.show().ok();
                window.set_focus().ok();
            }
        }))
        // Windowsスタートアップ登録
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(move |app| {
            info!("Starting My Quick Feed...");

            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir");

            // --config 指定があればそのパスを使う（無ければ %APPDATA% のデフォルト）
            let config_path = cli
                .config_path
                .clone()
                .unwrap_or_else(|| app_data_dir.join("my-quick-feed.yaml"));
            if cli.config_path.is_some() {
                info!("設定ファイル: {:?}（--config 指定）", config_path);
            }
            let config_manager =
                Arc::new(ConfigManager::load(config_path).expect("Failed to load config"));

            let db_path = app_data_dir.join("my-quick-feed.db");
            let db = Arc::new(Database::new(db_path).expect("Failed to initialize database"));

            let config = config_manager.get();
            let discord = if !config.discord.token.is_empty()
                && !config.discord.forum_channel_id.is_empty()
            {
                let client = DiscordClient::new(&config.discord.token);
                let client_arc = Arc::new(client);

                let verify_client = client_arc.clone();
                tauri::async_runtime::spawn(async move {
                    match verify_client.verify_token().await {
                        Ok(_) => info!("Discord client verified"),
                        Err(e) => error!("Discord verification failed: {}", e),
                    }
                });

                info!("Discord client created (verification in background)");
                Some(client_arc)
            } else {
                info!("Discord not configured");
                None
            };

            let pipeline = Arc::new(Pipeline::new(
                config_manager.clone(),
                db.clone(),
                discord,
                cli.post_enabled,
            ));

            let scheduler = Arc::new(Scheduler::new(config_manager.clone(), pipeline.clone()));

            let state = AppState {
                config_manager: config_manager.clone(),
                db: db.clone(),
                scheduler: scheduler.clone(),
                running: AtomicBool::new(true),
                log_dir: app_data_dir.join("logs"),
            };

            app.manage(state);

            if cli.run_once {
                // --run-once: スケジューラを起動せず、パイプラインを 1 回実行して終了
                let pipeline = pipeline.clone();
                let config_manager = config_manager.clone();
                let topic_filter = cli.topic_filter.clone();
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let config = config_manager.get();
                    let topics: Vec<TopicConfig> = match &topic_filter {
                        Some(name) => config
                            .topics
                            .into_iter()
                            .filter(|t| &t.name == name)
                            .collect(),
                        None => config.topics,
                    };
                    if topics.is_empty() {
                        if let Some(name) = &topic_filter {
                            error!("--run-once: トピック '{}' が見つかりません", name);
                        } else {
                            info!("--run-once: 実行対象トピックなし");
                        }
                    }
                    for topic in &topics {
                        info!(topic = %topic.name, "--run-once: 実行開始");
                        if let Err(e) = pipeline.run(topic).await {
                            error!(topic = %topic.name, "--run-once: 実行失敗: {}", e);
                        }
                    }
                    info!("--run-once: 全実行完了、終了します");
                    app_handle.exit(0);
                });
            } else {
                let scheduler_clone = scheduler.clone();
                tauri::async_runtime::spawn(async move {
                    scheduler_clone.start_all(cli.no_initial_run).await;
                });
            }

            // System tray
            let tray = TrayIconBuilder::new()
                .tooltip("My Quick Feed")
                .on_menu_event(|app, event| {
                    if event.id() == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            window.show().ok();
                            window.set_focus().ok();
                        }
                    } else if event.id() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            let submenu = SubmenuBuilder::new(app, "File")
                .text("show", "Show Window")
                .quit()
                .build()?;

            let menu = MenuBuilder::new(app).items(&[&submenu]).build()?;
            tray.set_menu(Some(menu))?;

            info!("My Quick Feed started successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            update_config,
            get_stats,
            get_topics,
            get_topic_stats,
            get_posts,
            get_topic_detail,
            refresh_topic,
            get_logs,
            export_logs,
            set_autostart,
            get_autostart,
            get_status,
            clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
