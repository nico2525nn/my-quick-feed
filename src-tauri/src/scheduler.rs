use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info, warn};

use crate::config::{ConfigManager, TopicConfig};
use crate::errors::AppResult;
use crate::pipeline::Pipeline;
use crate::ai::agent::cleanup_omp_sessions;

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

    pub async fn start_all(&self) {
        let config = self.config_manager.get();
        let topics = config.topics.clone();
        // 起動時に OMP セッションをクリーンアップ（履歴汚染防止）
        cleanup_omp_sessions();
        for topic in &topics {
            self.start_topic(topic).await;
        }
        info!("Started {} topic schedulers", topics.len());
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

        let handle = tokio::spawn(async move {
            let mut timer = interval(std::time::Duration::from_secs(interval_min * 60));
            timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

            info!(topic = %topic_name, "Scheduler started [interval={}min]", interval_min);

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
        if let Some(topic) = config.topics.iter().find(|t| t.name == topic_name) {
            info!(topic = %topic_name, "Pipeline triggered");
            // パイプライン実行前にもセッションクリーンアップ
            cleanup_omp_sessions();
            if let Err(e) = pipeline.run(topic).await {
                error!(topic = %topic_name, "Pipeline failed: {}", e);
            }
        } else {
            warn!(topic = %topic_name, "Topic not found in config");
        }
    }

    pub async fn stop_topic(&self, topic_name: &str) {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.remove(topic_name) {
            handle.abort();
            info!(topic = %topic_name, "Scheduler stopped");
        }
    }

    pub async fn stop_all(&self) {
        let mut handles = self.handles.lock().await;
        for (name, handle) in handles.drain() {
            handle.abort();
            info!(topic = %name, "Scheduler stopped");
        }
    }

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
