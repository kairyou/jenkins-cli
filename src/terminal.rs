use std::fmt::Display;
use std::io::{self, Write};

use console::measure_text_width;

/// Clear the current stdout/stderr line and return the cursor to column 0.
pub fn reset_line() {
    let mut stdout = io::stdout();
    let _ = write!(stdout, "\r\x1b[2K");
    let _ = stdout.flush();

    reset_stderr_line();
}

/// Clear the current stderr line and return the cursor to column 0.
pub fn reset_stderr_line() {
    let mut stderr = io::stderr();
    let _ = write!(stderr, "\r\x1b[2K");
    let _ = stderr.flush();
}

/// Move stdout to a clean new line without erasing streamed output.
pub fn finish_line() {
    let mut stdout = io::stdout();
    let _ = write!(stdout, "\r\n");
    let _ = stdout.flush();

    let mut stderr = io::stderr();
    let _ = write!(stderr, "\r\x1b[2K");
    let _ = stderr.flush();
}

/// Print streamed log output and flush immediately.
pub fn print_stream(text: &str) {
    print!("{}", text);
    let _ = io::stdout().flush();
}

/// Print a complete CLI-owned line using CRLF so raw-mode terminals return to column 0.
pub fn print_line(message: impl Display) {
    let mut stdout = io::stdout();
    let _ = write!(stdout, "{}\r\n", message);
    let _ = stdout.flush();
}

/// Build a full-width section separator, similar to common CLI progress dividers.
pub fn separator(label: &str) -> String {
    let cols = crossterm::terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(80)
        .max(20);
    let line = char::from_u32(0x2500).unwrap_or('-');
    let prefix = format!("{line} {label} ");
    let prefix_width = measure_text_width(&prefix);

    if prefix_width >= cols {
        return format!("{line} {label}");
    }

    format!("{}{}", prefix, line.to_string().repeat(cols - prefix_width))
}

/// Prepare the terminal for an interactive prompt.
pub fn before_prompt() {
    let _ = crossterm::terminal::disable_raw_mode();
    reset_line();
}

/// Restore terminal state before handing control back to the shell.
pub fn restore() {
    let _ = crossterm::terminal::disable_raw_mode();
    reset_line();
}
