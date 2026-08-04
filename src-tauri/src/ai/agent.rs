use std::collections::HashSet;
use std::path::{Path, PathBuf};
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

/// トピックごとのセッションID を保存するファイル（%TEMP%\my-quick-feed\omp\session_<topic>.txt）
fn topic_session_file(work_dir: &Path, safe_name: &str) -> PathBuf {
    work_dir.join(format!("session_{}.txt", safe_name))
}

/// 保存済みセッションID を読み込む
fn read_topic_session_id(path: &Path) -> Option<String> {
    let id = std::fs::read_to_string(path).ok()?.trim().to_string();
    if id.is_empty() { None } else { Some(id) }
}

/// セッションID を保存する（失敗しても致命的ではないので warn のみ）
fn write_topic_session_id(path: &Path, id: &str) {
    if let Err(e) = std::fs::write(path, id) {
        warn!("OMPセッションID の保存に失敗: {}: {}", path.display(), e);
    }
}

/// セッションディレクトリ内の .jsonl ファイル一覧
fn list_session_files(dir: &Path) -> HashSet<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map_or(false, |x| x == "jsonl"))
                .collect()
        })
        .unwrap_or_default()
}

/// 実行後に新規作成されたセッションファイルからセッションID を抽出する
/// omp のセッションファイル名は <timestamp>_<uuid>.jsonl で、uuid がそのままセッションID
fn discover_session_id(dir: &Path, before: &HashSet<PathBuf>) -> Option<String> {
    let after = list_session_files(dir);
    let mut newest: Option<(std::time::SystemTime, String)> = None;
    for f in after.difference(before) {
        let mtime = match std::fs::metadata(f).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name = match f.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let rest = match name.strip_suffix(".jsonl") {
            Some(r) => r,
            None => continue,
        };
        let id = match rest.rsplit_once('_') {
            Some((_, id)) => id,
            None => continue,
        };
        if newest.as_ref().map_or(true, |(t, _)| mtime > *t) {
            newest = Some((mtime, id.to_string()));
        }
    }
    newest.map(|(_, id)| id)
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

    // 参考文献 URL リスト（空でなければ「## 参考文献」セクションとして埋め込む）
    // 空の場合は元のプロンプトと同じ改行だけ残す
    // reference_mode: "preload" = サイトのページを全部読んで知識を得てから書く（幻覚対策・事前学習）
    //                 "on-demand"（デフォルト）= URL のみ、エージェントが必要に応じて Web で確認
    let reference_block = if topic.reference_urls.is_empty() {
        "\n".to_string()
    } else {
        let urls = topic
            .reference_urls
            .iter()
            .map(|u| format!("- {}", u))
            .collect::<Vec<_>>()
            .join("\n");
        let mode = topic.reference_mode.as_deref().unwrap_or("on-demand");
        if mode == "preload" {
            format!(
                "\n## 参考文献（事前学習・必須）\n以下のサイトのページを可能な限り全て読み、トピック「{}」の正確な背景知識・最新情報を得てから記事を書いてください。記事の内容は、読んだ知識と矛盾させないこと。読んだ情報が記事の元記事と食い違う場合は、参考文献の知識を優先して正確に書くこと。\n{}\n",
                topic.name, urls
            )
        } else {
            format!(
                "\n## 参考文献（背景知識・正確性のための参考 URL。必要に応じて Web で確認してください）\n{}\n",
                urls
            )
        }
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
{reference_block}
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
        reference_block = reference_block,
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

    // セッション再利用（実験的）:
    // - omp のセッション保存先を --session-dir でアプリ専用ディレクトリに固定する
    //   （%USERPROFILE%\.omp の対話セッションには触れない）
    // - トピックごとのセッションID は session_<topic>.txt に保存し、
    //   あれば omp に --resume <id> を渡して同じセッションを継続する
    let session_dir = work_dir.join("omp-sessions");
    let _ = std::fs::create_dir_all(&session_dir);
    let session_file = topic_session_file(&work_dir, &safe_name);
    let resume_id = read_topic_session_id(&session_file);
    // 非resume実行時（初回・フォールバック）に新規セッションID を検出するためのスナップショット
    let session_files_before = list_session_files(&session_dir);

    let result = tokio::time::timeout(Duration::from_secs(timeout_sec), async {
        let exec_started = std::time::Instant::now();
        let output = if has_file {
            run_omp_file(command, &work_dir, &prompt_path, model, resume_id.as_deref(), &session_dir).await
        } else {
            run_omp_direct(command, &prompt, model).await
        };
        let elapsed = exec_started.elapsed();
        match &output {
            Ok((articles, _)) => {
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
        Ok(Ok((articles, resume_used))) => {
            if resume_used {
                info!(topic = %topic.name, "OMPセッションを再利用（resume）");
            } else if let Some(id) = discover_session_id(&session_dir, &session_files_before) {
                // 新規セッションが作られたので、次回以降の resume 用に保存する
                info!(topic = %topic.name, "新規OMPセッションID を保存: {}", id);
                write_topic_session_id(&session_file, &id);
            }
            info!(topic = %topic.name, "Agent returned {} articles", articles.len());
            Ok(articles)
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AppError::Timeout(format!(
            "Agent timed out after {}s", timeout_sec
        ))),
    }
}

/// OMP を1回実行する（resume_id があれば --resume を付ける）
async fn run_omp_once(
    command: &str,
    work_dir: &Path,
    prompt_path: &Path,
    model: &str,
    session_dir: &Path,
    resume_id: Option<&str>,
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
    // セッション保存先をアプリ専用ディレクトリに固定
    // （%USERPROFILE%\.omp の対話セッションには触れない）
    let session_dir_str = session_dir.to_string_lossy().into_owned();
    cmd.args(["--session-dir", session_dir_str.as_str()]);
    // 保存済みセッションID があれば resume して同じセッションを継続する
    if let Some(id) = resume_id {
        cmd.args(["-r", id]);
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

async fn run_omp_file(
    command: &str,
    work_dir: &PathBuf,
    prompt_path: &PathBuf,
    model: &str,
    resume_id: Option<&str>,
    session_dir: &Path,
) -> AppResult<(Vec<ArticleResult>, bool)> {
    // resume 実行 → 失敗時は通常実行にフォールバックしてパイプラインを止めない
    // （戻り値の bool は「resume が使われたか」。resume は同一セッションに追記されるため
    //   ID の再保存は不要。false のときは新規セッションID を検出して保存する）
    let first = run_omp_once(command, work_dir, prompt_path, model, session_dir, resume_id).await;
    match first {
        Ok(articles) => Ok((articles, resume_id.is_some())),
        Err(e) if resume_id.is_some() => {
            warn!("resume 失敗のため通常実行にフォールバック: {}", e);
            let articles = run_omp_once(command, work_dir, prompt_path, model, session_dir, None).await?;
            Ok((articles, false))
        }
        Err(e) => Err(e),
    }
}

async fn run_omp_direct(command: &str, prompt: &str, model: &str) -> AppResult<(Vec<ArticleResult>, bool)> {
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
    parse_omp_output(output).map(|a| (a, false))
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
