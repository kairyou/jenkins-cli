use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use once_cell::sync::Lazy;
use std::sync::Mutex;

static ACTIVE_SPINNER: Lazy<Mutex<Option<ProgressBar>>> = Lazy::new(|| Mutex::new(None));
use std::time::Duration;

pub struct Spinner {
    spinner: ProgressBar,
}

impl Spinner {
    pub fn new(msg: String) -> Self {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["-", "\\", "|", "/"])
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        spinner.enable_steady_tick(Duration::from_millis(100));
        spinner.set_message(msg); // set message
        if let Ok(mut guard) = ACTIVE_SPINNER.lock() {
            *guard = Some(spinner.clone());
        }
        Spinner { spinner }
    }

    pub fn finish_with_message(self, msg: String) {
        self.spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&[""])
                .template("{msg}")
                .unwrap(),
        );
        self.spinner.finish_with_message(msg);
        if let Ok(mut guard) = ACTIVE_SPINNER.lock() {
            guard.take();
        }
    }

    // expose more ProgressBar methods

    // suspend spinner and execute a function
    pub fn suspend<F: FnOnce() -> T, T>(&self, f: F) -> T {
        self.spinner.suspend(f)
    }
    // set message
    #[allow(dead_code)]
    pub fn set_message(&self, msg: String) {
        self.spinner.set_message(msg);
    }

    // set spinner speed
    #[allow(dead_code)]
    pub fn enable_steady_tick(&self, ms: u64) {
        self.spinner.enable_steady_tick(Duration::from_millis(ms));
    }
}

pub fn pause_active_spinner() {
    if let Ok(guard) = ACTIVE_SPINNER.lock() {
        if let Some(spinner) = guard.as_ref() {
            spinner.set_draw_target(ProgressDrawTarget::hidden());
        }
    }
}

pub fn resume_active_spinner() {
    if let Ok(guard) = ACTIVE_SPINNER.lock() {
        if let Some(spinner) = guard.as_ref() {
            spinner.set_draw_target(ProgressDrawTarget::stderr());
        }
    }
}

pub fn clear_active_spinner() {
    if let Ok(mut guard) = ACTIVE_SPINNER.lock() {
        if let Some(spinner) = guard.take() {
            spinner.finish_and_clear();
        }
    }
}
