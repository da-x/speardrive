use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub composites_cache: PathBuf,
    pub local_cache: PathBuf,
    pub listen_addr: String,

    #[serde(default)]
    pub gitlabs: BTreeMap<String, GitlabJobSource>,

    #[serde(default)]
    pub local_source: BTreeMap<String, LocalPathSource>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct GitlabJobSource {
    pub api_key: String,
    pub hostname: String,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocalPathSource {
    pub root: PathBuf,
}
