// Ctrl+C handling and build cancellation flow.

use colored::Colorize;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tokio::sync::{mpsc, Mutex, Notify};

use crate::i18n::macros::t;
use crate::jenkins::{client::JenkinsClient, Event};
use crate::prompt;
use crate::spinner;
use crate::utils::{debug_enabled, debug_line, delay, flush_stdin, prepare_terminal_for_exit, reset_terminal_line};

// Configuration constants.
const CTRL_C_EXIT_WINDOW_MS: u64 = 800;
const CANCEL_MAX_ATTEMPTS: u32 = 10;
const CANCEL_MAX_WAIT: tokio::time::Duration = tokio::time::Duration::from_secs(30);
const CANCEL_RETRY_DELAY_MS: u64 = 1000;
const CANCEL_VERIFY_DELAY_MS: u64 = 3000;

// Shared Ctrl+C context used by the build/queue cancellation prompt.
struct CtrlCContext {
    client: std::sync::Arc<tokio::sync::RwLock<JenkinsClient>>,
    event_sender: mpsc::Sender<Event>,
}

// Ctrl+C phase transitions:
// Idle -> Polling (queue/build polling begins)
// Polling -> Cancelling (user confirms cancel)
// Polling -> Idle (build/queue completes)
// Cancelling -> Idle (cancel flow ends or times out)
// Ctrl+C during Cancelling or a double-press anywhere forces immediate exit.
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum CtrlCPhase {
    Idle = 0,
    Polling = 1,
    Cancelling = 2,
}

pub struct CtrlCControl {
    ctx: Mutex<Option<CtrlCContext>>,
    // Current input/cancel phase used by Ctrl+C handling.
    phase: AtomicU8,
    // Drives shutdown of the background key listener.
    app_running: AtomicBool,
    // Allows main to await completion of cancel flow.
    cancel_notify: Notify,
    // Used to detect double Ctrl+C exit.
    last_ctrl_c_ms: std::sync::atomic::AtomicU64,
}

impl CtrlCControl {
    fn new() -> Self {
        Self {
            ctx: Mutex::new(None),
            phase: AtomicU8::new(CtrlCPhase::Idle as u8),
            app_running: AtomicBool::new(true),
            cancel_notify: Notify::new(),
            last_ctrl_c_ms: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn set_phase(&self, phase: CtrlCPhase) {
        self.phase.store(phase as u8, Ordering::SeqCst);
    }

    /// Reset polling state only if no cancel flow is active.
    pub fn finish_polling(&self) {
        if self.phase() == CtrlCPhase::Polling {
            self.set_phase(CtrlCPhase::Idle);
        }
    }

    /// Current Ctrl+C phase used by handlers and the key listener.
    pub fn phase(&self) -> CtrlCPhase {
        match self.phase.load(Ordering::SeqCst) {
            1 => CtrlCPhase::Polling,
            2 => CtrlCPhase::Cancelling,
            _ => CtrlCPhase::Idle,
        }
    }

    pub fn phase_label(&self) -> &'static str {
        match self.phase() {
            CtrlCPhase::Idle => "idle",
            CtrlCPhase::Polling => "polling",
            CtrlCPhase::Cancelling => "cancelling",
        }
    }

    pub fn notify_cancel_waiters(&self) {
        self.cancel_notify.notify_waiters();
    }

    /// Block until the cancel flow has completed.
    pub async fn wait_for_cancel(&self) {
        self.cancel_notify.notified().await;
    }

    pub async fn set_ctx(
        &self,
        client: std::sync::Arc<tokio::sync::RwLock<JenkinsClient>>,
        event_sender: mpsc::Sender<Event>,
    ) {
        let mut guard = self.ctx.lock().await;
        *guard = Some(CtrlCContext { client, event_sender });
    }

    pub async fn get_ctx(&self) -> Option<(std::sync::Arc<tokio::sync::RwLock<JenkinsClient>>, mpsc::Sender<Event>)> {
        let guard = self.ctx.lock().await;
        guard
            .as_ref()
            .map(|state| (state.client.clone(), state.event_sender.clone()))
    }

    pub fn should_force_exit(&self, window_ms: u64) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let prev = self.last_ctrl_c_ms.swap(now_ms, Ordering::SeqCst);
        now_ms.saturating_sub(prev) <= window_ms
    }

    pub fn set_app_running(&self, running: bool) {
        self.app_running.store(running, Ordering::SeqCst);
    }

    pub fn app_running(&self) -> bool {
        self.app_running.load(Ordering::SeqCst)
    }
}

pub static CTRL_C: Lazy<CtrlCControl> = Lazy::new(CtrlCControl::new);

macro_rules! debug_ctrlc {
    ($($arg:tt)*) => {
        if debug_enabled() {
            debug_line(&format!(
                "[debug] ctrlc: {}",
                format_args!($($arg)*)
            ));
        }
    };
}

macro_rules! debug_cancel {
    ($($arg:tt)*) => {
        if debug_enabled() {
            debug_line(&format!(
                "[debug] cancel_build: {}",
                format_args!($($arg)*)
            ));
        }
    };
}

fn force_exit() -> ! {
    spinner::clear_active_spinner();
    prepare_terminal_for_exit();
    CTRL_C.notify_cancel_waiters();
    println!("Ctrl+C pressed again, exiting immediately.");
    std::process::exit(1);
}

/// Global Ctrl+C handler. During selection it lets dialoguer handle the interrupt.
/// During build/queue it asks whether to cancel and then exits.
pub async fn handle_ctrl_c(mut ctrlc_rx: mpsc::UnboundedReceiver<()>) {
    use crossterm::terminal;
    use tokio::signal;

    // Central Ctrl+C loop: selection is handled by dialoguer, polling prompts the cancel flow.
    loop {
        let detected = tokio::select! {
            _ = signal::ctrl_c() => true,
            msg = ctrlc_rx.recv() => msg.is_some(),
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => false,
        };

        if !detected {
            continue;
        }

        if CTRL_C.phase() == CtrlCPhase::Cancelling {
            force_exit();
        }

        if CTRL_C.phase() != CtrlCPhase::Polling {
            if CTRL_C.should_force_exit(CTRL_C_EXIT_WINDOW_MS) {
                force_exit();
            }
            continue;
        }

        debug_ctrlc!("received, phase={}", CTRL_C.phase_label());

        let (client, event_sender) = match CTRL_C.get_ctx().await {
            Some(ctx) => ctx,
            None => continue,
        };

        // Pause spinner output so the prompt is readable.
        let _ = event_sender.send(Event::StopSpinner).await;
        let _ = terminal::disable_raw_mode();
        spinner::pause_active_spinner();

        reset_terminal_line();
        println!("Checking for running builds...");
        flush_stdin();

        let prompt = t!("cancel-build-prompt").red().bold().to_string();
        let confirm = prompt::handle_confirm(prompt::with_prompt(|| {
            dialoguer::Confirm::new().with_prompt(prompt).default(false).interact()
        }));

        let Some(confirm) = confirm else {
            force_exit();
        };

        if !confirm {
            let _ = event_sender.send(Event::ResumeSpinner).await;
            spinner::resume_active_spinner();
            let _ = terminal::enable_raw_mode();
            continue;
        }

        CTRL_C.set_phase(CtrlCPhase::Cancelling);
        let _ = event_sender.send(Event::CancelPolling).await;
        println!("{}", t!("cancelling-build").yellow());
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            cancel_running_build(client).await;
            let _ = done_tx.send(());
        });
        tokio::select! {
          _ = signal::ctrl_c() => {
              CTRL_C.set_phase(CtrlCPhase::Idle);
              force_exit();
          },
          _ = ctrlc_rx.recv() => {
              CTRL_C.set_phase(CtrlCPhase::Idle);
              force_exit();
          },
          _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {
              CTRL_C.set_phase(CtrlCPhase::Idle);
              eprintln!("{}", t!("cancel-build-failed").red());
          },
          _ = &mut done_rx => {
              // Cancellation completed (or timed out internally)
          }
        }

        CTRL_C.set_phase(CtrlCPhase::Idle);
        spinner::clear_active_spinner();
        prepare_terminal_for_exit();
        CTRL_C.notify_cancel_waiters();
        println!("{}", t!("bye"));
        std::process::exit(0);
    }
}

pub async fn spawn_ctrl_c_key_listener(sender: mpsc::UnboundedSender<()>) {
    tokio::task::spawn_blocking(move || {
        use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
        use std::time::Duration;

        let mut raw_enabled = false;

        // Dedicated raw-mode listener for polling/cancelling phases.
        loop {
            if !CTRL_C.app_running() {
                if raw_enabled {
                    let _ = crossterm::terminal::disable_raw_mode();
                }
                break;
            }
            if matches!(CTRL_C.phase(), CtrlCPhase::Polling | CtrlCPhase::Cancelling) {
                if prompt::is_prompting() {
                    if raw_enabled {
                        let _ = crossterm::terminal::disable_raw_mode();
                        raw_enabled = false;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                if !raw_enabled {
                    let _ = crossterm::terminal::enable_raw_mode();
                    raw_enabled = true;
                    debug_ctrlc!("key listener: raw enabled");
                }

                if let Ok(true) = event::poll(Duration::from_millis(100)) {
                    if let Ok(Event::Key(key_event)) = event::read() {
                        if key_event.kind == KeyEventKind::Press {
                            let is_ctrl_c = matches!(key_event.code, KeyCode::Char('\u{3}'))
                                || (matches!(key_event.code, KeyCode::Char('c' | 'C'))
                                    && key_event.modifiers.contains(KeyModifiers::CONTROL));
                            if is_ctrl_c {
                                debug_ctrlc!("key listener: detected");
                                if CTRL_C.phase() == CtrlCPhase::Cancelling {
                                    force_exit();
                                }
                                let _ = sender.send(());
                            }
                        }
                    }
                }
            } else {
                if raw_enabled {
                    let _ = crossterm::terminal::disable_raw_mode();
                    raw_enabled = false;
                    debug_ctrlc!("key listener: raw disabled");
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

struct CancelContext<'a> {
    client: &'a JenkinsClient,
    attempts: u32,
    last_id: Option<u32>,
    stable_count: u32,
    started_at: tokio::time::Instant,
}

impl<'a> CancelContext<'a> {
    fn new(client: &'a JenkinsClient) -> Self {
        Self {
            client,
            attempts: 0,
            last_id: None,
            stable_count: 0,
            started_at: tokio::time::Instant::now(),
        }
    }
}

fn finish_ok<T: std::fmt::Display>(msg: T) {
    spinner::clear_active_spinner();
    prepare_terminal_for_exit();
    println!("{msg}");
}

fn finish_err<T: std::fmt::Display>(msg: T) {
    spinner::clear_active_spinner();
    prepare_terminal_for_exit();
    eprintln!("{msg}");
}

fn record_idle_attempt(ctx: &mut CancelContext<'_>, status: &crate::jenkins::client::BuildStatus) {
    // Use repeated idle snapshots to confirm the build is really finished.
    if status.id == ctx.last_id {
        ctx.stable_count += 1;
    } else {
        ctx.last_id = status.id;
        ctx.stable_count = 1;
    }
}

/// Fetch build status with a short timeout to avoid hanging the cancel flow.
async fn is_building_with_timeout(client: &JenkinsClient) -> Result<crate::jenkins::client::BuildStatus, ()> {
    tokio::time::timeout(tokio::time::Duration::from_secs(5), client.is_building())
        .await
        .ok()
        .and_then(|res| res.ok())
        .ok_or(())
}

/// Send a stop request with a timeout to prevent long stalls.
async fn stop_build_with_timeout(client: &JenkinsClient, id: Option<u32>) -> Result<(), ()> {
    tokio::time::timeout(tokio::time::Duration::from_secs(5), client.cancel_build(id))
        .await
        .ok()
        .and_then(|res| res.ok())
        .map(|_| ())
        .ok_or(())
}

/// Poll until Jenkins reports the build stopped, retrying stop if needed.
async fn verify_stop(client: &JenkinsClient) -> bool {
    let mut attempts = 0;
    while attempts < CANCEL_MAX_ATTEMPTS {
        match is_building_with_timeout(client).await {
            Ok(status) if !status.building => return true,
            Ok(status) => {
                debug_cancel!("still building, retry stop");
                let _ = stop_build_with_timeout(client, status.id).await;
            }
            Err(_) => {
                debug_cancel!("verify timeout/error, retrying");
            }
        }
        attempts += 1;
        delay(CANCEL_VERIFY_DELAY_MS).await;
    }
    false
}

async fn cancel_running_build(client: std::sync::Arc<tokio::sync::RwLock<JenkinsClient>>) {
    // Best-effort cancel flow with retries + status verification.
    let client_guard = client.read().await;
    let mut ctx = CancelContext::new(&client_guard);

    debug_cancel!("start");

    loop {
        if ctx.started_at.elapsed() >= CANCEL_MAX_WAIT {
            debug_cancel!("timed out waiting for build");
            return;
        }

        debug_cancel!("checking build (attempt {}/{})", ctx.attempts + 1, CANCEL_MAX_ATTEMPTS);

        let status = match is_building_with_timeout(ctx.client).await {
            Ok(status) => status,
            Err(_) => {
                debug_cancel!("is_building timed out");
                if ctx.attempts >= CANCEL_MAX_ATTEMPTS {
                    debug_cancel!("timed out querying build");
                    finish_err(t!("cancel-build-failed").red());
                    return;
                }
                ctx.attempts += 1;
                delay(CANCEL_RETRY_DELAY_MS).await;
                continue;
            }
        };

        debug_cancel!(
            "is_building={} id={:?} lastBuild={:?} lastCompleted={:?} inQueue={}",
            status.building,
            status.id,
            status.last_build,
            status.last_completed,
            status.in_queue
        );

        if !status.building {
            if status.in_queue {
                ctx.attempts += 1;
                delay(CANCEL_RETRY_DELAY_MS).await;
                continue;
            }
            record_idle_attempt(&mut ctx, &status);
            if ctx.stable_count >= 3 {
                finish_ok(t!("build-already-completed").yellow());
                return;
            }
            if ctx.attempts >= CANCEL_MAX_ATTEMPTS {
                debug_cancel!("no running build after retries");
                finish_ok(t!("build-already-completed").yellow());
                return;
            }
            ctx.attempts += 1;
            delay(CANCEL_RETRY_DELAY_MS).await;
            continue;
        }

        if let Some(num) = status.id {
            spinner::clear_active_spinner();
            reset_terminal_line();
            println!("{}: {}", t!("current-build-id"), num.to_string().cyan().bold());
        }
        debug_cancel!("sending stop");

        match stop_build_with_timeout(ctx.client, status.id).await {
            Ok(_) => {
                debug_cancel!("stop request ok, verifying");
                if verify_stop(ctx.client).await {
                    finish_ok(t!("build-cancelled").green());
                } else {
                    finish_err(t!("cancel-build-failed").red());
                }
                return;
            }
            Err(_) => {
                debug_cancel!("cancel request timed out");
                finish_err(t!("cancel-build-failed").red());
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CtrlCControl;

    #[test]
    fn double_ctrl_c_within_window_triggers_exit() {
        let ctrl = CtrlCControl::new();
        assert!(!ctrl.should_force_exit(1_000));
        assert!(ctrl.should_force_exit(1_000));
    }
}
