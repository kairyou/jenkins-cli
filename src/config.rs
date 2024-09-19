use anyhow::Result;
use colored::*;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use dirs::home_dir;
use once_cell::sync::Lazy;
use std::fs;
use tokio::sync::Mutex;

use crate::env_checks::check_unsupported_terminal;
use crate::models::JenkinsConfig;
use crate::t;
use crate::utils::clear_screen;

pub static CONFIG: Lazy<Mutex<JenkinsConfig>> = Lazy::new(|| {
    Mutex::new(JenkinsConfig::default()) // Initialize with default value
});

pub async fn initialize_config() -> Result<()> {
    let configs = load_config().expect(&t!("load-config-failed"));

    if configs.is_empty()
        || configs
            .iter()
            .any(|c| c.url.is_empty() || c.user.is_empty() || c.token.is_empty())
    {
        eprintln!("{}", t!("fill-required-config").yellow());
        println!("{}", t!("jenkins-login-instruction"));
        std::process::exit(1);
    }

    let selected_config = if configs.len() > 1 {
        let env_names: Vec<String> = configs.iter().map(|c| c.name.clone()).collect();
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
        configs[selection].clone()
    } else {
        configs[0].clone()
    };

    let mut config = CONFIG.lock().await;
    *config = selected_config;

    Ok(())
}

/// Load or create the Jenkins configuration file
fn load_config() -> Result<Vec<JenkinsConfig>, Box<dyn std::error::Error>> {
    let config_path = {
        let mut default_path = home_dir().expect(&t!("get-home-dir-failed"));
        default_path.push(".jenkins.yaml");
        default_path
    };
    let config_content = r#"- name: ''
  url: ''
  user: ''
  token: ''
  # includes: []
  # excludes: []
"#;
    // Create default configuration file
    if !config_path.exists() {
        // let default_config = vec![JenkinsConfig {
        //     name: "".to_string(),
        //     url: "".to_string(),
        //     user: "".to_string(),
        //     token: "".to_string(),
        //     includes: Some(vec![".*".to_string(), ".*-deploy".to_string()]),
        //     excludes: Some(vec![".*-deprecated".to_string()]),
        // }];
        // let config_content = serde_yaml::to_string(&default_config).expect("Failed to serialize default configuration");
        fs::write(&config_path, config_content).expect(&t!("write-default-config-failed"));
    }

    println!("{}: '{}'", t!("config-file"), config_path.display());
    let config_content = fs::read_to_string(&config_path).expect(&t!("read-config-file-failed"));
    let config: Vec<JenkinsConfig> = serde_yaml::from_str(&config_content).expect(&t!("parse-config-file-failed"));
    Ok(config)
}
