use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};
use crate::ai::{ArticleResult, ArticleListResult, resolve_system_prompt};
use crate::config::TopicConfig;
use crate::errors::{AppError, AppResult};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// OMP CLI を呼び出し、記事リストを取得する
pub async fn run_agent(
    command: &str,
    model: &str,
    timeout_sec: u64,
    topic: &TopicConfig,
    feed_items: &[crate::fetcher::FeedItem],
) -> AppResult<Vec<ArticleResult>> {
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
注目すべきニュースそれぞれに対して記事を生成し、JSON配列で出力してください。
JSON以外の出力は絶対に含めないでください。
[
  {{
    "title": "記事タイトル",
    "content": "記事本文（300字程度）",
    "image_url": "関連画像URL（あれば）",
    "sources": ["出典1", "出典2"]
  }}
]
重要でない記事はスキップして構いません。"#,
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
        Ok(Ok(articles)) => {
            info!(topic = %topic.name, "Agent returned {} articles", articles.len());
            Ok(articles)
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AppError::Timeout(format!(
            "Agent timed out after {}s", timeout_sec
        ))),
    }
}

fn run_omp_file(tmp_path: &std::path::Path) -> AppResult<Vec<ArticleResult>> {
    let mut cmd = Command::new("omp");
    cmd.args(["-p"]);
    cmd.arg(format!("@{}", tmp_path.to_string_lossy()));
    hide_window(&mut cmd);
    let output = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).output()
        .map_err(|e| AppError::Agent(format!("omp exec failed: {}", e)))?;
    parse_omp_output(output)
}

fn run_omp_direct(prompt: &str) -> AppResult<Vec<ArticleResult>> {
    let mut cmd = Command::new("omp");
    cmd.args(["-p", prompt]);
    hide_window(&mut cmd);
    let output = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).output()
        .map_err(|e| AppError::Agent(format!("omp exec failed: {}", e)))?;
    parse_omp_output(output)
}

#[cfg(target_os = "windows")]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(target_os = "windows"))]
fn hide_window(_cmd: &mut Command) {}

fn parse_omp_output(output: std::process::Output) -> AppResult<Vec<ArticleResult>> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("omp exit: {:?}\nstderr: {}", output.status.code(), stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // 1) 直接JSON配列としてパース
    if let Ok(list) = serde_json::from_str::<Vec<ArticleResult>>(&stdout) {
        if !list.is_empty() {
            return Ok(list);
        }
    }

    // 2) ArticleListResult でラップされた形式
    if let Ok(wrapped) = serde_json::from_str::<ArticleListResult>(&stdout) {
        if !wrapped.articles.is_empty() {
            return Ok(wrapped.articles);
        }
    }

    // 3) `[` から `]` までを抽出
    if let (Some(s), Some(e)) = (stdout.find('['), stdout.rfind(']')) {
        if s < e {
            let json = &stdout[s..=e];
            if let Ok(list) = serde_json::from_str::<Vec<ArticleResult>>(json) {
                if !list.is_empty() {
                    return Ok(list);
                }
            }
        }
    }

    // 4) 単一ArticleResult → Vec
    if let (Some(s), Some(e)) = (stdout.find('{'), stdout.rfind('}')) {
        if s < e {
            if let Ok(article) = serde_json::from_str::<ArticleResult>(&stdout[s..=e]) {
                return Ok(vec![article]);
            }
        }
    }

    // 5) OMP session JSONから抽出
    if let Some(articles) = extract_from_omp_session(&stdout) {
        return Ok(articles);
    }

    let preview = stdout.chars().take(300).collect::<String>();
    Err(AppError::Agent(format!("Could not extract articles. Preview: {}", preview)))
}

/// OMP session protocol JSONL からアシスタント応答を抽出
fn extract_from_omp_session(output: &str) -> Option<Vec<ArticleResult>> {
    let mut last_text = String::new();
    for line in output.lines() {
        let t = line.trim();
        if !t.starts_with('{') { continue; }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            if v.get("type").and_then(|x| x.as_str()) == Some("message_stop") {
                if let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                    for block in content {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            last_text.push_str(text);
                        }
                    }
                }
            }
        }
    }
    if last_text.is_empty() { return None; }

    // 配列
    if let (Some(s), Some(e)) = (last_text.find('['), last_text.rfind(']')) {
        if s < e {
            if let Ok(list) = serde_json::from_str::<Vec<ArticleResult>>(&last_text[s..=e]) {
                if !list.is_empty() { return Some(list); }
            }
        }
    }
    // 単一
    if let (Some(s), Some(e)) = (last_text.find('{'), last_text.rfind('}')) {
        if s < e {
            if let Ok(article) = serde_json::from_str::<ArticleResult>(&last_text[s..=e]) {
                return Some(vec![article]);
            }
        }
    }
    None
}

fn check_command_exists(command: &str) -> Result<(), String> {
    let (cmd, args): (&str, &[&str]) = if cfg!(windows) {
        ("cmd", &["/c", "where", command])
    } else {
        ("which", &[command])
    };
    Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status()
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
