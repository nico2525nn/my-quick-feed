use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use parking_lot::RwLock;
use tracing::{error, warn};

use crate::errors::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub token: String,
    pub forum_channel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub mode: String,
    pub agent_command: Option<String>,
    pub agent_timeout_sec: Option<u64>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    /// OMP の --thinking レベル（mimo-v2.5 は low/medium/high のみ対応。デフォルト high）
    #[serde(default)]
    pub thinking_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    #[serde(rename = "type")]
    pub source_type: String,
    pub url: String,
    /// RSSHUB用: ベースURL（省略時は rsshub.app）
    #[serde(default)]
    pub base_url: Option<String>,
    /// RSSHUB用: パス（例: /twitter/user/PlayApex）
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicConfig {
    pub name: String,
    pub language: Option<String>,
    pub interval_min: u64,
    pub sources: Vec<SourceConfig>,
    pub system_prompt: Option<String>,
    pub image_search_enabled: Option<bool>,
    pub research_enabled: Option<bool>,
    /// トピック専用のフォーラムチャンネル ID（未指定なら discord.forum_channel_id を使う）
    #[serde(default)]
    pub forum_channel_id: Option<String>,
    /// 参考文献 URL リスト（Wiki 等。プロンプトに埋め込む）
    #[serde(default)]
    pub reference_urls: Vec<String>,
    /// 参考文献の扱い: "preload" = 事前に内容を読み込んでプロンプトに埋め込む（幻覚対策・知識保持）、
    /// "on-demand"（デフォルト）= URL のみ埋め込み、エージェントが必要に応じて Web で読む
    #[serde(default)]
    pub reference_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub discord: DiscordConfig,
    pub ai: AiConfig,
    pub topics: Vec<TopicConfig>,
}

impl AppConfig {
    pub fn load(path: &PathBuf) -> AppResult<Self> {
        match std::fs::read_to_string(path) {
            // ファイルが無い/読めない → デフォルトを保存して返す（初回起動の正常系）
            Err(_) => {
                warn!("Config file not found at {:?}, creating default", path);
                let default = Self::default();
                match default.save(path) {
                    Ok(()) => Ok(default),
                    Err(e) => {
                        // 保存できなくても起動は続行する（設定画面から再設定可能）
                        error!("Failed to save default config at {:?}: {}", path, e);
                        Ok(default)
                    }
                }
            }
            Ok(content) => {
                // BOM 除去（PowerShell 等で保存された BOM 付き UTF-8 は serde_yaml が読めない）
                let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
                match serde_yaml::from_str::<AppConfig>(content) {
                    Ok(config) => Ok(config),
                    Err(e) => {
                        // パース失敗でクラッシュせず、デフォルトで続行（壊れた設定は上書きしない）
                        error!(
                            "Failed to parse config at {:?}: {} — using default (file left untouched)",
                            path, e
                        );
                        Ok(Self::default())
                    }
                }
            }
        }
    }

    pub fn save(&self, path: &PathBuf) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            discord: DiscordConfig {
                token: String::new(),
                forum_channel_id: String::new(),
            },
            ai: AiConfig {
                mode: "agent".to_string(),
                agent_command: Some("omp".to_string()),
                agent_timeout_sec: Some(120),
                api_key: None,
                model: Some("mimo-v2.5".to_string()),
                provider: Some("opencode-go".to_string()),
                base_url: None,
                // mimo-v2.5 は low/medium/high のみ対応。深く考えさせるため high をデフォルトに
                thinking_level: Some("high".to_string()),
            },
            topics: vec![],
        }
    }
}

pub struct ConfigManager {
    pub config: RwLock<AppConfig>,
    path: PathBuf,
}

impl ConfigManager {
    pub fn new(path: PathBuf, config: AppConfig) -> Self {
        Self {
            config: RwLock::new(config),
            path,
        }
    }

    pub fn load(path: PathBuf) -> AppResult<Self> {
        let config = AppConfig::load(&path)?;
        Ok(Self::new(path, config))
    }
    pub fn get(&self) -> AppConfig {
        self.config.read().clone()
    }

    pub fn update(&self, new_config: AppConfig) -> AppResult<()> {
        // 先にファイルへ保存してからメモリを更新する（save 失敗時に
        // メモリとファイルが不一致のまま残るのを防ぐ）
        new_config.save(&self.path)?;
        *self.config.write() = new_config;
        Ok(())
    }
}
