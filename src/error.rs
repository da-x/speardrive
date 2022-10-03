use std::{num::ParseIntError, sync::Arc};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Io error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("config file")]
    ConfigFile,

    #[error("Config error: {0}")]
    ConfigError(#[from] config::ConfigError),

    #[error("Toml error: {0}")]
    TomlError(#[from] toml::ser::Error),

    #[error("YAML error: {0}")]
    YAMLError(#[from] serde_yaml::Error),

    #[error("ParseIntError error: {0}")]
    ParseIntError(#[from] ParseIntError),

    #[error("Query builder error: {0}")]
    BuilderError(String),

    #[error("Gitlab error; {0}")]
    GitlabError(#[from] gitlab::GitlabError),

    #[error("")]
    Help,

    #[error("Error in logging: {0}")]
    Logging(#[from] crate::logging::Error),

    #[error("Invalid URI parts: {0}")]
    InvalidURIParts(#[from] hyper::http::uri::InvalidUriParts),

    #[error("Plan parse error: {0}")]
    PlanParse(String),

    #[error("Unknown source: {0}")]
    UnknownSource(String),

    #[error("{0}")]
    Boxed(Arc<dyn std::error::Error + Send + Sync + 'static>),

    #[error("Command error: {0} {1}")]
    CommandError(String, String),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),
}
