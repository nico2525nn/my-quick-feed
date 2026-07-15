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
    /// 1. 各ソースからフィード取得
    /// 2. 重複排除
    /// 3. AI処理（Agent又はDirect）
    /// 4. Discord投稿
    /// 5. DB更新
    pub async fn run(&self, topic: &TopicConfig) -> AppResult<()> {
        info!("Pipeline started for topic '{}'", topic.name);

        // Step 1: Fetch feeds from all sources
        let mut all_items: Vec<FeedItem> = Vec::new();
        for source in &topic.sources {
            let items = match source.source_type.as_str() {
                "rss" => match fetch_feed(&source.url).await {
                    Ok(feed) => feed.items,
                    Err(e) => {
                        error!("Failed to fetch RSS '{}': {}", source.url, e);
                        continue; // 1ソースが落ちても続行
                    }
                },
                "rsshub" => match fetch_rsshub(DEFAULT_RSSHUB_URL, &source.url).await {
                    Ok(feed) => feed.items,
                    Err(e) => {
                        error!("Failed to fetch RSSHUB '{}': {}", source.url, e);
                        continue;
                    }
                },
                other => {
                    warn!("Unknown source type: {}", other);
                    continue;
                }
            };
            all_items.extend(items);
        }

        if all_items.is_empty() {
            info!("No new items found for topic '{}'", topic.name);
            return Ok(());
        }

        info!(
            "Fetched {} total items for topic '{}'",
            all_items.len(),
            topic.name
        );

        // Step 2: Deduplicate
        let new_items: Vec<FeedItem> = all_items
            .into_iter()
            .filter(|item| {
                let url = &item.link;
                match self.db.is_seen(&topic.name, url) {
                    Ok(false) => true,
                    Ok(true) => false,
                    Err(e) => {
                        warn!("DB error checking seen item: {}", e);
                        true
                    }
                }
            })
            .collect();

        if new_items.is_empty() {
            info!("No new (unseen) items for topic '{}'", topic.name);
            return Ok(());
        }

        info!(
            "{} new items to process for topic '{}'",
            new_items.len(),
            topic.name
        );

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

                call_direct_api(api_key, model, base_url, topic, &new_items).await?
            }
            _ => {
                // Default: agent mode
                let command = config
                    .ai
                    .agent_command
                    .as_deref()
                    .unwrap_or("omp");
                let timeout = config.ai.agent_timeout_sec.unwrap_or(120);

                run_agent(command, timeout, topic, &new_items).await?
            }
        };

        // Mark items as seen
        for item in &new_items {
            if let Err(e) = self
                .db
                .mark_seen(&topic.name, &item.link, Some(&item.title))
            {
                warn!("Failed to mark item as seen: {}", e);
            }
        }

        // Step 4: Post to Discord (1 topic = 1 forum thread,追記方式)
        if let Some(discord) = &self.discord {
            let discord_config = config.discord.clone();
            let image_url = article_result.image_url.as_deref();
            let topic_name = &topic.name;

            // 既存スレッドを確認
            let existing_thread = self.db.get_topic_thread(topic_name).ok().flatten();

            let discord_result = match existing_thread {
                Some(ref thread_id) => {
                    // 既存スレッドに追記
                    match discord
                        .post_to_thread(thread_id, &article_result.title, &article_result.content, image_url)
                        .await
                    {
                        Ok(message_id) => Ok((message_id, thread_id.clone())),
                        Err(e) => Err(e),
                    }
                }
                None => {
                    // 新規スレッド作成
                    match discord
                        .post_to_forum(&discord_config, &article_result.title, &article_result.content, image_url)
                        .await
                    {
                        Ok((message_id, thread_id)) => {
                            // スレッドIDをDBに保存
                            self.db.set_topic_thread(topic_name, &thread_id).ok();
                            Ok((message_id, thread_id))
                        }
                        Err(e) => Err(e),
                    }
                }
            };

            match discord_result {
                Ok((message_id, thread_id)) => {
                    // Record in DB
                    match self.db.insert_post(
                        topic_name,
                        &article_result.title,
                        &article_result.content,
                        image_url,
                    ) {
                        Ok(post_id) => {
                            self.db
                                .update_post_discord(post_id, &message_id, Some(&thread_id))
                                .ok();
                            info!(
                                "Posted '{}' to thread {} (post_id: {}, message_id: {})",
                                article_result.title, thread_id, post_id, message_id
                            );
                        }
                        Err(e) => error!("Failed to save post to DB: {}", e),
                    }
                }
                Err(e) => {
                    error!("Failed to post to Discord: {}", e);
                    self.db
                        .insert_post(topic_name, &article_result.title, &article_result.content, image_url)
                        .ok();
                }
            }
        } else {
            // Discord未設定でもDBに記録
            self.db
                .insert_post(
                    &topic.name,
                    &article_result.title,
                    &article_result.content,
                    article_result.image_url.as_deref(),
                )
                .ok();
        }

        info!("Pipeline completed for topic '{}'", topic.name);
        Ok(())
    }
}
