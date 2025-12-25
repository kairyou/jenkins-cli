/// Prompt helpers with Ctrl+C support.
use std::error::Error;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};

static PROMPTING: AtomicBool = AtomicBool::new(false);

/// Run a prompt while marking user input as active (prevents other readers from stealing input).
pub fn with_prompt<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    use std::io::{self, Write};

    // Ensure prompts start at column 0 to avoid indentation drift.
    eprint!("\r\x1b[2K");
    let _ = io::stderr().flush();

    PROMPTING.store(true, Ordering::SeqCst);
    let result = f();
    PROMPTING.store(false, Ordering::SeqCst);
    result
}

/// Whether a dialoguer prompt is currently active.
pub fn is_prompting() -> bool {
    PROMPTING.load(Ordering::SeqCst)
}

/// Check if error is an interrupted error (Ctrl+C)
/// Uses multiple strategies to detect Ctrl+C interruption
fn is_interrupted_error(e: &dialoguer::Error) -> bool {
    // Use multiple strategies to stay compatible across dialoguer versions and error wrappers.
    // Strategy 1: Try to downcast to io::Error directly
    if let Some(io_err) = (e as &dyn Error).downcast_ref::<std::io::Error>() {
        if io_err.kind() == ErrorKind::Interrupted {
            return true;
        }
    }

    // Strategy 2: Walk the error chain
    let mut current: Option<&dyn Error> = Some(e);
    while let Some(err) = current {
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            if io_err.kind() == ErrorKind::Interrupted {
                return true;
            }
        }
        current = err.source();
    }

    // Strategy 3: Check error message (last resort but reliable)
    let err_str = format!("{:?}", e);
    if err_str.contains("Interrupted") || err_str.to_lowercase().contains("interrupted") {
        return true;
    }

    let err_display = e.to_string();
    if err_display.to_lowercase().contains("interrupted") {
        return true;
    }

    false
}

/// Handle dialoguer Result, converting interrupted errors to None
/// This allows Ctrl+C to act as "go back" in selection prompts
pub fn handle_selection<T>(result: Result<T, dialoguer::Error>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            if is_interrupted_error(&e) {
                return None; // Ctrl+C pressed - go back
            }
            // Not an interrupted error, exit
            eprintln!("Selection error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Handle dialoguer confirm Result (returns bool instead of index)
pub fn handle_confirm(result: Result<bool, dialoguer::Error>) -> Option<bool> {
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            if is_interrupted_error(&e) {
                return None; // Ctrl+C pressed - go back
            }
            eprintln!("Confirmation error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Handle dialoguer input Result (for String inputs)
pub fn handle_input(result: Result<String, dialoguer::Error>) -> Option<String> {
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            if is_interrupted_error(&e) {
                return None; // Ctrl+C pressed - go back
            }
            eprintln!("Input error: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_interrupted_error;
    use std::io;

    #[test]
    fn detects_interrupted_error() {
        let err = dialoguer::Error::IO(io::Error::new(io::ErrorKind::Interrupted, "ctrl-c"));
        assert!(is_interrupted_error(&err));
    }

    #[test]
    fn ignores_non_interrupted_error() {
        let err = dialoguer::Error::IO(io::Error::new(io::ErrorKind::Other, "oops"));
        assert!(!is_interrupted_error(&err));
    }
}
