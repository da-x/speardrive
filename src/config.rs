use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub composites_cache: PathBuf,
    pub local_cache: PathBuf,
    pub gitlabs: BTreeMap<String, GitlabJobArtifacts>,
    pub listen_addr: String,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct GitlabJobArtifacts {
    pub api_key: String,
    pub hostname: String,
}
