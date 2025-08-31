use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<GlobalConfig>, // global config(`config` @file)
    #[serde(default)]
    pub services: Vec<JenkinsConfig>, // all jenkins services(`jenkins` @file)
    #[serde(skip)]
    pub jenkins: Option<JenkinsConfig>, // current selected jenkins service
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>, // HTTP request timeout in seconds, default 30
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
