use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManaError {
    #[error("config: {0}")]
    Config(#[from] ConfigError),

    #[error("ingest: {0}")]
    Ingest(String),

    #[error("inference: {0}")]
    Inference(String),

    #[error("model not found in catalog: {0}")]
    ModelNotFound(String),

    #[error("zone not found: {0}")]
    ZoneNotFound(String),

    #[error("fsm state not found: {0}")]
    FsmStateNotFound(String),

    #[error("fsm guard: {0}")]
    FsmGuardError(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("parse error in {file}: {msg}")]
    ParseError { file: String, msg: String },

    #[error("validation: {field}: {msg}")]
    InvalidValue { field: String, msg: String },
}

pub type Result<T> = std::result::Result<T, ManaError>;
