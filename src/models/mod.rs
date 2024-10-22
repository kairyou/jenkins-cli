use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<GlobalConfig>,
    #[serde(default)]
    pub jenkins: Vec<JenkinsConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GlobalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>, // display language
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_history: Option<bool>, // enable history recording(build parameters)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_update: Option<bool>, // enable update check
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_history: Option<bool>, // override global setting
}

#[derive(Debug)]
pub struct RuntimeConfig {
    pub global: Option<GlobalConfig>,
    pub jenkins: JenkinsConfig,       // current selected jenkins config
    pub services: Vec<JenkinsConfig>, // all available jenkins configs
}
