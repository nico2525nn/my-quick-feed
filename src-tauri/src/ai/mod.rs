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
