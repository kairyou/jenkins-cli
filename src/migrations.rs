use crate::config::DATA_DIR;
use crate::constants::ParamType;
use crate::i18n::macros::t;
use crate::jenkins::history::HISTORY_FILE;
use crate::models::{FileConfig, JenkinsConfig};
use anyhow::{Context, Result};
use dirs::home_dir;
use serde_json::json;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

pub const CURRENT_HISTORY_VERSION: u32 = 1; // latest version

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
            "[config]\n# locale = \"en-US\"\n\n{}",
            toml::to_string_pretty(&file_config)?
        );
        fs::write(config_path, content)?;
        fs::rename(&yaml_path, yaml_path.with_extension("yaml.bak"))?;
    }
    Ok(())
}

/// Migrate history from yaml to toml (v0)
pub fn migrate_history_yaml_to_toml(yaml_path: &PathBuf, toml_path: &PathBuf) -> Result<()> {
    if yaml_path.exists() {
        let yaml_content = fs::read_to_string(&yaml_path)?;
        // HistoryItem: job_url/name/display_name/user_params(key=value)/created_at/completed_at
        let yaml_entries: Vec<YamlValue> = serde_yaml::from_str(&yaml_content)?;

        let entries: Vec<JsonValue> = yaml_entries
            .into_iter()
            .map(|entry| {
                let mut json_entry = serde_json::Map::new();

                if let Some(job_url) = entry["job_url"].as_str() {
                    json_entry.insert("job_url".to_string(), JsonValue::String(job_url.to_string()));
                }
                if let Some(name) = entry["name"].as_str() {
                    json_entry.insert("name".to_string(), JsonValue::String(name.to_string()));
                }
                if let Some(display_name) = entry["display_name"].as_str() {
                    json_entry.insert("display_name".to_string(), JsonValue::String(display_name.to_string()));
                }
                if let Some(created_at) = entry["created_at"].as_i64() {
                    json_entry.insert("created_at".to_string(), JsonValue::Number(created_at.into()));
                }
                if let Some(completed_at) = entry["completed_at"].as_i64() {
                    json_entry.insert("completed_at".to_string(), JsonValue::Number(completed_at.into()));
                }
                // println!("entry: {:?}", entry);
                if let Some(user_params) = entry["user_params"].as_mapping() {
                    let params: HashMap<String, String> = user_params
                        .iter()
                        .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_str()?.to_string())))
                        .collect();
                    json_entry.insert(
                        "params".to_string(),
                        JsonValue::Object(params.into_iter().map(|(k, v)| (k, JsonValue::String(v))).collect()),
                    );
                }
                JsonValue::Object(json_entry)
            })
            .collect();

        let file_history = json!({
            "entries": entries,
            "version": 0
        });
        let content = toml::to_string(&file_history)?;
        fs::write(toml_path, content)?;
        fs::remove_file(yaml_path)?;
    }
    Ok(())
}

/// Migrate old location of history file
pub fn migrate_history_location(history_path: &PathBuf) -> Result<()> {
    let old_history_paths = vec![home_dir().unwrap().join(".jenkins_history.toml")];
    for path in old_history_paths {
        println!("migrate_history_location: {:?}", path);
        if path.exists() {
            fs::rename(path, history_path)?;
        }
    }
    Ok(())
}

/// Migrate history to the latest version
pub fn migrate_history() -> Result<()> {
    let history_path = DATA_DIR.join(HISTORY_FILE);
    let home_dir = home_dir().expect(&t!("get-home-dir-failed"));
    // v0: YAML => TOML
    let yaml_path = home_dir.join(".jenkins_history.yaml");
    migrate_history_yaml_to_toml(&yaml_path, &history_path)?;
    migrate_history_location(&history_path)?;

    if !history_path.exists() {
        return Ok(());
    }

    let file = std::fs::File::open(&history_path).context("Failed to open history file")?;
    let mut reader = std::io::BufReader::new(file);
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .context("Failed to read file content")?;

    if content.trim().is_empty() {
        return Ok(());
    }

    // TOML => JSON
    let mut json_value: JsonValue = toml::from_str(&content).context("Failed to parse TOML")?;
    let version = json_value.get("version").and_then(JsonValue::as_u64).unwrap_or(0);

    if version < CURRENT_HISTORY_VERSION as u64 {
        for v in version..CURRENT_HISTORY_VERSION as u64 {
            match v {
                0 => migrate_to_v1(&mut json_value)?,
                // 1 => migrate_to_v2(&mut json_value)?,
                _ => break,
            }
        }

        let migrated_toml = toml::to_string(&json_value).context("Failed to convert JSON to TOML")?;
        fs::write(&history_path, migrated_toml).context("Failed to write migrated history")?;
    }

    Ok(())
}

// 1. add ParamInfo(type/value)
// 2. rename user_params to params
fn migrate_to_v1(json: &mut JsonValue) -> Result<()> {
    json["version"] = json!(1);
    if let Some(entries) = json.get_mut("entries").and_then(JsonValue::as_array_mut) {
        for entry in entries {
            let params_key = if entry.get("params").is_some() {
                "params"
            } else if entry.get("user_params").is_some() {
                "user_params"
            } else {
                continue; // if no params or user_params, skip this entry
            };

            if let Some(params) = entry.get_mut(params_key) {
                let new_params = match params.take() {
                    JsonValue::Object(map) => {
                        let mut new_map = serde_json::Map::new();
                        for (key, value) in map {
                            let sensitive_keywords = ["password", "token"];
                            let param_type = if sensitive_keywords
                                .iter()
                                .any(|keyword| key.to_lowercase().contains(keyword))
                            {
                                ParamType::Password
                            } else {
                                ParamType::String
                            };
                            let param_info = json!({
                                "value": value.as_str().unwrap_or_default().to_string(),
                                "type": param_type,
                            });
                            new_map.insert(key, param_info);
                        }
                        JsonValue::Object(new_map)
                    }
                    _ => JsonValue::Object(serde_json::Map::new()),
                };
                entry["params"] = new_params;
            }

            // delete old user_params
            entry.as_object_mut().unwrap().remove("user_params");
        }
    }
    Ok(())
}
