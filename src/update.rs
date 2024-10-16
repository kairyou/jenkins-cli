use crate::config::DATA_DIR;
use crate::i18n::macros::t;
use colored::*;
use semver::Version;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

const RELEASES_URL: &str = "https://ghp.ci/github.com/kairyou/jenkins-cli/releases/latest";
pub const PROJECT_URL: &str = "https://github.com/kairyou/jenkins-cli";
const CHECK_INTERVAL: u64 = 24 * 60 * 60; // 24 hours in seconds
const UPDATE_CHECK_FILE: &str = "update_check";
const TIMEOUT_DURATION: Duration = Duration::from_secs(5); // 5s for checking update

static UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(false);
static UPDATE_VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static UPDATE_NOTIFIED: AtomicBool = AtomicBool::new(false);

// debug mode: skip update check
const DEBUG_SKIP_UPDATE_CHECK: bool = true;

pub async fn check_update() {
    if let Some(version) = perform_update_check().await {
        UPDATE_AVAILABLE.store(true, Ordering::Relaxed);
        UPDATE_VERSION.set(version).ok();
    }
}

pub fn notify_if_update_available() {
    if UPDATE_NOTIFIED.load(Ordering::Relaxed) {
        return;
    }
    if UPDATE_AVAILABLE.load(Ordering::Relaxed) {
        if let Some(version) = UPDATE_VERSION.get() {
            display_update_notification(version);
            UPDATE_NOTIFIED.store(true, Ordering::Relaxed);
        }
    }
}

async fn perform_update_check() -> Option<String> {
    let update_check_path = DATA_DIR.join(UPDATE_CHECK_FILE);
    let last_check = get_last_check_time(&update_check_path);
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let time_until_next_check = {
        let base_time = CHECK_INTERVAL.saturating_sub(current_time.saturating_sub(last_check));
        if cfg!(debug_assertions) && DEBUG_SKIP_UPDATE_CHECK {
            println!("Debug: Time until next check: {} seconds", base_time);
            0
        } else {
            base_time
        }
    };

    if time_until_next_check > 0 {
        return None; // if the time is not enough, return
    }

    // Update last check time
    if let Err(e) = fs::write(&update_check_path, current_time.to_string()) {
        eprintln!("Failed to save update check time: {}", e);
    }

    match timeout(TIMEOUT_DURATION, check_latest_version()).await {
        Ok(Ok(Some(latest_version))) => Some(latest_version),
        _ => None,
    }
}

fn display_update_notification(version: &str) {
    println!(
        "✨ {} ({})",
        t!("new-version-available", "version" => version.green()),
        t!("current-version", "version" => env!("CARGO_PKG_VERSION"))
    );
    println!(
        "✨ {}",
        t!("update-instruction", 
           "command" => get_command().truecolor(215, 175, 255), 
           "url" => PROJECT_URL.truecolor(6, 175, 255))
    );
    println!();
}

fn get_last_check_time(path: &std::path::Path) -> u64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| content.trim().parse().ok())
        .unwrap_or(0)
}

async fn check_latest_version() -> Result<Option<String>, reqwest::Error> {
    let current_version = env!("CARGO_PKG_VERSION");
    let latest_version = get_latest_version().await?;

    match latest_version {
        Some(version) => match (Version::parse(&version), Version::parse(current_version)) {
            (Ok(latest), Ok(current)) => {
                if cfg!(debug_assertions) && DEBUG_SKIP_UPDATE_CHECK && latest >= current {
                    return Ok(Some(version));
                }
                if latest > current {
                    return Ok(Some(version));
                }
                Ok(None)
            }
            _ => Ok(None),
        },
        None => Ok(None),
    }
}

async fn get_latest_version() -> Result<Option<String>, reqwest::Error> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .get(RELEASES_URL)
        .header("User-Agent", "jenkins-cli")
        .send()
        .await?;

    let version = match response.status() {
        status if status.is_success() => {
            let content = response.text().await?;
            content.trim().to_string()
        }
        reqwest::StatusCode::FOUND => response
            .headers()
            .get("location")
            .and_then(|loc| loc.to_str().ok())
            .and_then(|loc| loc.rsplit('/').next())
            .map(|v| v.trim_start_matches('v').to_string())
            .unwrap_or_default(),
        _ => return Ok(None),
    };

    if !version.is_empty() && is_valid_version(&version) {
        Ok(Some(version))
    } else {
        Ok(None)
    }
}

fn is_valid_version(version: &str) -> bool {
    Version::parse(version).is_ok()
}

pub fn get_command() -> String {
    // if cfg!(target_os = "windows") {
    //     "cargo install jenkins".to_string()
    // } else {
    // }
    "bash <(curl -fsSL https://raw.githubusercontent.com/kairyou/jenkins-cli/main/scripts/install.sh)".to_string()
}
