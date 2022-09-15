use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub composites_cache: PathBuf,
    pub gitlabs: Vec<GitlabJobArtifacts>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct GitlabJobArtifacts {
    pub name: String,
    pub api_key: String,
    pub hostname: String,
    pub local_cache: PathBuf,
}
