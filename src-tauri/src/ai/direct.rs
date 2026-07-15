use std::time::Duration;
use tracing::{info, warn};

use crate::ai::ArticleResult;
use crate::config::TopicConfig;
use crate::errors::{AppError, AppResult};

/// Direct モード: OpenAI互換 Chat Completions API を直接呼び出す
pub async fn call_direct_api(
    api_key: &str,
    model: &str,
    base_url: &str,
    topic: &TopicConfig,
    feed_items: &[crate::fetcher::FeedItem],
) -> AppResult<ArticleResult> {
    let language = topic.language.as_deref().unwrap_or("ja");
    let system_prompt = topic
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);

    let feed_text: String = feed_items
        .iter()
        .map(|item| {
            format!(
                "- {}\n  Link: {}",
                item.title,
                item.link,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": format!("{}\n\n出力は必ずJSON形式で、以下のフィールドを含めてください: title (文字列), content (文字列, 300字程度), image_url (文字列またはnull), sources (文字列の配列)", system_prompt)
            },
            {
                "role": "user",
                "content": format!("以下の情報源からトピック「{}」に関するニュース記事を{}で生成してください。\n\n{}", topic.name, language, feed_text)
            }
        ],
        "temperature": 0.7,
        "max_tokens": 2000
    });

    info!(
        "Calling Direct API for topic '{}' (model: {})",
        topic.name, model
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(AppError::Api(format!(
            "API returned {}: {}",
            status,
            text.chars().take(300).collect::<String>()
        )));
    }

    let data: serde_json::Value = response.json().await?;
    let content = data["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| AppError::Api("No content in API response".into()))?;

    parse_direct_response(content)
}

fn parse_direct_response(content: &str) -> AppResult<ArticleResult> {
    let content = content.trim();

    // Try direct JSON parse
    if let Ok(article) = serde_json::from_str::<ArticleResult>(content) {
        return Ok(article);
    }

    // Try to find JSON block
    let json_start = content.find('{');
    let json_end = content.rfind('}');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        if start < end {
            let json_str = &content[start..=end];
            if let Ok(article) = serde_json::from_str::<ArticleResult>(json_str) {
                return Ok(article);
            }
        }
    }

    // Fallback: treat whole response as article content
    warn!("Could not parse structured JSON from API response, using raw text");
    Ok(ArticleResult {
        title: "Generated Article".to_string(),
        content: content.to_string(),
        image_url: None,
        sources: vec![],
    })
}

const DEFAULT_SYSTEM_PROMPT: &str =
    "与えられた情報源から収集した情報を基に、簡潔なニュース記事を生成してください。\
     出典を明記し、複数のソースを統合する場合はその旨も記載してください。";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_direct_valid_json() {
        let content = r#"{"title":"Test","content":"Body","image_url":null,"sources":["A"]}"#;
        let result = parse_direct_response(content).unwrap();
        assert_eq!(result.title, "Test");
    }

    #[test]
    fn test_parse_direct_json_in_markdown() {
        let content = "```json\n{\"title\":\"Test\",\"content\":\"Body\",\"image_url\":null,\"sources\":[\"A\"]}\n```";
        let result = parse_direct_response(content).unwrap();
        assert_eq!(result.title, "Test");
    }
}
