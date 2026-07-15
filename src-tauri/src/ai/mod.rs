pub mod agent;
pub mod direct;

pub use agent::*;
pub use direct::*;

use serde::{Deserialize, Serialize};

/// Agent/Direct モード共通の生成結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleResult {
    pub title: String,
    pub content: String,
    pub image_url: Option<String>,
    pub sources: Vec<String>,
}

/// トピック名を使ってデフォルトのシステムプロンプトを生成
/// `custom_prompt` が Some かつ空でなければそれを優先
pub fn resolve_system_prompt(topic_name: &str, language: &str, custom_prompt: Option<&str>) -> String {
    match custom_prompt {
        Some(p) if !p.trim().is_empty() => p.to_string(),
        _ => generate_default_prompt(topic_name, language),
    }
}

/// トピック名と言語からテンプレートプロンプトを生成
fn generate_default_prompt(topic_name: &str, language: &str) -> String {
    match language {
        "ja" => format!(
            r#"あなたは{}に関するニュース記事をまとめるアシスタントです。
以下の情報源から収集した情報を基に、簡潔なニュース記事を1件生成してください。
タイトルは「【{}】」で始め、本文は300字程度にまとめてください。
出典を明記し、複数のソースを統合する場合はその旨も記載してください。"#,
            topic_name, topic_name
        ),
        "en" => format!(
            r#"You are an assistant that summarizes news about {}.
Create one concise news article based on the information collected from the sources below.
Start the title with "【{}】" and keep the body around 300 characters.
Cite your sources and mention when multiple sources are combined."#,
            topic_name, topic_name
        ),
        _ => format!(
            r#"You are an assistant that summarizes news about {}.
Create one concise news article based on the information provided.
Start the title with "【{}】" and cite your sources."#,
            topic_name, topic_name
        ),
    }
}
