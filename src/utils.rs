use crossterm::{
  cursor::{self, MoveToColumn},
  execute,
  terminal::{Clear, ClearType},
};
use regex::Regex;
use url::Url;

use anyhow::{anyhow, Result};
use std::{
  io::stdout,
  sync::atomic::{AtomicBool, Ordering},
  sync::Arc,
};

/// Clears the terminal screen.
pub fn clear_screen() {
  execute!(stdout(), Clear(ClearType::All)).unwrap();
  execute!(stdout(), cursor::MoveTo(0, 0)).unwrap(); // Move the cursor to the top-left corner
}

/// Clears the current line.
/// like print!("\r\x1b[K")
pub fn clear_line() {
  execute!(stdout(), Clear(ClearType::CurrentLine)).unwrap();
  execute!(stdout(), MoveToColumn(0)).unwrap(); // Move the cursor to the start of the line
}

/// Moves the cursor up by one line and clears that line
/// @zh 移动光标到上一行并清除该行的内容
pub fn clear_previous_line() {
  execute!(stdout(), cursor::MoveUp(1)).unwrap();
  clear_line();
}

/// Formats the URL
/// - removing duplicate slashes
pub fn format_url(url: &str) -> String {
  if let Ok(mut parsed_url) = Url::parse(url) {
      let path = parsed_url.path();
      let re = Regex::new(r"/{2,}").unwrap();
      let normalized_path = re.replace_all(path, "/").to_string();

      parsed_url.set_path(&normalized_path);

      parsed_url.to_string()
  } else {
      url.to_string()
  }
}

/// get current unix timestamp
pub fn current_timestamp() -> i64 {
  use std::time::{SystemTime, UNIX_EPOCH};
  SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .expect("Time went backwards")
      .as_secs() as i64
}

/// check if ctrl+c is pressed
#[allow(dead_code)]
pub fn check_ctrl_c(ctrl_c_pressed: &Arc<AtomicBool>) -> Result<(), anyhow::Error> {
  if ctrl_c_pressed.load(Ordering::SeqCst) {
      Err(anyhow!("Ctrl+C pressed"))
  } else {
      Ok(())
  }
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

/// compare two version numbers, return a boolean value, support specified comparison operators
pub fn version_compare(current_version: &str, target_version: &str, op: &str) -> bool {
  use std::cmp::Ordering;
  let current: Vec<u32> = current_version
      .split('.')
      .filter_map(|s| s.parse().ok())
      .collect();
  let target: Vec<u32> = target_version
      .split('.')
      .filter_map(|s| s.parse().ok())
      .collect();

  let ordering = current
      .iter()
      .zip(target.iter())
      .find_map(|(c, t)| match c.cmp(t) {
          Ordering::Equal => None,
          non_eq => Some(non_eq),
      })
      .unwrap_or_else(|| current.len().cmp(&target.len())); // if length is different, the shorter version is considered smaller

  match op {
      "==" => ordering == Ordering::Equal,
      "!=" => ordering != Ordering::Equal,
      ">" => ordering == Ordering::Greater,
      ">=" => ordering == Ordering::Greater || ordering == Ordering::Equal,
      "<" => ordering == Ordering::Less,
      "<=" => ordering == Ordering::Less || ordering == Ordering::Equal,
      _ => false, // handle unsupported comparison operators
  }
}

/// flush stdin buffer (prevent `stdin` from reading previous inputs, such as pressing `Enter` key)
/// @zh 清空输入缓冲区 (防止 `stdin` 读取到之前的输入, 例如按下 `Enter` 键)
pub fn flush_stdin() {
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
          handleapi::INVALID_HANDLE_VALUE, processenv::GetStdHandle,
          wincon::FlushConsoleInputBuffer, winnt::HANDLE,
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
              trimmed.starts_with("origin/")
                  && !trimmed.is_empty()
                  && !excludes.iter().any(|&s| trimmed.contains(s))
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
