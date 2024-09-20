use anyhow::Result;
use colored::*;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use dirs::home_dir;
use once_cell::sync::Lazy;
use std::fs;
use std::path::PathBuf;
use tokio::sync::Mutex;

use crate::env_checks::check_unsupported_terminal;
use crate::models::{FileConfig, GlobalConfig, JenkinsConfig, RuntimeConfig};
use crate::t;
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
    // if let Some(global) = &global { println!("language: {:?}", global.language); }

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

/// Load or create the Jenkins configuration file
fn load_config() -> Result<FileConfig, Box<dyn std::error::Error>> {
    let home_dir = home_dir().expect(&t!("get-home-dir-failed"));
    let config_path = home_dir.join(CONFIG_FILE);
    let _ = migrate_yaml_to_toml(&config_path);
    let content = r#"[config]
# language = "en-US"

[[jenkins]]
name = ""
url = ""
user = ""
token = ""
# includes = ["*"]
# excludes = ["*"]
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
    let config = toml::from_str(&content).expect(&t!("parse-config-file-failed"));
    Ok(config)
}

// patch for old config file
fn migrate_yaml_to_toml(config_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let yaml_path = config_path.with_extension("yaml");
    // println!("{}", yaml_path.display());
    if yaml_path.exists() && !config_path.exists() {
        let yml_content = fs::read_to_string(&yaml_path)?;
        let config: Vec<JenkinsConfig> = serde_yaml::from_str(&yml_content)?;
        // println!("{:?}", config);
        let file_config = FileConfig {
            config: None,
            jenkins: config,
        };
        let content = format!(
            "[config]\n# language = \"en-US\"\n\n{}",
            toml::to_string_pretty(&file_config)?
        );
        // println!("{}", content);
        fs::write(config_path, content)?;
        fs::rename(&yaml_path, yaml_path.with_extension("yaml.bak"))?;
    }
    Ok(())
}
