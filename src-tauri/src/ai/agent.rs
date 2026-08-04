use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::{info, warn};
use crate::ai::{ArticleResult, ArticleListResult, resolve_system_prompt};
use crate::config::TopicConfig;
use crate::errors::{AppError, AppResult};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// OMP 作業ディレクトリ（セッション分離用）
fn omp_work_dir() -> PathBuf {
    std::env::temp_dir().join("my-quick-feed").join("omp")
}

/// OMP CLI を呼び出し、記事リストを取得する
pub async fn run_agent(
    command: &str,
    model: &str,
    timeout_sec: u64,
    topic: &TopicConfig,
    feed_items: &[crate::fetcher::FeedItem],
    recent_titles: &[String],
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

    // 直近投稿タイトル一覧（重複防止）
    let recent_block = if recent_titles.is_empty() {
        "（なし）".to_string()
    } else {
        recent_titles.iter().map(|t| format!("- {}", t)).collect::<Vec<_>>().join("\n")
    };

    // プロンプト本体に記事リストも含める（OMP -p は複数 @file に非対応のため）
    let prompt = format!(
        r#"あなたはニュース記事を生成するアシスタントです。

## トピック
{topic_name}

## 言語
{language}

## 使用モデル
{model}

## 既に投稿済みのトピック（重複防止用）
{recent_block}

## システム指示
{system_prompt}

## 元記事
{feed_summary}

## 出力形式
注目すべきニュースそれぞれに対して記事を生成し、JSON配列で出力してください。
既に投稿済みのトピックと内容が完全に重複する場合はスキップしてください。
JSON以外の出力は絶対に含めないでください。
[
  {{
    "title": "記事タイトル",
    "content": "記事本文（300字程度）",
    "image_url": "関連画像URL（あれば）",
    "tags": ["タグ1", "タグ2"],
    "sources": ["出典1", "出典2"]
  }}
]
重要でない記事はスキップして構いません。"#,
        topic_name = topic.name,
        language = language,
        model = model,
        recent_block = recent_block,
        system_prompt = system_prompt,
        feed_summary = feed_summary,
    );

    info!(
        topic = %topic.name, "Running agent [cmd={}, model={}, timeout={}s, prompt={}chars, articles={}chars]",
        command, model, timeout_sec, prompt.len(), feed_summary.len()
    );

    // OMP 作業ディレクトリを分離（セッション履歴の汚染防止）
    let work_dir = omp_work_dir();
    let _ = std::fs::create_dir_all(&work_dir);

    // プロンプトファイル（記事リスト込み）を作業ディレクトリに書き込み
    // 複数トピックの並行実行で上書きし合わないよう、トピック名+タイムスタンプでユニークにする
    let safe_name: String = topic
        .name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let prompt_path = work_dir.join(format!("{}-{}.txt", safe_name, ts));
    let has_file = std::fs::write(&prompt_path, &prompt).is_ok();

    let result = tokio::time::timeout(Duration::from_secs(timeout_sec), async {
        let exec_started = std::time::Instant::now();
        let output = if has_file {
            run_omp_file(command, &work_dir, &prompt_path, model).await
        } else {
            run_omp_direct(command, &prompt, model).await
        };
        let elapsed = exec_started.elapsed();
        match &output {
            Ok(articles) => {
                info!(
                    topic = %topic.name,
                    "OMP実行完了: {} 記事, {:.1}s",
                    articles.len(),
                    elapsed.as_secs_f64()
                );
            }
            Err(e) => {
                warn!(
                    topic = %topic.name,
                    "OMP実行失敗: {} ({:.1}s)",
                    e,
                    elapsed.as_secs_f64()
                );
            }
        }
        let _ = std::fs::remove_file(&prompt_path);
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

async fn run_omp_file(
    command: &str,
    work_dir: &PathBuf,
    prompt_path: &PathBuf,
    model: &str,
) -> AppResult<Vec<ArticleResult>> {
    let mut cmd = Command::new(command);
    cmd.args(["-p"]);
    // モデルを明示指定（プロンプト内の「## 使用モデル」だけでは確実でない）
    if !model.is_empty() && model != "default" {
        cmd.args(["--model", model]);
    }
    // マルチモーダル/対話型モデル（mimo等）はタスク実行を明示しないと
    // 「何をしたいですか？」と確認応答をするため、システムプロンプトで強制する
    if model.contains("mimo") {
        cmd.args([
            "--append-system-prompt",
            "あなたはタスク実行エージェントです。ユーザーが渡したファイルや指示は実行すべきタスクです。指示に従って実行し、要求された出力のみを返してください。ユーザーに確認したり質問したりしないでください。",
        ]);
    }
    cmd.arg(format!("@{}", prompt_path.to_string_lossy()));
    cmd.current_dir(work_dir); // セッション紐づけ先を分離
    cmd.kill_on_drop(true); // timeout 時は子プロセスを kill
    hide_window(&mut cmd);
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| AppError::Agent(format!("omp exec failed: {}", e)))?;
    parse_omp_output(output)
}

async fn run_omp_direct(command: &str, prompt: &str, model: &str) -> AppResult<Vec<ArticleResult>> {
    let mut cmd = Command::new(command);
    cmd.args(["-p"]);
    if !model.is_empty() && model != "default" {
        cmd.args(["--model", model]);
    }
    if model.contains("mimo") {
        cmd.args([
            "--append-system-prompt",
            "あなたはタスク実行エージェントです。ユーザーが渡したファイルや指示は実行すべきタスクです。指示に従って実行し、要求された出力のみを返してください。ユーザーに確認したり質問したりしないでください。",
        ]);
    }
    cmd.arg(prompt);
    cmd.current_dir(omp_work_dir());
    cmd.kill_on_drop(true);
    hide_window(&mut cmd);
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| AppError::Agent(format!("omp exec failed: {}", e)))?;
    parse_omp_output(output)
}

#[cfg(target_os = "windows")]
fn hide_window(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(target_os = "windows"))]
fn hide_window(_cmd: &mut Command) {}

fn parse_omp_output(output: std::process::Output) -> AppResult<Vec<ArticleResult>> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            "omp exit: {:?}\nstderr: {}\nstdout先頭: {}",
            output.status.code(),
            stderr,
            String::from_utf8_lossy(&output.stdout).chars().take(200).collect::<String>()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    info!("omp stdout: {} bytes", stdout.len());

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

    if let (Some(s), Some(e)) = (last_text.find('['), last_text.rfind(']')) {
        if s < e {
            if let Ok(list) = serde_json::from_str::<Vec<ArticleResult>>(&last_text[s..=e]) {
                if !list.is_empty() { return Some(list); }
            }
        }
    }
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
    std::process::Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status()
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
