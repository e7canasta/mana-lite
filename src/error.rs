use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManaError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("ingest error: {0}")]
    Ingest(String),

    #[error("inference error: {0}")]
    Inference(String),

    #[error("model not found in catalog: {0}")]
    ModelNotFound(String),

    #[error("model load failed: {model} ({reason})")]
    ModelLoadFailed { model: String, reason: String },

    #[error("zone not found: {0}")]
    ZoneNotFound(String),

    #[error("fsm state not found: {0}")]
    FsmStateNotFound(String),

    #[error("fsm guard error: {0}")]
    FsmGuardError(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("missing required field: {field}")]
    MissingField { field: String },

    #[error("invalid value for {field}: {msg}")]
    InvalidValue { field: String, msg: String },

    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("parse error in {file}: {msg}")]
    ParseError { file: String, msg: String },
}

pub type Result<T> = std::result::Result<T, ManaError>;
