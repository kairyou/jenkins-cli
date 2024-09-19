use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<GlobalConfig>,
    pub jenkins: Vec<JenkinsConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GlobalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_history: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct JenkinsConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub token: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<String>,
}

pub struct RuntimeConfig {
    #[allow(dead_code)]
    pub global: Option<GlobalConfig>,
    pub jenkins: JenkinsConfig,
}
