use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};
use crate::ai::{ArticleResult, resolve_system_prompt};
use crate::config::TopicConfig;
use crate::errors::{AppError, AppResult};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub async fn run_agent(
    command: &str,
    model: &str,
    timeout_sec: u64,
    topic: &TopicConfig,
    feed_items: &[crate::fetcher::FeedItem],
) -> AppResult<ArticleResult> {
    check_command_exists(command).map_err(|e| {
        AppError::Agent(format!(
            "{} が見つかりません。npm install -g oh-my-pi-cli 等でインストールしてください。\n  Detail: {}",
            command, e
        ))
    })?;

    let feed_summary = format_feed_summary(feed_items);
    let language = topic.language.as_deref().unwrap_or("ja");
    let system_prompt = resolve_system_prompt(&topic.name, language, topic.system_prompt.as_deref());

    let prompt = format!(
        r#"あなたはニュース記事を生成するアシスタントです。

## トピック
{topic_name}

## 言語
{language}

## 使用モデル
{model}

## システム指示
{system_prompt}

## 元記事
{feed_summary}

## 出力形式
以下のJSON形式のみを出力してください。余計な説明は含めないでください。
{{
  "title": "記事タイトル（{language}で）",
  "content": "記事本文（300字程度）",
  "image_url": "関連画像URL（あれば）",
  "sources": ["出典1", "出典2"]
}}
"#,
        topic_name = topic.name,
        language = language,
        model = model,
        system_prompt = system_prompt,
        feed_summary = feed_summary,
    );

    info!(
        topic = %topic.name, "Running agent [cmd={}, model={}, timeout={}s, prompt={}chars]",
        command, model, timeout_sec, prompt.len()
    );

    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("mqf_prompt_{}.txt", std::process::id()));
    let has_file = std::fs::write(&tmp_path, &prompt).is_ok();

    let result = tokio::time::timeout(Duration::from_secs(timeout_sec), async {
        let output = if has_file {
            run_omp_file(&tmp_path)
        } else {
            run_omp_direct(&prompt)
        };
        if has_file {
            std::fs::remove_file(&tmp_path).ok();
        }
        output
    })
    .await;

    match result {
        Ok(Ok(article)) => {
            info!(topic = %topic.name, "Agent OK: \"{}\"", article.title);
            Ok(article)
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AppError::Timeout(format!(
            "Agent timed out after {}s", timeout_sec
        ))),
    }
}

/// omp -p @file.txt （デフォルトテキストモード）
fn run_omp_file(tmp_path: &std::path::Path) -> AppResult<ArticleResult> {
    let mut cmd = Command::new("omp");
    cmd.args(["-p"]);
    cmd.arg(format!("@{}", tmp_path.to_string_lossy()));
    hide_window(&mut cmd);

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AppError::Agent(format!("omp exec failed: {}", e)))?;

    process_omp_output(output)
}

/// omp -p "prompt"（短いプロンプト用）
fn run_omp_direct(prompt: &str) -> AppResult<ArticleResult> {
    let mut cmd = Command::new("omp");
    cmd.args(["-p", prompt]);
    hide_window(&mut cmd);

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AppError::Agent(format!("omp exec failed: {}", e)))?;

    process_omp_output(output)
}

#[cfg(target_os = "windows")]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_window(_cmd: &mut Command) {}

/// OMPの標準出力から記事JSONを抽出
fn process_omp_output(output: std::process::Output) -> AppResult<ArticleResult> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("omp exit: {:?}\nstderr: {}", output.status.code(), stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    // OMPは複数行のJSONL（session protocol）を出力する場合がある
    // 各行から `{` で始まり `}` で終わるJSONを探す
    // 最終アシスタントメッセージに含まれる記事JSONを抽出

    // まず標準的なJSONブロックを探す（"title"を含むもの）
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Ok(article) = serde_json::from_str::<ArticleResult>(trimmed) {
            return Ok(article);
        }
    }

    // 全体から `{` ～ `}` のJSONブロックを探す（マークダウンfence内など）
    let json_start = stdout.find('{');
    let json_end = stdout.rfind('}');
    if let (Some(s), Some(e)) = (json_start, json_end) {
        if s < e {
            let candidate = &stdout[s..=e];
            if let Ok(article) = serde_json::from_str::<ArticleResult>(candidate) {
                return Ok(article);
            }
        }
    }

    // OMPのsession JSONをパースして assistant メッセージを探す
    if let Some(article) = extract_from_omp_jsonl(&stdout) {
        return Ok(article);
    }

    let preview = stdout.chars().take(300).collect::<String>();
    Err(AppError::Agent(format!(
        "Could not extract article JSON from omp output. Preview: {}",
        preview
    )))
}

/// OMPのJSONL（session protocol）から最終アシスタント応答を抽出してパース
fn extract_from_omp_jsonl(output: &str) -> Option<ArticleResult> {
    let mut last_content = String::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if val.get("type").and_then(|v| v.as_str()) == Some("message_stop") {
                if let Some(content) = val
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                {
                    for block in content {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            last_content.push_str(text);
                        }
                    }
                }
            }
        }
    }
    if !last_content.is_empty() {
        // 抽出したテキストからJSONを探す
        if let Ok(article) = serde_json::from_str::<ArticleResult>(&last_content) {
            return Some(article);
        }
        let s = last_content.find('{')?;
        let e = last_content.rfind('}')?;
        if s < e {
            serde_json::from_str(&last_content[s..=e]).ok()
        } else {
            None
        }
    } else {
        None
    }
}

fn check_command_exists(command: &str) -> Result<(), String> {
    let (cmd, args): (&str, &[&str]) = if cfg!(windows) {
        ("cmd", &["/c", "where", command])
    } else {
        ("which", &[command])
    };
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| if s.success() { Ok(()) } else { Err(format!("'{}' not found on PATH", command)) })
        .unwrap_or(Err(format!("Failed to check '{}'", command)))
}

fn format_feed_summary(items: &[crate::fetcher::FeedItem]) -> String {
    items.iter()
        .map(|item| format!("- {title}{desc}\n  Link: {link}",
            title = item.title,
            desc = item.description.as_ref().map(|d| format!("\n  {d}")).unwrap_or_default(),
            link = item.link))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_from_jsonl() {
        let j = r#"{"type":"message_stop","message":{"role":"assistant","content":[{"type":"text","text":"{\"title\":\"T\",\"content\":\"B\",\"image_url\":null,\"sources\":[\"S\"]}"}]}}"#;
        let r = extract_from_omp_jsonl(j).unwrap();
        assert_eq!(r.title, "T");
    }

    #[test]
    fn test_parse_direct_json() {
        let o = r#"{"title":"T","content":"B","image_url":null,"sources":["S"]}"#;
        assert_eq!(parse_agent_output(o).unwrap().title, "T");
    }
}

// 後方互換のため残す
fn parse_agent_output(output: &str) -> AppResult<ArticleResult> {
    // JSONL → session JSON → 直接JSON の順で試行
    if let Ok(article) = serde_json::from_str::<ArticleResult>(output) {
        return Ok(article);
    }
    let s = output.find('{');
    let e = output.rfind('}');
    if let (Some(s), Some(e)) = (s, e) {
        if s < e {
            if let Ok(article) = serde_json::from_str::<ArticleResult>(&output[s..=e]) {
                return Ok(article);
            }
        }
    }
    if let Some(article) = extract_from_omp_jsonl(output) {
        return Ok(article);
    }
    Err(AppError::Agent(format!("No article JSON found. Preview: {}", output.chars().take(300).collect::<String>())))
}
