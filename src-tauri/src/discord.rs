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

    /// トークンの検証
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

    /// フォーラムに新規スレッドを作成し、最初の記事を投稿する
    /// 戻り値: (message_id, thread_id)
    pub async fn post_to_forum(
        &self,
        config: &DiscordConfig,
        title: &str,
        content: &str,
        image_url: Option<&str>,
    ) -> AppResult<(String, String)> {
        let base = "https://discord.com/api/v10";
        let thread_url = format!("{}/channels/{}/threads", base, config.forum_channel_id);

        let body = serde_json::json!({
            "name": title,
            "message": {
                "content": "",
                "embeds": [make_embed(title, content, image_url)]
            }
        });

        let resp = self
            .http_client
            .post(&thread_url)
            .header("Authorization", format!("Bot {}", self.token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "MyQuickFeed/0.1")
            .json(&body)
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
            .ok_or_else(|| AppError::Discord("No thread id in response".into()))?
            .to_string();
        let message_id = data["message"]["id"]
            .as_str()
            .ok_or_else(|| AppError::Discord("No message id in response".into()))?
            .to_string();

        info!("Created thread {} with message {}", thread_id, message_id);
        Ok((message_id, thread_id))
    }

    /// 既存のフォーラムスレッドにメッセージとして追記する
    pub async fn post_to_thread(
        &self,
        thread_id: &str,
        title: &str,
        content: &str,
        image_url: Option<&str>,
    ) -> AppResult<String> {
        let base = "https://discord.com/api/v10";
        let msg_url = format!("{}/channels/{}/messages", base, thread_id);

        let body = serde_json::json!({
            "embeds": [make_embed(title, content, image_url)]
        });

        let resp = self
            .http_client
            .post(&msg_url)
            .header("Authorization", format!("Bot {}", self.token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "MyQuickFeed/0.1")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Discord(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Discord(format!(
                "Failed to post to thread ({}): {}",
                status, text
            )));
        }

        let data: serde_json::Value = resp.json().await?;
        let message_id = data["id"]
            .as_str()
            .ok_or_else(|| AppError::Discord("No message id in response".into()))?
            .to_string();

        info!("Posted to thread {}: message_id={}", thread_id, message_id);
        Ok(message_id)
    }
}

/// Embed 構造体を生成（共通）
fn make_embed(title: &str, content: &str, image_url: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "description": content,
        "color": 0x58a6ff,
        "image": image_url
            .filter(|u| !u.is_empty())
            .map(|u| serde_json::json!({"url": u})),
    })
}
