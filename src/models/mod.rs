use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub includes: Option<Vec<String>>,
    #[serde(default)]
    pub excludes: Option<Vec<String>>,
}