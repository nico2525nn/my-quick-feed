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

pub async fn fetch_feed(url: &str) -> AppResult<Feed> {
    info!("Fetching RSS feed: {}", url);
    let client = reqwest::Client::builder()
        .user_agent("MyQuickFeed/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let response = client.get(url).send().await?;
    let body = response.text().await?;
    parse_feed(&body)
}

pub fn parse_feed(xml: &str) -> AppResult<Feed> {
    let trimmed = xml.trim();
    if trimmed.starts_with("<rss") {
        parse_rss2(xml)
    } else if trimmed.starts_with("<feed") {
        parse_atom(xml)
    } else {
        Err(AppError::RssParse(
            "Unknown feed format: must start with <rss or <feed".into(),
        ))
    }
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
