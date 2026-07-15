use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info, warn};

use crate::config::{ConfigManager, TopicConfig};
use crate::errors::AppResult;
use crate::pipeline::Pipeline;

pub struct Scheduler {
    handles: AsyncMutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    config_manager: Arc<ConfigManager>,
    pipeline: Arc<Pipeline>,
}

impl Scheduler {
    pub fn new(config_manager: Arc<ConfigManager>, pipeline: Arc<Pipeline>) -> Self {
        Self {
            handles: AsyncMutex::new(HashMap::new()),
            config_manager,
            pipeline,
        }
    }

    /// 全てのトピックのスケジューラを起動
    pub async fn start_all(&self) {
        let config = self.config_manager.get();
        let topics = config.topics.clone();

        for topic in &topics {
            self.start_topic(topic).await;
        }
        info!("Started {} topic schedulers", topics.len());
    }

    /// 1トピックのスケジューラを起動
    pub async fn start_topic(&self, topic: &TopicConfig) {
        let mut handles = self.handles.lock().await;
        // Stop existing handle if any
        if let Some(handle) = handles.remove(&topic.name) {
            handle.abort();
        }

        let topic_name = topic.name.clone();
        let interval_min = topic.interval_min.max(1); // min 1 minute
        let pipeline = self.pipeline.clone();
        let config_manager = self.config_manager.clone();

        let handle = tokio::spawn(async move {
            let mut timer = interval(std::time::Duration::from_secs(interval_min * 60));
            timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

            info!(
                "Scheduler started for '{}' (interval: {} min)",
                topic_name, interval_min
            );

            // Run immediately on start
            Self::run_topic_pipeline(&pipeline, &config_manager, &topic_name).await;

            loop {
                timer.tick().await;
                Self::run_topic_pipeline(&pipeline, &config_manager, &topic_name).await;
            }
        });

        handles.insert(topic.name.clone(), handle);
    }

    async fn run_topic_pipeline(
        pipeline: &Arc<Pipeline>,
        config_manager: &Arc<ConfigManager>,
        topic_name: &str,
    ) {
        let config = config_manager.get();
        let topic = config.topics.iter().find(|t| t.name == topic_name);

        if let Some(topic) = topic {
            info!("Pipeline triggered for topic '{}'", topic_name);
            if let Err(e) = pipeline.run(topic).await {
                error!("Pipeline failed for topic '{}': {}", topic_name, e);
                // エラーが発生しても後続のトピックは続行
            }
        } else {
            warn!("Topic '{}' not found in config", topic_name);
        }
    }

    /// 特定トピックのスケジューラを停止
    pub async fn stop_topic(&self, topic_name: &str) {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.remove(topic_name) {
            handle.abort();
            info!("Scheduler stopped for topic '{}'", topic_name);
        }
    }

    /// 全て停止
    pub async fn stop_all(&self) {
        let mut handles = self.handles.lock().await;
        for (name, handle) in handles.drain() {
            handle.abort();
            info!("Scheduler stopped for topic '{}'", name);
        }
    }

    /// 手動リフレッシュ（即時実行）
    pub async fn refresh_topic(&self, topic_name: &str) -> AppResult<()> {
        let config = self.config_manager.get();
        let topic = config
            .topics
            .iter()
            .find(|t| t.name == topic_name)
            .cloned();

        match topic {
            Some(t) => {
                self.pipeline.run(&t).await?;
                Ok(())
            }
            None => Err(crate::errors::AppError::Other(format!(
                "Topic '{}' not found",
                topic_name
            ))),
        }
    }
}
