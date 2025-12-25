use anyhow::Result;
use colored::*;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use dirs::home_dir;
use once_cell::sync::Lazy;
use serde_json::json;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::PathBuf;
use tokio::sync::Mutex;
use toml_edit::{value, DocumentMut};

use crate::i18n::macros::t;
use crate::i18n::I18n;
use crate::migrations::migrate_config_yaml_to_toml;
use crate::models::{Config, GlobalConfig, JenkinsConfig};
use crate::prompt;

use crate::utils;
use crate::utils::clear_screen;

pub const CONFIG_FILE: &str = ".jenkins.toml";
pub const DATA_DIR_NAME: &str = ".jenkins-cli";

pub static CONFIG: Lazy<Mutex<Config>> = Lazy::new(|| {
    Mutex::new(Config {
        global: Some(GlobalConfig::default()),
        services: Vec::new(),
        jenkins: None,
    })
});

pub static DATA_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let home_dir = home_dir().expect(&t!("get-home-dir-failed"));
    let data_dir = home_dir.join(DATA_DIR_NAME);

    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).expect(&t!("create-data-dir-failed"));
    }

    data_dir
});

pub async fn initialize_config(matches: &clap::ArgMatches) -> Result<(GlobalConfig, bool)> {
    let _ = DATA_DIR.as_path(); // auto create data dir

    let file_config = load_config().expect(&t!("load-config-failed"));
    let global_config = file_config["config"]
        .as_object()
        .map(|obj| serde_json::from_value(JsonValue::Object(obj.clone())).unwrap_or_default())
        .unwrap_or_default();
    let jenkins_configs: Vec<JenkinsConfig> =
        serde_json::from_value(file_config["jenkins"].clone()).unwrap_or_default();

    apply_global_settings(&global_config);

    // println!("arg len: {}", std::env::args().len());
    let url_arg = matches.get_one::<String>("url");
    let cli_config = ["url", "user", "token", "cookie"]
        .iter()
        .fold(JenkinsConfig::default(), |mut config, &field| {
            if let Some(value) = matches.get_one::<String>(field) {
                match field {
                    "url" => config.url = value.to_string(),
                    "user" => config.user = value.to_string(),
                    "token" => config.token = value.to_string(),
                    "cookie" => config.cookie = value.to_string(),
                    _ => {}
                }
            }
            config
        });

    let has_valid_auth = |c: &JenkinsConfig| {
        let has_basic = !c.user.is_empty() && !c.token.is_empty();
        let has_cookie = !c.cookie.is_empty();
        has_cookie || has_basic
    };

    if url_arg.is_none()
        && (jenkins_configs.is_empty() || jenkins_configs.iter().any(|c| c.url.is_empty() || !has_valid_auth(c)))
    {
        eprintln!("{}", t!("fill-required-config").yellow());
        println!("{}", t!("jenkins-login-instruction"));
        std::process::exit(1);
    }

    let need_select = {
        let mut config = CONFIG.lock().await;
        config.global = Some(global_config.clone());
        config.services = jenkins_configs.clone();

        match url_arg {
            Some(url) => {
                config.jenkins = Some(if config.services.is_empty() {
                    cli_config.clone()
                } else {
                    let input_url = utils::simplify_url(url);
                    let matched_config = config
                        .services
                        .iter()
                        .find(|s| input_url == utils::simplify_url(&s.url))
                        .cloned();
                    match matched_config {
                        Some(matched) => JenkinsConfig {
                            user: if cli_config.user.is_empty() {
                                matched.user
                            } else {
                                cli_config.user
                            },
                            token: if cli_config.token.is_empty() {
                                matched.token
                            } else {
                                cli_config.token
                            },
                            cookie: if cli_config.cookie.is_empty() {
                                matched.cookie
                            } else {
                                cli_config.cookie
                            },
                            ..matched
                        },
                        None => cli_config,
                    }
                });
                false
            }
            None => !config.services.is_empty(),
        }
    };

    if need_select {
        select_jenkins_service().await?;
    }

    let service_step_enabled = url_arg.is_none() && !jenkins_configs.is_empty() && jenkins_configs.len() > 1;
    Ok((global_config, service_step_enabled))
}

pub async fn select_jenkins_service() -> Result<()> {
    let mut config = CONFIG.lock().await;
    let global_enable_history = config.global.as_ref().unwrap().enable_history.unwrap_or(true);
    let services = config.services.clone();

    let selected_config = if services.len() > 1 {
        let service_names: Vec<String> = services.iter().map(|c| c.name.clone()).collect();
        let selection = prompt::handle_selection(prompt::with_prompt(|| {
            FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt(t!("select-jenkins"))
                .items(&service_names)
                .default(0)
                .vim_mode(true) // Esc, j|k
                .interact()
        }));

        match selection {
            Some(idx) => services[idx].clone(),
            None => {
                // Ctrl+C pressed at service selection - exit program
                clear_screen();
                println!("{}", t!("bye"));
                std::process::exit(0);
            }
        }
    } else {
        services[0].clone()
    };

    let enable_history = selected_config.enable_history.unwrap_or(global_enable_history);
    config.jenkins = Some(JenkinsConfig {
        enable_history: Some(enable_history),
        ..selected_config
    });

    Ok(())
}

/// Persist cookie for matched Jenkins service in config file.
/// Returns true if the cookie is already current or was written.
pub fn persist_cookie_for_url(url: &str, cookie: &str) -> Result<bool> {
    let home_dir = home_dir().expect(&t!("get-home-dir-failed"));
    let config_path = home_dir.join(CONFIG_FILE);
    if !config_path.exists() {
        return Ok(false);
    }
    if crate::utils::debug_enabled() {
        crate::utils::debug_line(&format!(
            "[debug] persist_cookie_for_url: path={}, url={}",
            config_path.display(),
            url
        ));
    }

    let content = fs::read_to_string(&config_path).expect(&t!("read-config-file-failed"));
    let mut doc = match content.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return Ok(false),
    };

    let target_url = utils::simplify_url(url);
    let mut updated = false;
    if let Some(jenkins) = doc["jenkins"].as_array_of_tables_mut() {
        for table in jenkins.iter_mut() {
            let table_url = table.get("url").and_then(|v| v.as_str()).map(utils::simplify_url);
            if table_url.as_deref() == Some(&target_url) {
                if crate::utils::debug_enabled() {
                    crate::utils::debug_line(&format!("[debug] persist_cookie_for_url: matched {}", target_url));
                }
                let existing = table.get("cookie").and_then(|v| v.as_str()).unwrap_or("");
                if existing == cookie {
                    return Ok(true);
                }
                table["cookie"] = value(cookie);
                updated = true;
                break;
            }
        }
    }

    if updated {
        fs::write(&config_path, doc.to_string()).expect(&t!("write-default-config-failed"));
        if crate::utils::debug_enabled() {
            crate::utils::debug_line(&format!(
                "[debug] persist_cookie_for_url: wrote cookie for {}",
                target_url
            ));
        }
        return Ok(true);
    }

    Ok(false)
}

/// Apply global settings from the global configuration
fn apply_global_settings(global_config: &GlobalConfig) {
    // println!("global_settings: {:?}", global_config);
    if let Some(locale) = &global_config.locale {
        I18n::set_locale(locale);
    }
    // if let Some(log_level) = &global_config.log_level {
    //     set_log_level(log_level);
    // }
}

/// Load or create the Jenkins configuration file
fn load_config() -> Result<JsonValue, Box<dyn std::error::Error>> {
    let home_dir = home_dir().expect(&t!("get-home-dir-failed"));
    let config_path = home_dir.join(CONFIG_FILE);
    let _ = migrate_config_yaml_to_toml(&config_path);
    let content = r#"[config]
# locale = "en-US"
# enable_history = true
# check_update = true
# timeout = 30

[[jenkins]]
name = ""
url = ""
user = ""
token = ""
# includes = ["*"]
# excludes = []
"#;

    // Create default configuration file
    if !config_path.exists() {
        fs::write(&config_path, content).expect(&t!("write-default-config-failed"));
    }

    println!("{}: '{}'", t!("config-file"), config_path.display());
    let content = fs::read_to_string(&config_path).expect(&t!("read-config-file-failed"));
    match toml::from_str::<JsonValue>(content.trim()) {
        Ok(config) => Ok(config),
        Err(_e) => {
            // println!("Failed to parse config file: {}", _e);
            // Err(anyhow::anyhow!(t!("parse-config-file-failed")).into())
            Ok(json!({
                "config": {},
                "jenkins": []
            }))
        }
    }
}
