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
    state.scheduler.start_all().await;
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
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<AppStatus, String> {
    let running = state.running.load(std::sync::atomic::Ordering::SeqCst);
    let next_runs = state.scheduler.get_next_runs();
    let topic_next_fetch = next_runs
        .into_iter()
        .map(|(k, v)| (k, v.to_rfc3339()))
        .collect();
    Ok(AppStatus { running, topic_next_fetch })
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
    /// 設定ファイルパス（--config、省略時は %APPDATA% のデフォルト）
    pub config_path: Option<std::path::PathBuf>,
    /// ログレベルを debug に上げる（--verbose）
    pub verbose: bool,
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
                    scheduler_clone.start_all().await;
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
