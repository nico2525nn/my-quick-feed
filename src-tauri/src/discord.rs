use tracing::info;

use crate::config::DiscordConfig;
use crate::errors::{AppError, AppResult};

/// Discord REST API を使用したフォーラム投稿クライアント
pub struct DiscordClient {
    token: String,
    http_client: reqwest::Client,
}

impl DiscordClient {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_string(),
            http_client: reqwest::Client::new(),
        }
    }

    /// トークンの検証（起動時チェック用）
    pub async fn verify_token(&self) -> AppResult<()> {
        let resp = self
            .http_client
            .get("https://discord.com/api/v10/users/@me")
            .header("Authorization", format!("Bot {}", self.token))
            .send()
            .await
            .map_err(|e| AppError::Discord(format!("HTTP error: {}", e)))?;

        if resp.status().is_success() {
            let data: serde_json::Value = resp.json().await?;
            let username = data["username"].as_str().unwrap_or("unknown");
            info!("Discord authenticated as: {}", username);
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(AppError::Discord(format!(
                "Authentication failed ({}): {}",
                status, text
            )))
        }
    }

    /// フォーラムチャンネルにスレッドを作成し、記事を投稿する
    /// 戻り値: (message_id, option(thread_id))
    pub async fn post_to_forum(
        &self,
        config: &DiscordConfig,
        title: &str,
        content: &str,
        image_url: Option<&str>,
    ) -> AppResult<(String, Option<String>)> {
        // API v10 ベースURL
        let base = "https://discord.com/api/v10";
        let headers = vec![
            ("Authorization", format!("Bot {}", self.token)),
            ("Content-Type", "application/json".to_string()),
            ("User-Agent", "MyQuickFeed/0.1".to_string()),
        ];

        let apply_headers = |req: reqwest::RequestBuilder| -> reqwest::RequestBuilder {
            let mut r = req;
            for (k, v) in &headers {
                r = r.header(*k, &v[..]);
            }
            r
        };

        // Step 1: フォーラムスレッドを作成
        let create_thread_body = serde_json::json!({
            "name": title,
            "message": {
                "content": "",
                "embeds": [{
                    "title": title,
                    "description": content,
                    "color": 0x58a6ff,
                    "image": image_url
                        .filter(|u| !u.is_empty())
                        .map(|u| serde_json::json!({"url": u})),
                }]
            }
        });

        let thread_url = format!("{}/channels/{}/threads", base, config.forum_channel_id);
        let resp = apply_headers(self.http_client.post(&thread_url))
            .json(&create_thread_body)
            .send()
            .await
            .map_err(|e| AppError::Discord(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Discord(format!(
                "Failed to create forum thread ({}): {}",
                status, text
            )));
        }

        let data: serde_json::Value = resp.json().await?;
        let thread_id = data["id"]
            .as_str()
            .ok_or_else(|| AppError::Discord("No thread id in response".into()))?;
        let message_id = data["message"]["id"]
            .as_str()
            .ok_or_else(|| AppError::Discord("No message id in response".into()))?;

        info!(
            "Created forum thread: {} (thread: {}, message: {})",
            title, thread_id, message_id
        );

        Ok((message_id.to_string(), Some(thread_id.to_string())))
    }
}
