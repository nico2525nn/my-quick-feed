use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Local};
use parking_lot::Mutex as SyncMutex;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{error, info, warn};

use crate::config::{ConfigManager, TopicConfig};
use crate::errors::{AppError, AppResult};
use crate::pipeline::Pipeline;

pub struct Scheduler {
    handles: AsyncMutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// トピックごとの次回実行予定時刻（Dashboard 表示用）
    next_runs: Arc<SyncMutex<HashMap<String, DateTime<Local>>>>,
    /// 実行中のトピック一覧（同一トピックの並行実行・二重投稿防止）
    running: Arc<AsyncMutex<HashMap<String, ()>>>,
    config_manager: Arc<ConfigManager>,
    pipeline: Arc<Pipeline>,
}

impl Scheduler {
    pub fn new(config_manager: Arc<ConfigManager>, pipeline: Arc<Pipeline>) -> Self {
        Self {
            handles: AsyncMutex::new(HashMap::new()),
            next_runs: Arc::new(SyncMutex::new(HashMap::new())),
            running: Arc::new(AsyncMutex::new(HashMap::new())),
            config_manager,
            pipeline,
        }
    }

    pub async fn start_all(&self) {
        let config = self.config_manager.get();
        let topics = config.topics.clone();
        for topic in &topics {
            self.start_topic(topic).await;
        }
        info!("Started {} topic schedulers", topics.len());
    }

    /// トピックごとの次回実行予定時刻を取得（Dashboard 用）
    pub fn get_next_runs(&self) -> HashMap<String, DateTime<Local>> {
        self.next_runs.lock().clone()
    }

    /// 現在パイプライン実行中のトピック名一覧（Dashboard の実行中表示用）
    pub async fn get_running_topics(&self) -> Vec<String> {
        self.running.lock().await.keys().cloned().collect()
    }

    pub async fn start_topic(&self, topic: &TopicConfig) {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.remove(&topic.name) {
            handle.abort();
        }

        let topic_name = topic.name.clone();
        let interval_min = topic.interval_min.max(1);
        let pipeline = self.pipeline.clone();
        let config_manager = self.config_manager.clone();
        let next_runs = self.next_runs.clone();
        let running = self.running.clone();

        let handle = tokio::spawn(async move {
            info!(topic = %topic_name, "Scheduler started [interval={}min]", interval_min);

            // 起動時即実行（1回のみ）
            run_topic_pipeline(&pipeline, &config_manager, &running, &topic_name).await;

            // 実行完了後に interval を計測する方式（実行が interval より長い場合の
            // 連続実行と、次回予定時刻のズレを防ぐ）
            loop {
                let next = Local::now() + chrono::Duration::minutes(interval_min as i64);
                next_runs.lock().insert(topic_name.clone(), next);
                tokio::time::sleep(std::time::Duration::from_secs(interval_min * 60)).await;
                run_topic_pipeline(&pipeline, &config_manager, &running, &topic_name).await;
            }
        });

        handles.insert(topic.name.clone(), handle);
    }

    pub async fn stop_topic(&self, topic_name: &str) {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.remove(topic_name) {
            handle.abort();
            self.next_runs.lock().remove(topic_name);
            info!(topic = %topic_name, "Scheduler stopped");
        }
    }

    pub async fn stop_all(&self) {
        let mut handles = self.handles.lock().await;
        for (name, handle) in handles.drain() {
            handle.abort();
            self.next_runs.lock().remove(&name);
            info!(topic = %name, "Scheduler stopped");
        }
    }

    pub async fn refresh_topic(&self, topic_name: &str) -> AppResult<()> {
        let config = self.config_manager.get();
        let topic = config
            .topics
            .iter()
            .find(|t| t.name == topic_name)
            .cloned()
            .ok_or_else(|| {
                AppError::Other(format!("Topic '{}' not found", topic_name))
            })?;

        // 同一トピックが実行中なら失敗を返す（二重投稿防止）
        {
            let mut running = self.running.lock().await;
            if running.contains_key(topic_name) {
                return Err(AppError::Other(format!(
                    "Topic '{}' is already running",
                    topic_name
                )));
            }
            running.insert(topic_name.to_string(), ());
        }

        let result = self.pipeline.run(&topic).await;
        self.running.lock().await.remove(topic_name);
        result
    }
}

/// トピックのパイプラインを実行する（同一トピックの並行実行はスキップ）
async fn run_topic_pipeline(
    pipeline: &Arc<Pipeline>,
    config_manager: &Arc<ConfigManager>,
    running: &AsyncMutex<HashMap<String, ()>>,
    topic_name: &str,
) {
    {
        let mut guard = running.lock().await;
        if guard.contains_key(topic_name) {
            info!(topic = %topic_name, "Pipeline already running, skipping");
            return;
        }
        guard.insert(topic_name.to_string(), ());
    }

    let config = config_manager.get();
    if let Some(topic) = config.topics.iter().find(|t| t.name == topic_name) {
        info!(topic = %topic_name, "Pipeline triggered");
        if let Err(e) = pipeline.run(topic).await {
            error!(topic = %topic_name, "Pipeline failed: {}", e);
        }
    } else {
        warn!(topic = %topic_name, "Topic not found in config");
    }

    running.lock().await.remove(topic_name);
}

