use tracing::info;

use crate::errors::AppResult;
use crate::fetcher::rss::{fetch_feed, Feed};

/// RSSHUBラッパー — X/Twitter等のRSSHUBエンドポイントからフィードを取得
pub async fn fetch_rsshub(base_url: &str, path: &str) -> AppResult<Feed> {
    let url = format!("{}/{}", base_url.trim_end_matches('/'), path.trim_start_matches('/'));
    info!("Fetching RSSHUB feed: {}", url);
    fetch_feed(&url).await
}

/// X/Twitter ユーザーのタイムラインを取得
pub async fn fetch_twitter_user(base_url: &str, username: &str) -> AppResult<Feed> {
    fetch_rsshub(base_url, &format!("twitter/user/{}", username)).await
}

/// デフォルトRSSHUBインスタンスを使用
pub const DEFAULT_RSSHUB_URL: &str = "https://rsshub.app";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsshub_url_format() {
        let url = format!("{}/{}", DEFAULT_RSSHUB_URL.trim_end_matches('/'), "twitter/user/test".trim_start_matches('/'));
        assert_eq!(url, "https://rsshub.app/twitter/user/test");
    }
}
