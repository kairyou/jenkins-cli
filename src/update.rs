use crate::config::DATA_DIR;
use crate::i18n::macros::t;
use colored::*;
use semver::Version;
use std::env;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RELEASES_URL: &str = "https://ghfast.top/github.com/kairyou/jenkins-cli/releases/latest";
pub const PROJECT_URL: &str = "https://github.com/kairyou/jenkins-cli";
const CHECK_INTERVAL: u64 = 24 * 60 * 60; // 24 hours in seconds
const UPDATE_CHECK_FILE: &str = "update_check";
const VERSION_CACHE_FILE: &str = "latest_version";
const TIMEOUT_DURATION: Duration = Duration::from_secs(5); // 5s for checking update
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

static UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(false);
static UPDATE_VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static UPDATE_NOTIFIED: AtomicBool = AtomicBool::new(false);

fn is_debug_update() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    let value = env::var("FORCE_UPDATE_CHECK").unwrap_or_default().to_lowercase();
    value == "1"
}

pub async fn check_update() {
    let update_check_path = DATA_DIR.join(UPDATE_CHECK_FILE);
    let last_check = get_last_check_time(&update_check_path);
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let time_until_next_check = {
        let base_time = CHECK_INTERVAL.saturating_sub(current_time.saturating_sub(last_check));
        if is_debug_update() {
            0
        } else {
            base_time
        }
    };

    if time_until_next_check > 0 {
        return; // if the time is not enough, return
    }

    // Update last check time
    if let Err(e) = fs::write(&update_check_path, current_time.to_string()) {
        eprintln!("Failed to save update check time: {}", e);
    }

    if let Ok(Some(version)) = get_latest_version().await {
        if is_debug_update() {
            println!("online_check version: {}", version);
        }
        save_version_cache(&version);
        if compare_versions(&version, CURRENT_VERSION).is_some() {
            mark_update_available(&version);
        }
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

fn display_update_notification(version: &str) {
    println!();
    println!(
        "✨ {} ({})",
        t!("new-version-available", "version" => version.green()),
        t!("current-version", "version" => CURRENT_VERSION)
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

fn compare_versions(latest: &str, current: &str) -> Option<String> {
    match (Version::parse(latest), Version::parse(current)) {
        (Ok(latest), Ok(current)) => {
            if is_debug_update() && latest >= current {
                return Some(latest.to_string());
            }
            if latest > current {
                return Some(latest.to_string());
            }
            None
        }
        _ => None,
    }
}

fn save_version_cache(version: &str) {
    if let Err(e) = fs::write(DATA_DIR.join(VERSION_CACHE_FILE), version) {
        eprintln!("Failed to save latest version cache: {}", e);
    }
}

fn load_version_cache() -> Option<String> {
    fs::read_to_string(DATA_DIR.join(VERSION_CACHE_FILE))
        .ok()
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
}

async fn get_latest_version() -> Result<Option<String>, reqwest::Error> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT_DURATION)
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

/// Pre-check the cached version so we can notify before network I/O.
pub fn precheck_update_status() {
    if let Some(cached_version) = load_version_cache() {
        if is_debug_update() {
            println!("cache_check version: {}", cached_version);
        }
        if let Some(version) = compare_versions(&cached_version, CURRENT_VERSION) {
            mark_update_available(&version);
        }
    }
}

/// Store the detected version for later notification
fn mark_update_available(version: &str) {
    UPDATE_AVAILABLE.store(true, Ordering::Relaxed);
    let _ = UPDATE_VERSION.set(version.to_string());
}

pub fn get_command() -> String {
    // if cfg!(target_os = "windows") {
    //     "cargo install jenkins".to_string()
    // } else {
    // }
    "bash <(curl -fsSL https://raw.githubusercontent.com/kairyou/jenkins-cli/main/scripts/install.sh)".to_string()
}
