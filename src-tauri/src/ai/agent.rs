use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};
use crate::ai::{ArticleResult, resolve_system_prompt};
use crate::config::TopicConfig;
use crate::errors::{AppError, AppResult};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// OMP CLI を子プロセスとして呼び出し、エージェントに記事生成を委託する
pub async fn run_agent(
    command: &str,
    model: &str,
    timeout_sec: u64,
    topic: &TopicConfig,
    feed_items: &[crate::fetcher::FeedItem],
) -> AppResult<ArticleResult> {
    check_command_exists(command).map_err(|e| {
        AppError::Agent(format!(
            "{} が見つかりません。npm install -g {} 等でインストールしてください。\n  Detail: {}",
            command,
            if command == "omp" { "oh-my-pi-cli" } else { command },
            e
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
        model = model,
        system_prompt = system_prompt,
        feed_summary = feed_summary,
    );

    info!(
        topic = %topic.name, "Running agent [cmd={}, model={}, timeout={}s, prompt={}chars]",
        command, model, timeout_sec, prompt.len()
    );

    // プロンプトを一時ファイルに書き出し
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("mqf_prompt_{}.txt", std::process::id()));
    let has_file = std::fs::write(&tmp_path, &prompt).is_ok();

    let result = tokio::time::timeout(Duration::from_secs(timeout_sec), async {
        let output = if has_file {
            // omp -p --mode json @file.txt
            run_omp(&tmp_path)
        } else {
            // ファイル不可 → 直接引数
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
            info!(
                topic = %topic.name,
                "Agent generated: \"{}\" ({} sources)",
                article.title, article.sources.len()
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

/// omp -p --mode json @file.txt
fn run_omp(tmp_path: &std::path::Path) -> AppResult<ArticleResult> {
    let mut cmd = Command::new("omp");
    cmd.args(["-p", "--mode", "json"]);
    cmd.arg(format!("@{}", tmp_path.to_string_lossy()));
    hide_window(&mut cmd);

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AppError::Agent(format!("Failed to execute omp: {}", e)))?;

    process_output(output)
}

/// omp -p --mode json "prompt"（短いプロンプト用）
fn run_omp_direct(prompt: &str) -> AppResult<ArticleResult> {
    let mut cmd = Command::new("omp");
    cmd.args(["-p", "--mode", "json", prompt]);
    hide_window(&mut cmd);

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AppError::Agent(format!("Failed to execute omp: {}", e)))?;

    process_output(output)
}

#[cfg(target_os = "windows")]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_window(_cmd: &mut Command) {}

fn process_output(output: std::process::Output) -> AppResult<ArticleResult> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            "omp exit: {:?}\nstderr: {}",
            output.status.code(),
            stderr
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // --mode json の場合、OMPはJSON行を出力する。最初の { から } までを探す
    parse_agent_output(&stdout)
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
        .map(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(format!("'{}' not found on PATH", command))
            }
        })
        .unwrap_or(Err(format!("Failed to check '{}'", command)))
}

fn format_feed_summary(items: &[crate::fetcher::FeedItem]) -> String {
    items
        .iter()
        .map(|item| {
            format!(
                "- {title}{desc}\n  Link: {link}",
                title = item.title,
                desc = item.description.as_ref().map(|d| format!("\n  {d}")).unwrap_or_default(),
                link = item.link,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_agent_output(output: &str) -> AppResult<ArticleResult> {
    let json_start = output.find('{');
    let json_end = output.rfind('}');

    let json_str = match (json_start, json_end) {
        (Some(start), Some(end)) if start < end => &output[start..=end],
        _ => {
            let preview = output.chars().take(300).collect::<String>();
            return Err(AppError::Agent(format!(
                "No JSON in omp output. Preview: {}",
                preview
            )));
        }
    };

    serde_json::from_str::<ArticleResult>(json_str).map_err(|e| {
        AppError::Agent(format!(
            "Failed to parse JSON: {}\nRaw: {}",
            e,
            json_str.chars().take(500).collect::<String>()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json() {
        let o = r#"{"title":"T","content":"B","image_url":null,"sources":["S"]}"#;
        let r = parse_agent_output(o).unwrap();
        assert_eq!(r.title, "T");
    }

    #[test]
    fn test_parse_json_in_markdown() {
        let o = "```json\n{\"title\":\"T\",\"content\":\"B\",\"image_url\":null,\"sources\":[\"S\"]}\n```";
        let r = parse_agent_output(o).unwrap();
        assert_eq!(r.title, "T");
    }
}
