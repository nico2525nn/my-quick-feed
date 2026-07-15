use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("RSS parse error: {0}")]
    RssParse(String),

    #[error("AI agent error: {0}")]
    Agent(String),

    #[error("AI API error: {0}")]
    Api(String),

    #[error("Discord error: {0}")]
    Discord(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_yaml::Error),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("{0}")]
    Other(String),
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}

pub type AppResult<T> = Result<T, AppError>;
