use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("verification failed: {0}")]
    Verify(String),
    #[error("adapter error: {0}")]
    Adapter(String),
}

pub type Result<T> = std::result::Result<T, Error>;
