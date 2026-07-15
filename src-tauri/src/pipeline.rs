use std::sync::Arc;
use tracing::{error, info, warn};
use crate::ai::{call_direct_api, run_agent, ArticleResult};
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
        Self { config_manager, db, discord }
    }

    pub async fn run(&self, topic: &TopicConfig) -> AppResult<()> {
        let tn = &topic.name;
        info!(topic = %tn, "Pipeline started");

        // Step 1: Fetch
        let mut all_items: Vec<FeedItem> = Vec::new();
        for source in &topic.sources {
            let items = match source.source_type.as_str() {
                "rss" => match fetch_feed(&source.url).await {
                    Ok(feed) => feed.items,
                    Err(e) => { error!(topic = %tn, "RSS failed: {} — {}", source.url, e); continue; }
                },
                "rsshub" => match fetch_rsshub(DEFAULT_RSSHUB_URL, &source.url).await {
                    Ok(feed) => feed.items,
                    Err(e) => { error!(topic = %tn, "RSSHUB failed: {} — {}", source.url, e); continue; }
                },
                other => { warn!(topic = %tn, "Unknown source type: {}", other); continue; }
            };
            all_items.extend(items);
        }
        if all_items.is_empty() { info!(topic = %tn, "No items"); return Ok(()); }
        info!(topic = %tn, "Fetched {} items", all_items.len());

        // Step 2: Dedup
        let new_items: Vec<FeedItem> = all_items.into_iter()
            .filter(|item| match self.db.is_seen(tn, &item.link) {
                Ok(false) => true,
                Ok(true) => false,
                Err(e) => { warn!(topic = %tn, "DB dedup error: {}", e); true }
            })
            .collect();
        if new_items.is_empty() { info!(topic = %tn, "All seen"); return Ok(()); }
        info!(topic = %tn, "{} new items", new_items.len());

        // Step 3: AI
        let config = self.config_manager.get();
        let articles: Vec<ArticleResult> = match config.ai.mode.as_str() {
            "direct" => {
                let api_key = config.ai.api_key.as_deref()
                    .ok_or_else(|| AppError::Config("API key not configured".into()))?;
                let model = config.ai.model.as_deref().unwrap_or("gpt-4o-mini");
                let base_url = config.ai.base_url.as_deref().unwrap_or("https://openrouter.ai/api/v1");
                info!(topic = %tn, "Direct API [model={}]", model);
                call_direct_api(api_key, model, base_url, topic, &new_items).await.map(|a| vec![a])?
            }
            _ => {
                let cmd = config.ai.agent_command.as_deref().unwrap_or("omp");
                let model = config.ai.model.as_deref().unwrap_or("default");
                let timeout = config.ai.agent_timeout_sec.unwrap_or(120);
                info!(topic = %tn, "Agent [cmd={}, model={}, timeout={}s]", cmd, model, timeout);
                run_agent(cmd, model, timeout, topic, &new_items).await?
            }
        };

        // Mark all as seen
        for item in &new_items { self.db.mark_seen(tn, &item.link, Some(&item.title)).ok(); }

        if articles.is_empty() { info!(topic = %tn, "No articles from agent"); return Ok(()); }
        info!(topic = %tn, "{} article(s) generated", articles.len());

        // Step 4: Post each article
        if let Some(discord) = &self.discord {
            let dc = config.discord.clone();
            let thread_id = self.db.get_topic_thread(tn).ok().flatten();

            for (i, article) in articles.iter().enumerate() {
                let img = article.image_url.as_deref();

                let result = if i == 0 {
                    match &thread_id {
                        Some(tid) => discord.post_to_thread(tid, &article.title, &article.content, img).await
                            .map(|mid| (mid, tid.clone())),
                        None => discord.post_to_forum(&dc, &article.title, &article.content, img).await
                            .map(|(mid, tid)| { self.db.set_topic_thread(tn, &tid).ok(); (mid, tid) }),
                    }
                } else {
                    // 2件目以降は既存スレッドを参照
                    match &thread_id {
                        Some(tid) => discord.post_to_thread(tid, &article.title, &article.content, img).await
                            .map(|mid| (mid, tid.clone())),
                        None => {
                            // 初回作成に失敗していた場合 → 新規作成
                            discord.post_to_forum(&dc, &article.title, &article.content, img).await
                                .map(|(mid, tid)| { self.db.set_topic_thread(tn, &tid).ok(); (mid, tid) })
                        }
                    }
                };

                match result {
                    Ok((msg_id, tid)) => {
                        self.db.insert_post(tn, &article.title, &article.content, img).ok();
                        info!(topic = %tn, "Posted #{}: \"{}\" (thread={})", i + 1, article.title, tid);
                    }
                    Err(e) => {
                        error!(topic = %tn, "Post #{} failed [{}]: {}", i + 1, article.title, e);
                        self.db.insert_post(tn, &article.title, &article.content, img).ok();
                    }
                }
            }
        } else {
            for article in &articles {
                self.db.insert_post(tn, &article.title, &article.content, article.image_url.as_deref()).ok();
            }
            info!(topic = %tn, "Saved {} articles to DB", articles.len());
        }

        info!(topic = %tn, "Pipeline done");
        Ok(())
    }
}
