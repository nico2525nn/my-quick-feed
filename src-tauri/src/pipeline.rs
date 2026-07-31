use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, info, warn};
use crate::ai::{call_direct_api, run_agent, ArticleResult};
use crate::config::{ConfigManager, TopicConfig};
use crate::db::Database;
use crate::discord::DiscordClient;
use crate::errors::{AppError, AppResult};
use crate::fetcher::{fetch_feed, fetch_rsshub, FeedItem, DEFAULT_RSSHUB_URL};

/// 直近タイトル取得対象日数（重複防止用）
const RECENT_TITLE_DAYS: i64 = 2;

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

        // Step 1: Fetch（同一実行内の重複はメモリ上 HashSet で管理）
        let mut seen_urls: HashSet<String> = HashSet::new();
        let mut all_items: Vec<FeedItem> = Vec::new();
        for source in &topic.sources {
            let items = match source.source_type.as_str() {
                "rss" => match fetch_feed(&source.url).await {
                    Ok(feed) => feed.items,
                    Err(e) => { error!(topic = %tn, "RSS failed: {} — {}", source.url, e); continue; }
                },
                "rsshub" => {
                    // base_url + path 形式（url 優先、無ければ base_url+path）
                    let base = source.base_url.as_deref().unwrap_or(DEFAULT_RSSHUB_URL);
                    let path = source.path.as_deref().unwrap_or(&source.url);
                    match fetch_rsshub(base, path).await {
                        Ok(feed) => feed.items,
                        Err(e) => { error!(topic = %tn, "RSSHUB failed: {} — {}", source.url, e); continue; }
                    }
                }
                other => { warn!(topic = %tn, "Unknown source type: {}", other); continue; }
            };
            for item in items {
                if seen_urls.insert(item.link.clone()) {
                    all_items.push(item);
                }
            }
        }
        if all_items.is_empty() { info!(topic = %tn, "No items"); return Ok(()); }
        info!(topic = %tn, "Fetched {} items (deduped)", all_items.len());

        // Step 2: 直近2日分の投稿タイトルを取得（重複防止用）
        let recent_titles = self.db.get_recent_titles(tn, RECENT_TITLE_DAYS).unwrap_or_default();
        if !recent_titles.is_empty() {
            info!(topic = %tn, "{} recent titles loaded for dedup", recent_titles.len());
        }

        // Step 3: AI
        let config = self.config_manager.get();
        let articles: Vec<ArticleResult> = match config.ai.mode.as_str() {
            "direct" => {
                let api_key = config.ai.api_key.as_deref()
                    .ok_or_else(|| AppError::Config("API key not configured".into()))?;
                let model = config.ai.model.as_deref().unwrap_or("mimo-v2.5");
                let base_url = config.ai.base_url.as_deref().unwrap_or("https://openrouter.ai/api/v1");
                info!(topic = %tn, "Direct API [model={}]", model);
                call_direct_api(api_key, model, base_url, topic, &all_items, &recent_titles)
                    .await.map(|a| vec![a])?
            }
            _ => {
                let cmd = config.ai.agent_command.as_deref().unwrap_or("omp");
                let model = config.ai.model.as_deref().unwrap_or("mimo-v2.5");
                let timeout = config.ai.agent_timeout_sec.unwrap_or(180);
                info!(topic = %tn, "Agent [cmd={}, model={}, timeout={}s]", cmd, model, timeout);
                run_agent(cmd, model, timeout, topic, &all_items, &recent_titles).await?
            }
        };

        if articles.is_empty() { info!(topic = %tn, "No articles from agent"); return Ok(()); }
        info!(topic = %tn, "{} article(s) generated", articles.len());

        // Step 4: Post each article（通常メッセージ・Markdown、Embed枠なし）
        if let Some(discord) = &self.discord {
            let dc = config.discord.clone();
            let thread_id = self.db.get_topic_thread(tn).ok().flatten();

            // スレッドが無ければ作成し、トピック説明文を最初の投稿に
            let thread_id = match thread_id {
                Some(tid) => tid,
                None => {
                    let desc = format!(
                        "🖊️ 【{}】{} に関するニュースを自動収集・まとめます",
                        topic.name, topic.name
                    );
                    info!(topic = %tn, "Creating new forum thread");
                    match discord.create_thread(&dc, &topic.name, &desc).await {
                        Ok((_, tid)) => {
                            self.db.set_topic_thread(tn, &tid).ok();
                            tid
                        }
                        Err(e) => {
                            error!(topic = %tn, "Thread creation failed: {}", e);
                            return Ok(());
                        }
                    }
                }
            };

            for (i, article) in articles.iter().enumerate() {
                let img = article.image_url.as_deref();
                let tags_json = if article.tags.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&article.tags).unwrap_or_default())
                };

                match discord.post_article(&thread_id, &article.title, &article.content, img).await {
                    Ok(msg_id) => {
                        self.db.insert_post(tn, &article.title, &article.content, img, tags_json.as_deref()).ok();
                        info!(topic = %tn, "Posted #{}: \"{}\" (msg={})", i + 1, article.title, msg_id);
                    }
                    Err(e) => {
                        error!(topic = %tn, "Post #{} failed [{}]: {}", i + 1, article.title, e);
                        self.db.insert_post(tn, &article.title, &article.content, img, tags_json.as_deref()).ok();
                    }
                }
            }
        } else {
            for article in &articles {
                let tags_json = if article.tags.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&article.tags).unwrap_or_default())
                };
                self.db.insert_post(tn, &article.title, &article.content, article.image_url.as_deref(), tags_json.as_deref()).ok();
            }
            info!(topic = %tn, "Saved {} articles to DB", articles.len());
        }

        info!(topic = %tn, "Pipeline done");
        Ok(())
    }
}
