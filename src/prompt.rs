/// Prompt helpers with Ctrl+C support.
use std::error::Error;
use std::io::{self, ErrorKind, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static PROMPTING: AtomicBool = AtomicBool::new(false);
static PROMPT_KIND: AtomicU8 = AtomicU8::new(PromptKind::Other as u8);
#[cfg(windows)]
static INPUT_INTERRUPT_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum PromptKind {
    Other = 0,
    FuzzySelect = 1,
    FuzzySelectVim = 2,
    Confirm = 3,
    Input = 4,
}

impl PromptKind {
    #[cfg(windows)]
    fn from_u8(v: u8) -> Self {
        match v {
            1 => PromptKind::FuzzySelect,
            2 => PromptKind::FuzzySelectVim,
            3 => PromptKind::Confirm,
            4 => PromptKind::Input,
            _ => PromptKind::Other,
        }
    }
}

struct PromptStateGuard {
    prev_prompting: bool,
    prev_kind: u8,
}

impl PromptStateGuard {
    fn enter(kind: PromptKind) -> Self {
        #[cfg(windows)]
        INPUT_INTERRUPT_REQUESTED.store(false, Ordering::SeqCst);
        let prev_prompting = PROMPTING.swap(true, Ordering::SeqCst);
        let prev_kind = PROMPT_KIND.swap(kind as u8, Ordering::SeqCst);
        Self {
            prev_prompting,
            prev_kind,
        }
    }
}

impl Drop for PromptStateGuard {
    fn drop(&mut self) {
        PROMPT_KIND.store(self.prev_kind, Ordering::SeqCst);
        PROMPTING.store(self.prev_prompting, Ordering::SeqCst);
    }
}

/// Run a prompt while marking user input as active (prevents other readers from stealing input).
pub fn with_prompt<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    with_prompt_kind(PromptKind::Other, f)
}

/// Run a prompt with explicit prompt-kind metadata (used for Ctrl+C compatibility paths).
pub fn with_prompt_kind<F, R>(kind: PromptKind, f: F) -> R
where
    F: FnOnce() -> R,
{
    // Ensure prompts start at column 0 to avoid indentation drift.
    crate::terminal::before_prompt();

    let _prompt_guard = PromptStateGuard::enter(kind);
    f()
}

/// Whether a dialoguer prompt is currently active.
pub fn is_prompting() -> bool {
    PROMPTING.load(Ordering::SeqCst)
}

#[cfg(windows)]
fn current_prompt_kind() -> PromptKind {
    PromptKind::from_u8(PROMPT_KIND.load(Ordering::SeqCst))
}

#[cfg(windows)]
fn current_prompt_kind_label() -> &'static str {
    match current_prompt_kind() {
        PromptKind::Other => "other",
        PromptKind::FuzzySelect => "fuzzy",
        PromptKind::FuzzySelectVim => "fuzzy-vim",
        PromptKind::Confirm => "confirm",
        PromptKind::Input => "input",
    }
}

/// Best-effort helper used by the global Ctrl+C handler during prompt input.
/// On Windows, inject a key sequence (Esc or Esc+q) so dialoguer `interact_opt()`
/// prompts can back out cleanly instead of terminating the process.
#[cfg(windows)]
pub fn request_prompt_back() {
    let (ok, label) = match current_prompt_kind() {
        PromptKind::FuzzySelectVim => (
            inject_key_sequence(&[InjectedKey::escape(), InjectedKey::char('q')]),
            "ESC+q",
        ),
        PromptKind::Confirm => (inject_key_sequence(&[InjectedKey::escape()]), "ESC"),
        PromptKind::Input => (inject_key_sequence(&[InjectedKey::enter()]), "Enter(input-interrupt)"),
        _ => (inject_key_sequence(&[InjectedKey::escape()]), "ESC"),
    };
    if ok && matches!(current_prompt_kind(), PromptKind::Input) {
        INPUT_INTERRUPT_REQUESTED.store(true, Ordering::SeqCst);
    } else if !ok {
        INPUT_INTERRUPT_REQUESTED.store(false, Ordering::SeqCst);
    }
    if ok {
        crate::utils::debug_line(&format!(
            "[debug] prompt: kind={} injected {} for Ctrl+C",
            current_prompt_kind_label(),
            label
        ));
    } else {
        crate::utils::debug_line(&format!(
            "[debug] prompt: kind={} failed to inject {} for Ctrl+C",
            current_prompt_kind_label(),
            label
        ));
    }
}

#[derive(Clone)]
struct ReedlineTextPrompt {
    prompt_text: String,
}

impl reedline::Prompt for ReedlineTextPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.prompt_text)
    }

    fn render_prompt_right(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: reedline::PromptEditMode) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: reedline::PromptHistorySearch,
    ) -> std::borrow::Cow<'_, str> {
        let prefix = match history_search.status {
            reedline::PromptHistorySearchStatus::Passing => "",
            reedline::PromptHistorySearchStatus::Failing => "failing ",
        };
        std::borrow::Cow::Owned(format!("({prefix}reverse-search: {}) ", history_search.term))
    }
}

/// Read multi-line text while keeping Enter as submit.
pub fn text_input(prompt_text: &str, default_value: &str) -> Option<String> {
    use reedline::{
        default_emacs_keybindings, EditCommand, Emacs, KeyCode, KeyModifiers, Reedline, ReedlineEvent, Signal,
    };

    with_prompt_kind(PromptKind::Input, || {
        let mut keybindings = default_emacs_keybindings();
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Char('j'),
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );
        keybindings.remove_binding(KeyModifiers::SHIFT, KeyCode::Enter);

        let mut editor = Reedline::create()
            .with_edit_mode(Box::new(Emacs::new(keybindings)))
            .use_bracketed_paste(true)
            .use_kitty_keyboard_enhancement(false);

        if !default_value.is_empty() {
            editor.run_edit_commands(&[EditCommand::InsertString(default_value.to_string())]);
        }

        let prompt = ReedlineTextPrompt {
            prompt_text: format!("{prompt_text}\n"),
        };

        match editor.read_line(&prompt) {
            Ok(Signal::Success(value)) => Some(value),
            Ok(Signal::CtrlC | Signal::CtrlD) => None,
            Ok(Signal::ExternalBreak(_)) => None,
            Ok(_) => None,
            Err(_) => None,
        }
    })
}

pub fn string_input(prompt_text: &str, default_value: &str, trim: Option<bool>) -> Option<String> {
    let user_value = handle_input(with_prompt_kind(PromptKind::Input, || {
        dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(prompt_text)
            .with_initial_text(default_value.to_string())
            .allow_empty(true)
            .interact_text()
    }))?;

    Some(if trim.unwrap_or(false) {
        user_value.trim().to_string()
    } else {
        user_value
    })
}

pub fn password_input(prompt_text: &str, default_value: &str) -> Option<String> {
    with_prompt(|| {
        use console::measure_text_width;
        use crossterm::event::{self, Event, KeyCode, KeyModifiers};
        use crossterm::terminal;

        print!("{}", prompt_text);
        let _ = io::stdout().flush();

        if let Ok((cols, _)) = terminal::size() {
            if measure_text_width(prompt_text) + 1 >= cols as usize {
                println!();
            } else {
                print!(" ");
            }
        } else {
            print!(" ");
        }
        let _ = io::stdout().flush();

        let mut raw_active = terminal::enable_raw_mode().is_ok();
        let mut input = String::new();
        loop {
            match event::read() {
                Ok(Event::Key(key)) => match key.code {
                    KeyCode::Enter => {
                        if raw_active {
                            let _ = terminal::disable_raw_mode();
                        }
                        print!("\r\n");
                        let _ = io::stdout().flush();
                        raw_active = false;
                        break;
                    }
                    KeyCode::Backspace if !input.is_empty() => {
                        input.pop();
                        print!("\x08 \x08");
                        let _ = io::stdout().flush();
                    }
                    KeyCode::Char('\u{3}') | KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if raw_active {
                            let _ = terminal::disable_raw_mode();
                        }
                        print!("\r\n");
                        let _ = io::stdout().flush();
                        return None;
                    }
                    KeyCode::Char(ch) => {
                        input.push(ch);
                        print!("*");
                        let _ = io::stdout().flush();
                    }
                    _ => {}
                },
                Ok(_) => {}
                Err(_) => {
                    if raw_active {
                        let _ = terminal::disable_raw_mode();
                    }
                    print!("\r\n");
                    let _ = io::stdout().flush();
                    return None;
                }
            }
        }

        if raw_active {
            let _ = terminal::disable_raw_mode();
        }

        if input.is_empty() {
            Some(default_value.to_string())
        } else {
            Some(input)
        }
    })
}

#[cfg(windows)]
#[derive(Copy, Clone)]
struct InjectedKey {
    vk: u16,
    unicode_char: u16,
}

#[cfg(windows)]
impl InjectedKey {
    fn escape() -> Self {
        Self {
            vk: 0x1B,
            unicode_char: 0x1B,
        }
    }

    fn char(ch: char) -> Self {
        let c = ch as u32;
        let unicode = u16::try_from(c).unwrap_or_default();
        let vk = if ch.is_ascii_alphabetic() {
            ch.to_ascii_uppercase() as u16
        } else if ch.is_ascii_digit() {
            ch as u16
        } else {
            0
        };
        Self {
            vk,
            unicode_char: unicode,
        }
    }

    fn enter() -> Self {
        Self {
            vk: 0x0D,           // VK_RETURN
            unicode_char: 0x0D, // '\r'
        }
    }
}

#[cfg(windows)]
fn inject_key_sequence(keys: &[InjectedKey]) -> bool {
    use std::mem;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::winbase::STD_INPUT_HANDLE;
    use winapi::um::wincon::{WriteConsoleInputW, INPUT_RECORD, KEY_EVENT};

    unsafe fn make_key_event(key_spec: InjectedKey, is_key_down: i32) -> INPUT_RECORD {
        let mut record: INPUT_RECORD = mem::zeroed();
        record.EventType = KEY_EVENT;
        let key = record.Event.KeyEvent_mut();
        (*key).bKeyDown = is_key_down;
        (*key).wRepeatCount = 1;
        (*key).wVirtualKeyCode = key_spec.vk;
        (*key).wVirtualScanCode = 0;
        *(*key).uChar.UnicodeChar_mut() = key_spec.unicode_char;
        (*key).dwControlKeyState = 0;
        record
    }

    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut records = Vec::with_capacity(keys.len() * 2);
        for key_spec in keys {
            records.push(make_key_event(*key_spec, 1));
            records.push(make_key_event(*key_spec, 0));
        }
        let mut written: DWORD = 0;
        let ok = WriteConsoleInputW(handle, records.as_ptr(), records.len() as DWORD, &mut written);
        ok != 0 && written == records.len() as DWORD
    }
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

/// Handle dialoguer optional selection Result (`interact_opt`) where Esc/q can return `None`.
pub fn handle_selection_opt<T>(result: Result<Option<T>, dialoguer::Error>) -> Option<T> {
    match result {
        Ok(value) => value,
        Err(e) => {
            if is_interrupted_error(&e) {
                crate::utils::debug_line("[debug] prompt: interrupted (selection)");
                return None; // Ctrl+C pressed - go back
            }
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
                crate::utils::debug_line("[debug] prompt: interrupted (confirm)");
                return None; // Ctrl+C pressed - go back
            }
            eprintln!("Confirmation error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Handle dialoguer optional confirm Result (`interact_opt`) where Esc/q can return `None`.
pub fn handle_confirm_opt(result: Result<Option<bool>, dialoguer::Error>) -> Option<bool> {
    match result {
        Ok(value) => value,
        Err(e) => {
            if is_interrupted_error(&e) {
                crate::utils::debug_line("[debug] prompt: interrupted (confirm)");
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
        Ok(value) => {
            #[cfg(windows)]
            if INPUT_INTERRUPT_REQUESTED.swap(false, Ordering::SeqCst) {
                crate::utils::debug_line("[debug] prompt: interrupted (input via injected Enter)");
                return None;
            }
            Some(value)
        }
        Err(e) => {
            if is_interrupted_error(&e) {
                crate::utils::debug_line("[debug] prompt: interrupted (input)");
                return None; // Ctrl+C pressed - go back
            }
            #[cfg(windows)]
            INPUT_INTERRUPT_REQUESTED.store(false, Ordering::SeqCst);
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
        let err = dialoguer::Error::IO(io::Error::other("oops"));
        assert!(!is_interrupted_error(&err));
    }
}
