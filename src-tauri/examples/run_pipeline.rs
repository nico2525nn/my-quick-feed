//! パイプラインテスト用バイナリ（アプリGUIを立ち上げずに実行）
//!
//! 使い方: cargo run --example run_pipeline [config_path]
//!  - config_path 省略時は %APPDATA%\com.myquickfeed.app\my-quick-feed.yaml を使用
//!
//! 注意: 実際の Discord に投稿されます。

use my_quick_feed_lib::config::ConfigManager;
use my_quick_feed_lib::db::Database;
use my_quick_feed_lib::discord::DiscordClient;
use my_quick_feed_lib::pipeline::Pipeline;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "my_quick_feed=info".into()),
        )
        .init();

    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let base = PathBuf::from(appdata).join("com.myquickfeed.app");

    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| base.join("my-quick-feed.yaml"));

    let config_manager = Arc::new(ConfigManager::load(config_path).expect("Failed to load config"));
    let config = config_manager.get();

    let db = Arc::new(
        Database::new(base.join("my-quick-feed.db")).expect("Failed to init DB"),
    );

    let discord = if !config.discord.token.is_empty() && !config.discord.forum_channel_id.is_empty()
    {
        Some(Arc::new(DiscordClient::new(&config.discord.token)))
    } else {
        None
    };

    let pipeline = Pipeline::new(config_manager.clone(), db, discord);

    println!("=== Topics: {} 個 ===", config.topics.len());
    for topic in &config.topics {
        println!("--- Running pipeline for topic: {} ---", topic.name);
        match pipeline.run(topic).await {
            Ok(()) => println!("OK: {}", topic.name),
            Err(e) => println!("FAIL: {}: {}", topic.name, e),
        }
    }
    println!("=== Done ===");
}
