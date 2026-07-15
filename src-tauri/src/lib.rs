mod ai;
mod config;
mod db;
mod discord;
mod errors;
mod fetcher;
mod pipeline;
mod scheduler;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    tray::TrayIconBuilder,
    Manager,
};
use tracing::{error, info};

use config::{AppConfig, ConfigManager, TopicConfig};
use db::{Database, LogEntry};
use discord::DiscordClient;
use pipeline::Pipeline;
use scheduler::Scheduler;

/// アプリケーション状態
pub struct AppState {
    pub config_manager: Arc<ConfigManager>,
    pub db: Arc<Database>,
    pub scheduler: Arc<Scheduler>,
    pub running: AtomicBool,
    pub log_buffer: tokio::sync::Mutex<Vec<LogEntry>>,
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
    // Restart scheduler
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
async fn get_posts(
    state: tauri::State<'_, AppState>,
    topic_id: String,
    limit: i64,
) -> Result<Vec<db::PostSummary>, String> {
    state
        .db
        .get_posts(&topic_id, limit)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn refresh_topic(
    state: tauri::State<'_, AppState>,
    topic_name: String,
) -> Result<(), String> {
    state
        .scheduler
        .refresh_topic(&topic_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_logs(state: tauri::State<'_, AppState>) -> Result<Vec<LogEntry>, String> {
    let logs = state.log_buffer.lock().await;
    Ok(logs.clone())
}

/// ===== App Entry Point =====

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "my_quick_feed=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            info!("Starting My Quick Feed...");

            // Resolve data paths
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir");

            // Config path
            let config_path = app_data_dir.join("my-quick-feed.yaml");
            let config_manager =
                Arc::new(ConfigManager::load(config_path).expect("Failed to load config"));

            // DB path
            let db_path = app_data_dir.join("my-quick-feed.db");
            let db = Arc::new(Database::new(db_path).expect("Failed to initialize database"));

            // Discord client — verify in background, store regardless
            let config = config_manager.get();
            let discord = if !config.discord.token.is_empty()
                && !config.discord.forum_channel_id.is_empty()
            {
                let client = DiscordClient::new(&config.discord.token);
                let client_arc = Arc::new(client);

                // Verify token in background
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
                info!("Discord not configured — skipping client initialization");
                None
            };

            // Pipeline
            let pipeline = Arc::new(Pipeline::new(
                config_manager.clone(),
                db.clone(),
                discord,
            ));

            // Scheduler
            let scheduler = Arc::new(Scheduler::new(config_manager.clone(), pipeline.clone()));

            // App state
            let state = AppState {
                config_manager: config_manager.clone(),
                db: db.clone(),
                scheduler: scheduler.clone(),
                running: AtomicBool::new(true),
                log_buffer: tokio::sync::Mutex::new(Vec::new()),
            };

            app.manage(state);

            // Start scheduler
            let scheduler_clone = scheduler.clone();
            tauri::async_runtime::spawn(async move {
                scheduler_clone.start_all().await;
            });

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
            get_posts,
            refresh_topic,
            get_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
