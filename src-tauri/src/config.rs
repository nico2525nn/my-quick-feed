use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use parking_lot::RwLock;
use tracing::warn;

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
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    #[serde(rename = "type")]
    pub source_type: String,
    pub url: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub discord: DiscordConfig,
    pub ai: AiConfig,
    pub topics: Vec<TopicConfig>,
}

impl AppConfig {
    pub fn load(path: &PathBuf) -> AppResult<Self> {
        if !path.exists() {
            warn!("Config file not found at {:?}, creating default", path);
            let default = Self::default();
            default.save(path)?;
            return Ok(default);
        }
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = serde_yaml::from_str(&content)?;
        Ok(config)
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
                model: None,
                base_url: None,
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
        {
            let mut guard = self.config.write();
            *guard = new_config.clone();
        }
        new_config.save(&self.path)?;
        Ok(())
    }
}
