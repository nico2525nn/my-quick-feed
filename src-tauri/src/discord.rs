use tracing::info;

use crate::config::DiscordConfig;
use crate::errors::{AppError, AppResult};

/// Discord REST API を使用したフォーラム投稿クライアント
/// v1: REST API のみ（Gateway 不使用）
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

    /// フォーラムに新規スレッドを作成し、最初の投稿としてトピック説明文を投稿する
    /// 戻り値: (message_id, thread_id)
    pub async fn create_thread(
        &self,
        config: &DiscordConfig,
        thread_name: &str,
        description: &str,
    ) -> AppResult<(String, String)> {
        let base = "https://discord.com/api/v10";
        let thread_url = format!("{}/channels/{}/threads", base, config.forum_channel_id);

        // Embed枠なし・通常メッセージで説明文を投稿
        let body = serde_json::json!({
            "name": thread_name,
            "message": {
                "content": description,
                "embeds": []
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

        info!("Created thread {} with description", thread_id);
        Ok((message_id, thread_id))
    }

    /// 既存のフォーラムスレッドに記事を通常メッセージ（Markdown、Embed枠なし）として投稿する
    pub async fn post_article(
        &self,
        thread_id: &str,
        title: &str,
        content: &str,
        image_url: Option<&str>,
    ) -> AppResult<String> {
        let base = "https://discord.com/api/v10";
        let msg_url = format!("{}/channels/{}/messages", base, thread_id);

        let mut md = format!("**{}**\n\n{}", title, content.trim());
        if let Some(url) = image_url.filter(|u| !u.is_empty()) {
            md.push_str(&format!("\n\n![image]({})", url));
        }

        let body = serde_json::json!({
            "content": md,
            "embeds": []
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
