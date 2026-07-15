use std::sync::Arc;
use tracing::{error, info, warn};
use crate::ai::{call_direct_api, run_agent};
use crate::config::{ConfigManager, TopicConfig};
use crate::db::Database;
use crate::discord::DiscordClient;
use crate::errors::{AppError, AppResult};
use crate::fetcher::{fetch_feed, fetch_rsshub, FeedItem, DEFAULT_RSSHUB_URL};

pub struct Pipeline {
    config_manager: Arc<ConfigManager>,
    db: Arc<Database>,
    discord: Option<Arc<DiscordClient>>,
}

impl Pipeline {
    pub fn new(
        config_manager: Arc<ConfigManager>,
        db: Arc<Database>,
        discord: Option<Arc<DiscordClient>>,
    ) -> Self {
        Self {
            config_manager,
            db,
            discord,
        }
    }

    /// 1トピックのパイプラインを実行
    pub async fn run(&self, topic: &TopicConfig) -> AppResult<()> {
        let tn = &topic.name;
        info!(topic = %tn, "Pipeline started");

        // Step 1: Fetch feeds
        let mut all_items: Vec<FeedItem> = Vec::new();
        for source in &topic.sources {
            let items = match source.source_type.as_str() {
                "rss" => match fetch_feed(&source.url).await {
                    Ok(feed) => feed.items,
                    Err(e) => {
                        error!(topic = %tn, "RSS fetch failed: {} — {}", source.url, e);
                        continue;
                    }
                },
                "rsshub" => match fetch_rsshub(DEFAULT_RSSHUB_URL, &source.url).await {
                    Ok(feed) => feed.items,
                    Err(e) => {
                        error!(topic = %tn, "RSSHUB fetch failed: {} — {}", source.url, e);
                        continue;
                    }
                },
                other => {
                    warn!(topic = %tn, "Unknown source type: {}", other);
                    continue;
                }
            };
            all_items.extend(items);
        }

        if all_items.is_empty() {
            info!(topic = %tn, "No items fetched from any source");
            return Ok(());
        }
        info!(topic = %tn, "Fetched {} items total", all_items.len());

        // Step 2: Deduplicate
        let new_items: Vec<FeedItem> = all_items
            .into_iter()
            .filter(|item| match self.db.is_seen(tn, &item.link) {
                Ok(false) => true,
                Ok(true) => false,
                Err(e) => {
                    warn!(topic = %tn, "DB dedup check error: {}", e);
                    true
                }
            })
            .collect();

        if new_items.is_empty() {
            info!(topic = %tn, "All items already seen — nothing new");
            return Ok(());
        }
        info!(topic = %tn, "{} new items to process", new_items.len());

        // Step 3: AI processing
        let config = self.config_manager.get();
        let article_result = match config.ai.mode.as_str() {
            "direct" => {
                let api_key = config
                    .ai
                    .api_key
                    .as_deref()
                    .ok_or_else(|| AppError::Config("API key not configured for direct mode".into()))?;
                let model = config
                    .ai
                    .model
                    .as_deref()
                    .unwrap_or("gpt-4o-mini");
                let base_url = config
                    .ai
                    .base_url
                    .as_deref()
                    .unwrap_or("https://openrouter.ai/api/v1");

                info!(topic = %tn, "Calling Direct API [model={}]", model);
                call_direct_api(api_key, model, base_url, topic, &new_items).await?
            }
            _ => {
                let command = config
                    .ai
                    .agent_command
                    .as_deref()
                    .unwrap_or("omp");
                let model = config.ai.model.as_deref().unwrap_or("default");
                let timeout = config.ai.agent_timeout_sec.unwrap_or(120);

                info!(topic = %tn, "Running agent [cmd={}, model={}, timeout={}s]", command, model, timeout);
                run_agent(command, model, timeout, topic, &new_items).await?
            }
        };

        // Mark seen
        for item in &new_items {
            if let Err(e) = self.db.mark_seen(tn, &item.link, Some(&item.title)) {
                warn!(topic = %tn, "Failed to mark seen: {}", e);
            }
        }
        info!(topic = %tn, "Article generated: \"{}\"", article_result.title);

        // Step 4: Post to Discord
        if let Some(discord) = &self.discord {
            let discord_config = config.discord.clone();
            let image_url = article_result.image_url.as_deref();

            let existing_thread = self.db.get_topic_thread(tn).ok().flatten();

            let discord_result = match &existing_thread {
                Some(thread_id) => {
                    info!(topic = %tn, "Posting to existing thread {}", thread_id);
                    discord
                        .post_to_thread(thread_id, &article_result.title, &article_result.content, image_url)
                        .await
                        .map(|mid| (mid, thread_id.clone()))
                }
                None => {
                    info!(topic = %tn, "Creating new forum thread");
                    discord
                        .post_to_forum(&discord_config, &article_result.title, &article_result.content, image_url)
                        .await
                        .map(|(mid, tid)| {
                            self.db.set_topic_thread(tn, &tid).ok();
                            (mid, tid)
                        })
                }
            };

            match discord_result {
                Ok((message_id, thread_id)) => {
                    match self.db.insert_post(tn, &article_result.title, &article_result.content, image_url) {
                        Ok(post_id) => {
                            self.db.update_post_discord(post_id, &message_id, Some(&thread_id)).ok();
                            info!(topic = %tn, "Posted to Discord: post_id={}, thread={}", post_id, thread_id);
                        }
                        Err(e) => error!(topic = %tn, "Failed to save post to DB: {}", e),
                    }
                }
                Err(e) => {
                    error!(topic = %tn, "Discord post failed: {}", e);
                    self.db.insert_post(tn, &article_result.title, &article_result.content, image_url).ok();
                }
            }
        } else {
            self.db
                .insert_post(tn, &article_result.title, &article_result.content, article_result.image_url.as_deref())
                .ok();
            info!(topic = %tn, "Saved to DB (Discord not configured)");
        }

        info!(topic = %tn, "Pipeline completed");
        Ok(())
    }
}
