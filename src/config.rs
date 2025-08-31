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

use crate::env_checks::check_unsupported_terminal;
use crate::i18n::macros::t;
use crate::i18n::I18n;
use crate::migrations::migrate_config_yaml_to_toml;
use crate::models::{Config, GlobalConfig, JenkinsConfig};

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

pub async fn initialize_config(matches: &clap::ArgMatches) -> Result<GlobalConfig> {
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
    let cli_config = ["url", "user", "token"]
        .iter()
        .fold(JenkinsConfig::default(), |mut config, &field| {
            if let Some(value) = matches.get_one::<String>(field) {
                match field {
                    "url" => config.url = value.to_string(),
                    "user" => config.user = value.to_string(),
                    "token" => config.token = value.to_string(),
                    _ => {}
                }
            }
            config
        });

    if url_arg.is_none()
        && (jenkins_configs.is_empty()
            || jenkins_configs
                .iter()
                .any(|c| c.url.is_empty() || c.user.is_empty() || c.token.is_empty()))
    {
        eprintln!("{}", t!("fill-required-config").yellow());
        println!("{}", t!("jenkins-login-instruction"));
        std::process::exit(1);
    }

    let need_select = {
        let mut config = CONFIG.lock().await;
        config.global = Some(global_config.clone());
        config.services = jenkins_configs;

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

    Ok(global_config)
}

pub async fn select_jenkins_service() -> Result<()> {
    let mut config = CONFIG.lock().await;
    let global_enable_history = config.global.as_ref().unwrap().enable_history.unwrap_or(true);
    let services = config.services.clone();

    let selected_config = if services.len() > 1 {
        let service_names: Vec<String> = services.iter().map(|c| c.name.clone()).collect();
        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(t!("select-jenkins"))
            .items(&service_names)
            .default(0)
            .interact()
            .unwrap_or_else(|e| {
                clear_screen();
                if e.to_string().contains("interrupted") {
                    std::process::exit(0);
                } else if e.to_string().contains("IO error") {
                    check_unsupported_terminal();
                    std::process::exit(0);
                }
                eprintln!("{}: {}", t!("select-jenkins-failed"), e);
                std::process::exit(1);
            });
        services[selection].clone()
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
