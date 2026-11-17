use crossterm::{
    cursor, execute,
    terminal::{Clear, ClearType},
};
use regex::Regex;
use url::Url;

use once_cell::sync::Lazy;
use std::io::stdout;

static PATH_SLASHES_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"/{2,}|/+$").unwrap());
static PROTOCOL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^https?://").unwrap());

/// Clears the terminal screen.
pub fn clear_screen() {
    execute!(stdout(), Clear(ClearType::All)).unwrap();
    execute!(stdout(), cursor::MoveTo(0, 0)).unwrap(); // Move the cursor to the top-left corner
}

/// Formats the URL
/// - removing duplicate slashes
pub fn format_url(url: &str) -> String {
    if let Ok(mut parsed_url) = Url::parse(url) {
        let path = parsed_url.path();
        let normalized_path = PATH_SLASHES_RE.replace_all(path, "/").to_string();

        parsed_url.set_path(&normalized_path);

        parsed_url.to_string()
    } else {
        url.to_string()
    }
}

/// Simplify URL by removing protocol and trailing slashes for matching
pub fn simplify_url(url: &str) -> String {
    // println!("simplify_url: {:?}", Url::parse(url));
    PROTOCOL_RE.replace(url.trim_end_matches('/'), "").to_string()
}

/// Enable debug logging when JENKINS_DEBUG is set.
pub fn debug_enabled() -> bool {
    std::env::var("JENKINS_DEBUG").is_ok()
}

/// Print a debug line that won't indent subsequent output while a spinner is active.
pub fn debug_line(message: &str) {
    if !debug_enabled() {
        return;
    }
    crate::terminal::reset_stderr_line();
    eprintln!("{}", message);
}

/// Clear the current line on stdout and return cursor to column 0.
pub fn reset_terminal_line() {
    crate::terminal::reset_line();
}

/// Move to a clean new line for interactive prompts after streamed output.
pub fn finish_terminal_line() {
    crate::terminal::finish_line();
}

/// Ensure terminal is in a sane state before exiting.
pub fn prepare_terminal_for_exit() {
    crate::terminal::restore();
}

/// get current unix timestamp
pub fn current_timestamp() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64
}

/// Format Unix seconds in the system time zone, omitting out-of-range timestamps.
pub(crate) fn format_local_timestamp(timestamp: i64, format: &str) -> Option<String> {
    format_timestamp(timestamp, format, jiff::tz::TimeZone::system())
}

fn format_timestamp(timestamp: i64, format: &str, time_zone: jiff::tz::TimeZone) -> Option<String> {
    let datetime = jiff::Timestamp::from_second(timestamp)
        .ok()?
        .to_zoned_datetime(time_zone);
    Some(datetime.strftime(format).to_string())
}

/// delay `ms` milliseconds
/// # Examples
/// ```rust
/// use jenkins::utils::delay;
/// async fn test_delay() {
///     delay(5 * 1000).await; // = tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
/// }
/// ```
pub async fn delay(ms: u64) {
    // tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

// /// compare two version numbers, return a boolean value, support specified comparison operators
// /// # Examples
// /// ```rust
// /// use jenkins::utils::version_compare;
// ///
// /// assert!(version_compare("2.5.1", "3.0.0", "<"));
// /// assert!(version_compare("3.6.1", "3.0.0", ">"));
// /// assert!(version_compare("3.6.1", "3.6.1", "=="));
// /// assert!(version_compare("3.6", "3.6.4", "<"));
// /// assert!(version_compare("3.6.4", "3.6.4", "=="));
// /// assert!(version_compare("3.6.4", "3.6.4", ">="));
// /// assert!(version_compare("3.6.4", "3.6.1", ">="));
// /// assert!(version_compare("3.6.1", "3.6.4", "<="));
// /// ```
// #[allow(dead_code)]
// pub fn version_compare(current_version: &str, target_version: &str, op: &str) -> bool {
//     use std::cmp::Ordering;
//     let current: Vec<u32> = current_version.split('.').filter_map(|s| s.parse().ok()).collect();
//     let target: Vec<u32> = target_version.split('.').filter_map(|s| s.parse().ok()).collect();

//     let ordering = current
//         .iter()
//         .zip(target.iter())
//         .find_map(|(c, t)| match c.cmp(t) {
//             Ordering::Equal => None,
//             non_eq => Some(non_eq),
//         })
//         .unwrap_or_else(|| current.len().cmp(&target.len())); // if length is different, the shorter version is considered smaller

//     match op {
//         "==" => ordering == Ordering::Equal,
//         "!=" => ordering != Ordering::Equal,
//         ">" => ordering == Ordering::Greater,
//         ">=" => ordering == Ordering::Greater || ordering == Ordering::Equal,
//         "<" => ordering == Ordering::Less,
//         "<=" => ordering == Ordering::Less || ordering == Ordering::Equal,
//         _ => false, // handle unsupported comparison operators
//     }
// }

/// flush stdin buffer (prevent `stdin` from reading previous inputs, such as pressing `Enter` key)
pub fn flush_stdin() {
    // @zh 清空输入缓冲区 (防止 `stdin` 读取到之前的输入, 例如按下 `Enter` 键)
    if let Err(e) = flush_stdin_impl() {
        eprintln!("Failed to flush stdin: {}", e);
    }
}
fn flush_stdin_impl() -> std::io::Result<()> {
    use std::io;
    // set non-blocking mode
    #[cfg(unix)]
    {
        use libc::{fcntl, F_GETFL, F_SETFL, O_NONBLOCK};
        use std::{io::Read, os::unix::io::AsRawFd};

        let stdin = io::stdin();
        let mut handle = stdin.lock();

        let fd = handle.as_raw_fd();
        let flags = unsafe { fcntl(fd, F_GETFL) };
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }

        if unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) } == -1 {
            return Err(io::Error::last_os_error());
        }

        let mut buffer = [0; 1024];
        while handle.read(&mut buffer).is_ok() {}

        // restore original file descriptor flags
        if unsafe { fcntl(fd, F_SETFL, flags) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }

    // Windows, use `FlushConsoleInputBuffer` to flush the input buffer
    #[cfg(windows)]
    {
        use winapi::um::{
            handleapi::INVALID_HANDLE_VALUE, processenv::GetStdHandle, wincon::FlushConsoleInputBuffer, winnt::HANDLE,
        };

        unsafe {
            let handle: HANDLE = GetStdHandle(winapi::um::winbase::STD_INPUT_HANDLE);
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }

            if FlushConsoleInputBuffer(handle) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }

    Ok(())
}

/// get git repository (remote) branch list
pub fn get_git_branches() -> Vec<String> {
    use std::process::Command;

    let output = Command::new("git").arg("branch").arg("-r").output();

    if let Ok(output) = output {
        let branches = String::from_utf8_lossy(&output.stdout);
        // println!("{}", branches);
        let excludes = [
            // "/feature/", // feature branch
            "/HEAD", // exclude the default branch (origin/HEAD)
        ];
        branches
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                // only need the branches of the remote repository (origin)
                trimmed.starts_with("origin/") && !trimmed.is_empty() && !excludes.iter().any(|&s| trimmed.contains(s))
            })
            .map(|line| line.trim().replace("origin/", ""))
            .collect()
    } else {
        vec![]
    }
}

/// get current git branch
pub fn get_current_branch() -> String {
    use std::process::Command;
    let output = Command::new("git")
        .arg("symbolic-ref")
        .arg("--short")
        .arg("HEAD")
        .output();

    if let Ok(output) = output {
        let branch = String::from_utf8_lossy(&output.stdout);
        branch.trim().to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod timestamp_tests {
    use super::format_timestamp;
    use jiff::tz::{Offset, TimeZone};

    #[test]
    fn preserves_history_and_preset_formats() {
        let zone = TimeZone::fixed(Offset::from_hours(8).unwrap());
        assert_eq!(
            format_timestamp(1_704_124_801, "%Y-%m-%d %H:%M:%S", zone.clone()).as_deref(),
            Some("2024-01-02 00:00:01")
        );
        assert_eq!(
            format_timestamp(1_704_124_801, "%Y-%m-%d %H:%M", zone).as_deref(),
            Some("2024-01-02 00:00")
        );
    }

    #[test]
    fn handles_epoch_and_negative_timestamps() {
        assert_eq!(
            format_timestamp(0, "%Y-%m-%d %H:%M:%S", TimeZone::UTC).as_deref(),
            Some("1970-01-01 00:00:00")
        );
        assert_eq!(
            format_timestamp(-1, "%Y-%m-%d %H:%M:%S", TimeZone::UTC).as_deref(),
            Some("1969-12-31 23:59:59")
        );
    }

    #[test]
    fn follows_daylight_saving_transitions() {
        let zone = TimeZone::get("America/New_York").unwrap();
        for (instant, expected) in [
            ("2024-03-10T06:59:59Z", "2024-03-10 01:59:59"),
            ("2024-03-10T07:00:00Z", "2024-03-10 03:00:00"),
            ("2024-11-03T05:59:59Z", "2024-11-03 01:59:59"),
            ("2024-11-03T06:00:00Z", "2024-11-03 01:00:00"),
        ] {
            let timestamp = instant.parse::<jiff::Timestamp>().unwrap().as_second();
            assert_eq!(
                format_timestamp(timestamp, "%Y-%m-%d %H:%M:%S", zone.clone()).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn rejects_out_of_range_timestamps() {
        for timestamp in [i64::MIN, i64::MAX] {
            assert_eq!(format_timestamp(timestamp, "%Y-%m-%d %H:%M:%S", TimeZone::UTC), None);
        }
    }
}
