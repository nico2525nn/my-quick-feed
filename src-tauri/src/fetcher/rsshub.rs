use tracing::info;

use crate::errors::AppResult;
use crate::fetcher::rss::{fetch_feed, Feed};

/// RSSHUB URL を組み立てる。
/// - path が http(s):// で始まる場合はそのまま使う（フル URL 指定・二重結合防止）
/// - それ以外は base_url と path を結合する
fn build_rsshub_url(base_url: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!("{}/{}", base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }
}

/// RSSHUBラッパー — X/Twitter等のRSSHUBエンドポイントからフィードを取得
/// - base_url + path 形式: fetch_rsshub("https://rsshub.app", "/twitter/user/PlayApex")
/// - url フル指定形式: fetch_rsshub("https://rsshub.app", "https://rsshub.app/twitter/user/PlayApex")
///   → path が http(s):// で始まる場合はそのまま使う（二重結合を防ぐ）
pub async fn fetch_rsshub(base_url: &str, path: &str) -> AppResult<Feed> {
    let url = build_rsshub_url(base_url, path);
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
    fn test_rsshub_url_join() {
        // base_url + path 形式
        assert_eq!(
            build_rsshub_url("https://rsshub.app", "/twitter/user/PlayApex"),
            "https://rsshub.app/twitter/user/PlayApex"
        );
        // 末尾スラッシュ・先頭スラッシュの重複を避ける
        assert_eq!(
            build_rsshub_url("https://rsshub.app/", "twitter/user/PlayApex"),
            "https://rsshub.app/twitter/user/PlayApex"
        );
        // フル URL 指定はそのまま（二重結合を防ぐ）
        assert_eq!(
            build_rsshub_url("https://rsshub.app", "https://rsshub.app/twitter/user/PlayApex"),
            "https://rsshub.app/twitter/user/PlayApex"
        );
    }
}
