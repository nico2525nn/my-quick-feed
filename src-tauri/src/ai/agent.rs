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
    // 事前にコマンドの存在を確認
    check_command_exists(command).map_err(|e| {
        AppError::Agent(format!(
            "{} is not installed or not on PATH. Install it with: cargo install {}\n  Detail: {}",
            command, if command == "omp" { "oh-my-pi-cli" } else { "opencode-cli" }, e
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
        "Running agent '{}' for topic '{}' (timeout: {}s, prompt: {} chars)",
        command, topic.name, timeout_sec, prompt.len()
    );

    // プロンプトが長すぎる場合はファイル経由、そうでなければ引数直接
    let result = if prompt.len() > 4000 {
        run_agent_via_stdin(command, &prompt, timeout_sec).await
    } else {
        run_agent_via_arg(command, &prompt, timeout_sec).await
    };

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

/// コマンドライン引数としてプロンプトを渡す（4000文字以下の場合）
async fn run_agent_via_arg(
    command: &str,
    prompt: &str,
    timeout_sec: u64,
) -> Result<AppResult<ArticleResult>, tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(timeout_sec), async {
        let output = Command::new(command)
            .arg("task")
            .arg(prompt)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Agent(format!("Failed to execute '{}': {}", command, e)))?;

        process_output(output, command)
    })
    .await
}

/// stdin 経由でプロンプトを渡す（長文プロンプト対策）
async fn run_agent_via_stdin(
    command: &str,
    prompt: &str,
    timeout_sec: u64,
) -> Result<AppResult<ArticleResult>, tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(timeout_sec), async {
        // 一時ファイルに書き出してリダイレクト
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("mqf_prompt_{}.txt", std::process::id()));
        let _display_path = tmp_path.display().to_string();

        match std::fs::write(&tmp_path, prompt) {
            Ok(_) => {
                let result = run_with_pipe_redirect(command, &tmp_path);
                let _ = std::fs::remove_file(&tmp_path);
                result
            }
            Err(e) => {
                // ファイル書き込み不可 → 直接引数で（失敗覚悟）
                warn!("Cannot write temp file ({}), falling back to direct arg", e);
                let output = Command::new(command)
                    .arg("task")
                    .arg(prompt)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .map_err(|e| AppError::Agent(format!("Failed to execute '{}': {}", command, e)))?;
                process_output(output, command)
            }
        }
    })
    .await
}

/// リダイレクトでプロンプトを渡す: cmd /c "type file | command task"
fn run_with_pipe_redirect(command: &str, file_path: &std::path::Path) -> AppResult<ArticleResult> {
    let file_path_str = file_path.to_string_lossy();

    // cmd /c "type file.txt | omp task"
    let shell_cmd = format!(
        "type \"{}\" | {} task",
        file_path_str.replace('/', "\\"),
        command
    );

    let output = Command::new("cmd")
        .args(["/c", &shell_cmd])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| AppError::Agent(format!("Failed to execute pipeline: {}", e)))?;

    process_output(output, command)
}

fn process_output(
    output: std::process::Output,
    command: &str,
) -> AppResult<ArticleResult> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        warn!(
            "Agent '{}' exited with status: {:?}\nstderr: {}\nstdout: {}",
            command,
            output.status.code(),
            stderr,
            stdout.chars().take(300).collect::<String>()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    parse_agent_output(&stdout)
}

/// コマンドが実行可能か確認（which 相当）
fn check_command_exists(command: &str) -> Result<(), String> {
    let success = if cfg!(windows) {
        Command::new("cmd")
            .args(["/c", "where", command])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("which")
            .arg(command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    if success {
        Ok(())
    } else {
        Err(format!("'{}' not found on PATH", command))
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
        let output = r#"{"title":"Test","content":"Body","image_url":"https://example.com/img.jpg","sources":["A"]}"#;
        let result = parse_agent_output(output).unwrap();
        assert_eq!(result.title, "Test");
    }

    #[test]
    fn test_parse_agent_with_markdown_fence() {
        let output = "```json\n{\"title\":\"T\",\"content\":\"B\",\"image_url\":null,\"sources\":[\"S\"]}\n```";
        let result = parse_agent_output(output).unwrap();
        assert_eq!(result.title, "T");
    }
}
