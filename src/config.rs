use anyhow::Result;
use colored::*;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use dirs::home_dir;
use once_cell::sync::Lazy;
use std::fs;
use tokio::sync::Mutex;

use crate::env_checks::check_unsupported_terminal;
use crate::i18n::macros::t;
use crate::i18n::I18n;
use crate::migrations::migrate_config_yaml_to_toml;
use crate::models::{FileConfig, GlobalConfig, JenkinsConfig, RuntimeConfig};

use crate::utils::clear_screen;

pub const CONFIG_FILE: &str = ".jenkins.toml";

pub static CONFIG: Lazy<Mutex<RuntimeConfig>> = Lazy::new(|| {
    Mutex::new(RuntimeConfig {
        global: Some(GlobalConfig::default()),
        jenkins: JenkinsConfig::default(),
    })
});

// let (global_config, jenkins_config) = CONFIG.lock().await;
pub async fn initialize_config() -> Result<()> {
    let cfg = load_config().expect(&t!("load-config-failed"));
    let global = cfg.config;
    let jenkins = cfg.jenkins;

    if let Some(global_config) = &global {
        // println!("language: {:?}", global.locale);
        apply_global_settings(global_config);
    }

    if jenkins.is_empty()
        || jenkins
            .iter()
            .any(|c| c.url.is_empty() || c.user.is_empty() || c.token.is_empty())
    {
        eprintln!("{}", t!("fill-required-config").yellow());
        println!("{}", t!("jenkins-login-instruction"));
        std::process::exit(1);
    }

    let selected_config = if jenkins.len() > 1 {
        let env_names: Vec<String> = jenkins.iter().map(|c| c.name.clone()).collect();
        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(t!("select-jenkins-env"))
            .items(&env_names)
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
                eprintln!("{}: {}", t!("select-jenkins-env-failed"), e);
                std::process::exit(1);
            });
        jenkins[selection].clone()
    } else {
        jenkins[0].clone()
    };
    let mut config = CONFIG.lock().await;
    *config = RuntimeConfig {
        global: global.clone(),
        jenkins: selected_config,
    };

    Ok(())
}

/// Apply global settings from the global configuration
fn apply_global_settings(global_config: &GlobalConfig) {
    // println!("global_settings: {:?}", global_config);
    // 应用语言设置
    if let Some(locale) = &global_config.locale {
        I18n::set_locale(locale);
    }
    // if let Some(log_level) = &global_config.log_level {
    //     set_log_level(log_level);
    // }
}

/// Load or create the Jenkins configuration file
fn load_config() -> Result<FileConfig, Box<dyn std::error::Error>> {
    let home_dir = home_dir().expect(&t!("get-home-dir-failed"));
    let config_path = home_dir.join(CONFIG_FILE);
    let _ = migrate_config_yaml_to_toml(&config_path);
    let content = r#"[config]
# locale = "en-US"

[[jenkins]]
name = ""
url = ""
user = ""
token = ""
# includes = ["*"]
# excludes = []
"#;

    // #[rustfmt::skip]
    // println!("{}", toml::to_string_pretty(&Config {
    //   config: None,
    //   jenkins: vec![JenkinsConfig { name: "".to_string(), url: "".to_string(), user: "".to_string(), token: "".to_string(), includes: vec![], excludes: vec![], }] }) .unwrap()
    // );
    // Create default configuration file
    if !config_path.exists() {
        fs::write(&config_path, content).expect(&t!("write-default-config-failed"));
    }

    println!("{}: '{}'", t!("config-file"), config_path.display());
    let content = fs::read_to_string(&config_path).expect(&t!("read-config-file-failed"));
    // let config = toml::from_str(&content).expect(&t!("parse-config-file-failed"));
    match toml::from_str::<FileConfig>(content.trim()) {
        Ok(config) => Ok(config),
        Err(_e) => {
            // println!("Failed to parse config file: {}", _e);
            // Err(anyhow::anyhow!(t!("parse-config-file-failed")).into())
            Ok(FileConfig::default())
        }
    }
}
