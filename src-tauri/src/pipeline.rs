use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
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
    /// false なら Discord 投稿・DB 保存をしない（--no-post ドライラン）
    post_enabled: bool,
}

impl Pipeline {
    pub fn new(
        config_manager: Arc<ConfigManager>,
        db: Arc<Database>,
        discord: Option<Arc<DiscordClient>>,
        post_enabled: bool,
    ) -> Self {
        Self { config_manager, db, discord, post_enabled }
    }

    pub async fn run(&self, topic: &TopicConfig) -> AppResult<()> {
        let tn = &topic.name;
        let started = Instant::now();
        info!(topic = %tn, "=== Pipeline started ===");
        info!(topic = %tn, "Sources: {} 個", topic.sources.len());

        // Step 1: Fetch（同一実行内の重複はメモリ上 HashSet で管理）
        let mut seen_urls: HashSet<String> = HashSet::new();
        let mut all_items: Vec<FeedItem> = Vec::new();
        for (si, source) in topic.sources.iter().enumerate() {
            // Reddit等のレート制限回避: ソース間に2秒ディレイ
            if si > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            let fetch_started = Instant::now();
            let items = match source.source_type.as_str() {
                "rss" => match fetch_feed(&source.url).await {
                    Ok(feed) => {
                        info!(
                            topic = %tn,
                            "  [{}] RSS取得OK: {} ({} items, {:.1}s)",
                            si + 1, source.url, feed.items.len(),
                            fetch_started.elapsed().as_secs_f64()
                        );
                        feed.items
                    }
                    Err(e) => {
                        error!(topic = %tn, "  [{}] RSS取得失敗: {} — {}", si + 1, source.url, e);
                        continue;
                    }
                },
                "rsshub" => {
                    let base = source.base_url.as_deref().unwrap_or(DEFAULT_RSSHUB_URL);
                    let path = source.path.as_deref().unwrap_or(&source.url);
                    match fetch_rsshub(base, path).await {
                        Ok(feed) => {
                            info!(
                                topic = %tn,
                                "  [{}] RSSHUB取得OK: {}/{} ({} items, {:.1}s)",
                                si + 1, base, path, feed.items.len(),
                                fetch_started.elapsed().as_secs_f64()
                            );
                            feed.items
                        }
                        Err(e) => {
                            error!(topic = %tn, "  [{}] RSSHUB取得失敗: {} — {}", si + 1, source.url, e);
                            continue;
                        }
                    }
                }
                other => {
                    warn!(topic = %tn, "  [{}] 不明なソース種別: {}", si + 1, other);
                    continue;
                }
            };
            for item in items {
                if seen_urls.insert(item.link.clone()) {
                    all_items.push(item);
                }
            }
        }

        if all_items.is_empty() {
            info!(topic = %tn, "=== 取得アイテムなし: 終了 ({:.1}s) ===", started.elapsed().as_secs_f64());
            return Ok(());
        }
        // OMP のコンテキスト制限対策: 最大40件まで（多すぎると処理が重い/タイムアウトする）
        if all_items.len() > 40 {
            info!(topic = %tn, "アイテム数制限: {} → 40件", all_items.len());
            all_items.truncate(40);
        }
        info!(
            topic = %tn,
            "取得完了: {} items（同一実行内重複除外後、{:.1}s）",
            all_items.len(),
            started.elapsed().as_secs_f64()
        );

        // Step 2: 直近2日分の投稿タイトルを取得（重複防止用）
        let recent_titles = match self.db.get_recent_titles(tn, RECENT_TITLE_DAYS) {
            Ok(t) => t,
            Err(e) => {
                // エラー時は重複防止が効かない（再投稿の可能性）ためログで明示する
                error!(topic = %tn, "直近タイトル取得失敗（重複防止が効きません）: {}", e);
                Vec::new()
            }
        };
        if !recent_titles.is_empty() {
            info!(topic = %tn, "直近{}日分の投稿タイトル: {} 件", RECENT_TITLE_DAYS, recent_titles.len());
            for (i, t) in recent_titles.iter().take(5).enumerate() {
                info!(topic = %tn, "  [{}] 既投稿: {}", i + 1, t);
            }
            if recent_titles.len() > 5 {
                info!(topic = %tn, "  ... 他 {} 件", recent_titles.len() - 5);
            }
        } else {
            info!(topic = %tn, "直近{}日分の投稿なし（初回実行）", RECENT_TITLE_DAYS);
        }

        // Step 3: AI
        let config = self.config_manager.get();
        let ai_started = Instant::now();
        let articles: Vec<ArticleResult> = match config.ai.mode.as_str() {
            "direct" => {
                let api_key = config.ai.api_key.as_deref()
                    .ok_or_else(|| AppError::Config("API key not configured".into()))?;
                let model = config.ai.model.as_deref().unwrap_or("mimo-v2.5");
                let base_url = config.ai.base_url.as_deref().unwrap_or("https://openrouter.ai/api/v1");
                info!(topic = %tn, "Direct API呼び出し [model={}]", model);
                call_direct_api(api_key, model, base_url, topic, &all_items, &recent_titles)
                    .await.map(|a| vec![a])?
            }
            _ => {
                let cmd = config.ai.agent_command.as_deref().unwrap_or("omp");
                let model = config.ai.model.as_deref().unwrap_or("mimo-v2.5");
                let timeout = config.ai.agent_timeout_sec.unwrap_or(300).max(30);
                // 設定に無い場合は high（mimo-v2.5 は low/medium/high のみ対応・深く考えさせる）
                let thinking = config.ai.thinking_level.as_deref().unwrap_or("high");
                info!(topic = %tn, "Agent呼び出し [cmd={}, model={}, timeout={}s, thinking={}, session_reuse={}]", cmd, model, timeout, thinking, config.ai.session_reuse);
                run_agent(cmd, model, timeout, Some(thinking), config.ai.session_reuse, topic, &all_items, &recent_titles).await?
            }
        };
        info!(
            topic = %tn,
            "AI生成完了: {} 記事 ({:.1}s)",
            articles.len(),
            ai_started.elapsed().as_secs_f64()
        );

        if articles.is_empty() {
            info!(topic = %tn, "=== 生成記事0件: 終了 ({:.1}s) ===", started.elapsed().as_secs_f64());
            return Ok(());
        }

        for (i, a) in articles.iter().enumerate() {
            info!(
                topic = %tn,
                "  記事[{}]: \"{}\" (tags={:?}, sources={})",
                i + 1, a.title, a.tags, a.sources.len()
            );
        }

        // Step 4: Post each article（通常メッセージ・Markdown、Embed枠なし）
        if !self.post_enabled {
            // ドライラン（--no-post）: 投稿も DB 保存もしない。
            // DB に保存すると直近タイトルの重複防止リストに入り、後で実際に投稿できなくなるため。
            info!(topic = %tn, "投稿無効（--no-post）: 生成記事をログに出力のみ");
            for (i, a) in articles.iter().enumerate() {
                let chars = a.content.chars().count();
                let preview: String = a.content.chars().take(150).collect();
                let suffix = if chars > 150 { "…" } else { "" };
                info!(
                    topic = %tn,
                    "  記事[{}]: \"{}\" — {}{}",
                    i + 1, a.title, preview, suffix
                );
            }
            return Ok(());
        }

        if let Some(discord) = &self.discord {
            // トピック専用のフォーラムチャンネルがあればそれを使う（無ければグローバル設定）
            let mut dc = config.discord.clone();
            if let Some(ch) = topic.forum_channel_id.as_deref().filter(|c| !c.is_empty()) {
                dc.forum_channel_id = ch.to_string();
                info!(topic = %tn, "トピック専用チャンネル使用: {}", ch);
            }
            let thread_id = match self.db.get_topic_thread(tn) {
                Ok(opt) => opt,
                Err(e) => {
                    // DB エラーで「スレッドなし」誤判定すると重複スレッドができるため中断する
                    error!(topic = %tn, "スレッドID取得失敗: {}", e);
                    return Err(e);
                }
            };

            // スレッドが無ければ作成し、トピック説明文を最初の投稿に
            let thread_id = match thread_id {
                Some(tid) => {
                    info!(topic = %tn, "既存スレッド使用: {}", tid);
                    tid
                }
                None => {
                    let desc = format!(
                        "🖊️ 【{}】{} に関するニュースを自動収集・まとめます",
                        topic.name, topic.name
                    );
                    info!(topic = %tn, "新規スレッド作成: {}", topic.name);
                    match discord.create_thread(&dc, &topic.name, &desc).await {
                        Ok((_, tid)) => {
                            if let Err(e) = self.db.set_topic_thread(tn, &tid) {
                                warn!(topic = %tn, "スレッドID保存失敗（次回実行で再作成を試みます）: {}", e);
                            }
                            info!(topic = %tn, "スレッド作成OK: {}", tid);
                            tid
                        }
                        Err(e) => {
                            // 失敗を呼び出し元に通知して再試行可能にする（成功扱いにしない）
                            error!(topic = %tn, "スレッド作成失敗: {}", e);
                            return Err(AppError::Discord(format!("スレッド作成失敗: {}", e)));
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
                let post_started = Instant::now();
                match discord.post_article(&thread_id, &article.title, &article.content, img).await {
                    Ok(msg_id) => {
                        info!(
                            topic = %tn,
                            "投稿OK [{}]: \"{}\" (msg={}, {:.1}s)",
                            i + 1, article.title, msg_id,
                            post_started.elapsed().as_secs_f64()
                        );
                        // 投稿成功時のみ DB に記録（失敗分は次回実行で再試行する）
                        if let Ok(post_id) = self.db.insert_post(
                            tn, &article.title, &article.content, img, tags_json.as_deref(),
                        ) {
                            let _ = self.db.update_post_discord(post_id, &msg_id, Some(&thread_id));
                        }
                    }
                    Err(e) => {
                        // DB には記録しない（直近タイトルの重複防止リストに入ると再試行されなくなる）
                        error!(topic = %tn, "投稿失敗 [{}]: \"{}\" — {}", i + 1, article.title, e);
                    }
                }
            }
        } else {
            info!(topic = %tn, "Discord未設定: DBのみに保存");
            for article in &articles {
                let tags_json = if article.tags.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&article.tags).unwrap_or_default())
                };
                self.db.insert_post(tn, &article.title, &article.content, article.image_url.as_deref(), tags_json.as_deref()).ok();
            }
        }

        info!(topic = %tn, "=== Pipeline 完了 ({:.1}s) ===", started.elapsed().as_secs_f64());
        Ok(())
    }
}
