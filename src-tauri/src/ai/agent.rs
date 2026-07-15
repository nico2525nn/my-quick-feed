use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};
use crate::ai::{ArticleResult, resolve_system_prompt};
use crate::config::TopicConfig;
use crate::errors::{AppError, AppResult};

/// OMP / OpenCode CLI を子プロセスとして呼び出し、エージェントに記事生成を委託する
pub async fn run_agent(
    command: &str,
    timeout_sec: u64,
    topic: &TopicConfig,
    feed_items: &[crate::fetcher::FeedItem],
) -> AppResult<ArticleResult> {
    let feed_summary = format_feed_summary(feed_items);
    let language = topic.language.as_deref().unwrap_or("ja");
    let system_prompt = resolve_system_prompt(&topic.name, language, topic.system_prompt.as_deref());

    let prompt = format!(
        r#"あなたはニュース記事を生成するアシスタントです。

## トピック
{topic_name}

## 言語
{language}

## システム指示
{system_prompt}

## 元記事
{feed_summary}

## 出力形式
以下のJSON形式で出力してください。JSON以外の出力は絶対に含めないでください。
{{
  "title": "記事タイトル（{language}で）",
  "content": "記事本文（300字程度）",
  "image_url": "関連画像URL（あれば）",
  "sources": ["出典1", "出典2"]
}}
"#,
        topic_name = topic.name,
        language = language,
        system_prompt = system_prompt,
        feed_summary = feed_summary,
    );

    info!(
        "Running agent '{}' for topic '{}' (timeout: {}s)",
        command, topic.name, timeout_sec
    );

    let result = tokio::time::timeout(Duration::from_secs(timeout_sec), async {
        let output = Command::new(command)
            .arg("task")
            .arg(&prompt)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Agent(format!("Failed to execute '{}': {}", command, e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Agent process exited with error: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        parse_agent_output(&stdout)
    })
    .await;

    match result {
        Ok(Ok(article)) => {
            info!(
                "Agent generated article: {} (sources: {})",
                article.title,
                article.sources.len()
            );
            Ok(article)
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AppError::Timeout(format!(
            "Agent '{}' timed out after {}s",
            command, timeout_sec
        ))),
    }
}

fn format_feed_summary(items: &[crate::fetcher::FeedItem]) -> String {
    items
        .iter()
        .map(|item| {
            format!(
                "- {title}{desc}\n  Link: {link}",
                title = item.title,
                desc = item
                    .description
                    .as_ref()
                    .map(|d| format!("\n  {d}"))
                    .unwrap_or_default(),
                link = item.link,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_agent_output(output: &str) -> AppResult<ArticleResult> {
    // Try to find JSON block in the output
    let json_start = output.find('{');
    let json_end = output.rfind('}');

    let json_str = match (json_start, json_end) {
        (Some(start), Some(end)) if start < end => &output[start..=end],
        _ => {
            return Err(AppError::Agent(
                "No JSON object found in agent output".into(),
            ));
        }
    };

    serde_json::from_str::<ArticleResult>(json_str).map_err(|e| {
        AppError::Agent(format!(
            "Failed to parse agent JSON output: {}\nRaw: {}",
            e,
            json_str.chars().take(500).collect::<String>()
        ))
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_json() {
        let output = r#"{"title":"Test Article","content":"Test content","image_url":"https://example.com/img.jpg","sources":["Source 1"]}"#;
        let result = parse_agent_output(output).unwrap();
        assert_eq!(result.title, "Test Article");
        assert_eq!(result.sources.len(), 1);
    }

    #[test]
    fn test_parse_agent_with_markdown_fence() {
        let output = r#"Here's the result:
```json
{"title":"Test","content":"Body","image_url":null,"sources":["Src"]}
```
"#;
        let result = parse_agent_output(output).unwrap();
        assert_eq!(result.title, "Test");
    }
}
