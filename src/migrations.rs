use crate::jenkins::history::HistoryEntry;
use crate::models::{FileConfig, JenkinsConfig};
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

/// Migrate config from yaml to toml
pub fn migrate_config_yaml_to_toml(config_path: &PathBuf) -> Result<()> {
    let yaml_path = config_path.with_extension("yaml");
    if yaml_path.exists() && !config_path.exists() {
        let yml_content = fs::read_to_string(&yaml_path)?;
        let config: Vec<JenkinsConfig> = serde_yaml::from_str(&yml_content)?;
        let file_config = FileConfig {
            config: None,
            jenkins: config,
        };
        let content = format!(
            "[config]\n# language = \"en-US\"\n\n{}",
            toml::to_string_pretty(&file_config)?
        );
        fs::write(config_path, content)?;
        fs::rename(&yaml_path, yaml_path.with_extension("yaml.bak"))?;
    }
    Ok(())
}

/// Migrate history from yaml to toml
pub fn migrate_history_yaml_to_toml(config_path: &PathBuf) -> Result<()> {
    let yaml_path = config_path.with_extension("yaml");
    if yaml_path.exists() {
        let yaml_content = fs::read_to_string(&yaml_path)?;
        let yaml_entries: Vec<HistoryEntry> = serde_yaml::from_str(&yaml_content)?;
        let file_history = crate::jenkins::history::FileHistory { entries: yaml_entries };
        let content = toml::to_string(&file_history)?;
        fs::write(config_path, content)?;
        fs::remove_file(yaml_path)?;
    }
    Ok(())
}
