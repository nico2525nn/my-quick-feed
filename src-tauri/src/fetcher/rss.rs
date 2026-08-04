use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;
use tracing::{info, warn};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct FeedItem {
    pub title: String,
    pub link: String,
    pub description: Option<String>,
    pub pub_date: Option<DateTime<Utc>>,
    pub guid: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Feed {
    pub title: Option<String>,
    pub items: Vec<FeedItem>,
}

/// レート制限対策のフィードキャッシュ（メモリ内）。
/// Reddit は 1 分あたり 1 リクエスト程度の厳しい制限があり、429 で全リトライが
/// 失敗しても、前回取得できていたフィードがあればそれを使ってパイプラインを継続する。
/// キャッシュは正常取得のたびに更新される（古いフィードでも記事生成には十分使える）。
static FEED_CACHE: LazyLock<Arc<Mutex<HashMap<String, (Feed, SystemTime)>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// 429 応答から待機時間（秒）を決定する。
/// Retry-After ヘッダー → x-ratelimit-reset ヘッダー（Reddit が使う）→ バックオフ。
fn rate_limit_wait_secs(response: &reqwest::Response, attempt: u64) -> u64 {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    };
    header("retry-after")
        .or_else(|| header("x-ratelimit-reset"))
        .unwrap_or(5 * (attempt + 1))
}

pub async fn fetch_feed(url: &str) -> AppResult<Feed> {
    // Reddit は http だと 429 になるため https に置換
    let url = if url.starts_with("http://www.reddit.com/") || url.starts_with("http://reddit.com/") {
        let replaced = url.replacen("http://", "https://", 1);
        info!("Reddit URL: http→https に置換: {}", replaced);
        replaced
    } else {
        url.to_string()
    };
    info!("Fetching RSS feed: {}", url);
    let client = reqwest::Client::builder()
        .user_agent("MyQuickFeed/0.1 (RSS aggregator)")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // 429（レート制限）対策: 最大5回リトライ
    // （Retry-After / x-ratelimit-reset 尊重、無ければ 5s/10s/15s/20s/25s バックオフ）
    let mut last_err: Option<AppError> = None;
    for attempt in 0..5 {
        let response = client.get(url.as_str()).send().await?;
        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let wait = rate_limit_wait_secs(&response, attempt);
            warn!(
                "Rate limited (429) for {}, retry in {}s (attempt {}/5)",
                url, wait, attempt + 1
            );
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            last_err = Some(AppError::RssParse(format!(
                "HTTP 429 Too Many Requests for {}",
                url
            )));
            continue;
        }

        if !status.is_success() {
            return Err(AppError::RssParse(format!(
                "HTTP {} for {}",
                status, url
            )));
        }
        let body = response.text().await?;
        info!(
            "Fetched {} ({} bytes, status {})",
            url,
            body.len(),
            status
        );
        let feed = parse_feed(&body)?;
        // 正常取得できたフィードをキャッシュ（次回 429 時のフォールバック用）
        FEED_CACHE
            .lock()
            .unwrap()
            .insert(url.clone(), (feed.clone(), SystemTime::now()));
        return Ok(feed);
    }

    // 全リトライ失敗時: 以前に取得できたキャッシュがあればそれを使う
    // （古いフィードでも、直近タイトルとの重複照合で記事生成には十分使える）
    if let Some((feed, fetched_at)) = FEED_CACHE.lock().unwrap().get(url.as_str()) {
        let age_mins = SystemTime::now()
            .duration_since(*fetched_at)
            .map(|d| d.as_secs() / 60)
            .unwrap_or(0);
        warn!(
            "Rate limited and retries exhausted for {}, using cached feed ({} min old, {} items)",
            url,
            age_mins,
            feed.items.len()
        );
        return Ok(feed.clone());
    }

    Err(last_err.unwrap_or_else(|| {
        AppError::RssParse(format!("Failed to fetch after retries: {}", url))
    }))
}

pub fn parse_feed(xml: &str) -> AppResult<Feed> {
    // BOM除去
    let cleaned = xml.trim_start_matches('\u{feff}').trim_start_matches('\u{fffe}');
    // XML宣言とDOCTYPEを除去
    let cleaned = strip_xml_header(cleaned);
    let trimmed = cleaned.trim();

    if trimmed.starts_with("<rss") {
        parse_rss2(cleaned)
    } else if trimmed.starts_with("<feed") {
        parse_atom(cleaned)
    } else {
        let preview: String = trimmed.chars().take(200).collect();
        Err(AppError::RssParse(format!(
            "Unknown feed format (preview): {}",
            preview
        )))
    }
}

/// XML宣言 <?xml ... ?> と DOCTYPE を削除
fn strip_xml_header(s: &str) -> &str {
    let s = s.trim_start();
    if s.starts_with("<?xml") {
        if let Some(end) = s.find("?>") {
            let after = s[end + 2..].trim_start();
            if after.starts_with("<!DOCTYPE") {
                if let Some(doc_end) = after.find(">") {
                    return after[doc_end + 1..].trim_start();
                }
            }
            return after;
        }
    }
    s
}

fn parse_rss2(xml: &str) -> AppResult<Feed> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let feed_title: Option<String> = None;
    let mut items = Vec::new();

    let mut in_item = false;
    let mut in_channel = false;
    let mut current_title = String::new();
    let mut current_link = String::new();
    let mut current_desc = String::new();
    let mut current_pub_date = String::new();
    let mut current_guid = String::new();
    let mut in_title = false;
    let mut in_link = false;
    let mut in_desc = false;
    let mut in_pub_date = false;
    let mut in_guid = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                match name.as_str() {
                    "channel" => in_channel = true,
                    "item" => in_item = true,
                    "title" if in_item || in_channel => in_title = true,
                    "link" if in_item || in_channel => in_link = true,
                    "description" if in_item => in_desc = true,
                    "pubdate" if in_item => in_pub_date = true,
                    "guid" if in_item => in_guid = true,
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_title {
                    current_title.push_str(&text);
                } else if in_link {
                    current_link.push_str(&text);
                } else if in_desc {
                    current_desc.push_str(&text);
                } else if in_pub_date {
                    current_pub_date.push_str(&text);
                } else if in_guid {
                    current_guid.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                match name.as_str() {
                    "channel" => in_channel = false,
                    "item" => {
                        if !current_title.is_empty() && !current_link.is_empty() {
                            let pub_date = parse_date(&current_pub_date);
                            items.push(FeedItem {
                                title: current_title.clone(),
                                link: current_link.clone(),
                                description: if current_desc.is_empty() {
                                    None
                                } else {
                                    Some(current_desc.clone())
                                },
                                pub_date,
                                guid: if current_guid.is_empty() {
                                    None
                                } else {
                                    Some(current_guid.clone())
                                },
                            });
                        }
                        in_item = false;
                        current_title.clear();
                        current_link.clear();
                        current_desc.clear();
                        current_pub_date.clear();
                        current_guid.clear();
                    }
                    "title" => in_title = false,
                    "link" => in_link = false,
                    "description" => in_desc = false,
                    "pubdate" => in_pub_date = false,
                    "guid" => in_guid = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                warn!("XML parse warning: {:?}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    info!("Parsed RSS feed: {} items", items.len());
    Ok(Feed {
        title: feed_title,
        items,
    })
}

fn parse_atom(xml: &str) -> AppResult<Feed> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let feed_title: Option<String> = None;
    let mut items = Vec::new();

    let mut in_entry = false;
    let mut current_title = String::new();
    let mut current_link = String::new();
    let mut current_desc = String::new();
    let mut current_pub_date = String::new();
    let mut current_id = String::new();
    let mut in_title = false;
    let mut _in_link_href = false;
    let mut in_desc = false;
    let mut in_pub_date = false;
    let mut in_id = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                match name.as_str() {
                    "entry" => in_entry = true,
                    "title" if in_entry || feed_title.is_none() => in_title = true,
                    "link" if in_entry => {
                        if let Ok(attr) = e.try_get_attribute("href") {
                            if let Some(val) = attr {
                                current_link = String::from_utf8_lossy(&val.value).to_string();
                            }
                        }
                        _in_link_href = true;
                    }
                    "summary" | "content" if in_entry => in_desc = true,
                    "published" | "updated" if in_entry => {
                        if current_pub_date.is_empty() {
                            in_pub_date = true
                        }
                    }
                    "id" if in_entry => in_id = true,
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_title {
                    current_title.push_str(&text);
                } else if in_desc {
                    current_desc.push_str(&text);
                } else if in_pub_date {
                    current_pub_date.push_str(&text);
                } else if in_id {
                    current_id.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                match name.as_str() {
                    "entry" => {
                        if !current_title.is_empty() {
                            let pub_date = parse_date(&current_pub_date);
                            items.push(FeedItem {
                                title: current_title.clone(),
                                link: current_link.clone(),
                                description: if current_desc.is_empty() {
                                    None
                                } else {
                                    Some(current_desc.clone())
                                },
                                pub_date,
                                guid: if current_id.is_empty() {
                                    None
                                } else {
                                    Some(current_id.clone())
                                },
                            });
                        }
                        in_entry = false;
                        current_title.clear();
                        current_link.clear();
                        current_desc.clear();
                        current_pub_date.clear();
                        current_id.clear();
                    }
                    "title" => in_title = false,
                    "link" => _in_link_href = false,
                    "summary" | "content" => in_desc = false,
                    "published" | "updated" => in_pub_date = false,
                    "id" => in_id = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                warn!("XML parse warning: {:?}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    info!("Parsed Atom feed: {} items", items.len());
    Ok(Feed {
        title: feed_title,
        items,
    })
}

fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // RFC 2822 (common in RSS)
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // RFC 3339 / ISO 8601 (common in Atom)
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Try common RSS formats
    for fmt in &[
        "%a, %d %b %Y %H:%M:%S %z",
        "%a, %d %b %Y %H:%M:%S %Z",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%SZ",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    warn!("Could not parse date: {}", s);
    None
}
