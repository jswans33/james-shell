use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType},
    tty::IsTty,
};

// ── Raw-mode sentinel ─────────────────────────────────────────────────────────

/// `true` while the line editor holds the terminal in raw mode.
///
/// The `ctrlc` handler in `main.rs` reads this flag to suppress the spurious
/// newline it would otherwise print on platforms where SIGINT can still be
/// delivered during raw mode (primarily Windows).
pub static EDITOR_ACTIVE: AtomicBool = AtomicBool::new(false);

// ── Raw-mode guard ────────────────────────────────────────────────────────────

/// RAII guard: enables terminal raw mode on construction and restores it on
/// drop — even on panic — so the terminal is never left in a broken state.
struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        EDITOR_ACTIVE.store(true, Ordering::Relaxed);
        Ok(RawModeGuard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        EDITOR_ACTIVE.store(false, Ordering::Relaxed);
    }
}

// ── Line editor ───────────────────────────────────────────────────────────────

const MAX_HISTORY_SIZE: usize = 10_000;

/// A line editor with cursor movement, Emacs keybindings, and persistent history.
pub struct LineEditor {
    /// Current line content, stored as `char`s for Unicode-safe cursor indexing.
    buffer: Vec<char>,
    /// Cursor position within `buffer` (0 = before the first char).
    cursor: usize,
    /// Command history (oldest → newest).
    history: Vec<String>,
    /// Index into `history` during navigation; equals `history.len()` otherwise.
    history_idx: usize,
    /// Snapshot of the in-progress line taken the first time the user presses Up.
    /// Restored when the user presses Down past the end of the history list.
    saved_buffer: String,
    /// Path to `~/.jsh_history`, or `None` when HOME is not set.
    history_path: Option<PathBuf>,
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl LineEditor {
    /// Create a new editor and load history from `~/.jsh_history`.
    pub fn new() -> Self {
        let history_path = history_file_path();
        let history = history_path
            .as_deref()
            .map(load_history)
            .unwrap_or_default();
        let history_idx = history.len();
        LineEditor {
            buffer: Vec::new(),
            cursor: 0,
            history,
            history_idx,
            saved_buffer: String::new(),
            history_path,
        }
    }

    /// Read one line of input, displaying `prompt` to the left.
    ///
    /// Returns:
    /// - `Ok(Some(line))` — the user submitted a line (may be empty)
    /// - `Ok(None)` — EOF (Ctrl-D on an empty buffer, or stdin was closed)
    /// - `Err(_)` — I/O error (including `ErrorKind::Interrupted` for SIGINT)
    ///
    /// When stdout is not a TTY (e.g. integration tests that pipe stdin/stdout)
    /// the method falls back to a plain `read_line()` call so tests work
    /// without modification.
    pub fn read_line(&mut self, prompt: &str) -> io::Result<Option<String>> {
        // Gate on stdin, not stdout: interactive editing requires a keyboard on
        // the *input* side. `printf 'cmd\n' | james-shell` has stdout on a
        // terminal but stdin on a pipe — entering raw mode there would hand
        // event::read() a non-keyboard stream, causing errors or misparse.
        if !io::stdin().is_tty() {
            return self.read_line_fallback(prompt);
        }

        self.reset_state();
        let _guard = RawModeGuard::enter()?;

        // Raw mode disables echo; we must display the prompt ourselves.
        print!("{prompt}");
        io::stdout().flush()?;

        loop {
            let ev = match event::read() {
                Ok(ev) => ev,
                // crossterm handles EINTR internally, but be defensive.
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };

            let Event::Key(key) = ev else {
                continue; // ignore mouse, resize, paste, etc.
            };

            // Filter out key-release events that Windows may generate.
            if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
                continue;
            }

            match self.handle_key(key, prompt)? {
                KeyAction::Submit(line) => return Ok(Some(line)),
                KeyAction::Eof => return Ok(None),
                KeyAction::Continue => {}
            }
        }
    }

    /// Add `line` to the in-memory history and append it to `~/.jsh_history`.
    ///
    /// Empty lines (after trimming) and consecutive duplicates are silently
    /// ignored. The in-memory list is trimmed to `MAX_HISTORY_SIZE`.
    pub fn add_to_history(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.history.last().map(String::as_str) == Some(trimmed) {
            return;
        }
        self.history.push(trimmed.to_string());
        if self.history.len() > MAX_HISTORY_SIZE {
            let excess = self.history.len() - MAX_HISTORY_SIZE;
            self.history.drain(..excess);
        }
        if let Some(ref path) = self.history_path {
            append_to_history_file(path, trimmed);
        }
    }

    // ── Private ───────────────────────────────────────────────────────────────

    fn reset_state(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_idx = self.history.len();
        self.saved_buffer.clear();
    }

    /// Non-TTY path: print prompt and delegate to `BufRead::read_line`.
    fn read_line_fallback(&mut self, prompt: &str) -> io::Result<Option<String>> {
        print!("{prompt}");
        io::stdout().flush()?;
        let stdin = io::stdin();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(line)),
            Err(e) => Err(e),
        }
    }

    fn handle_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        prompt: &str,
    ) -> io::Result<KeyAction> {
        use KeyCode::*;
        use KeyModifiers as Mod;

        match (key.code, key.modifiers) {
            // ── Submit ────────────────────────────────────────────────────────
            (Enter, _) => {
                // Raw mode suppresses the terminal's automatic newline on Enter.
                print!("\r\n");
                io::stdout().flush()?;
                let line: String = self.buffer.iter().collect();
                return Ok(KeyAction::Submit(line));
            }

            // ── Ctrl-D: EOF or delete-at-cursor ───────────────────────────────
            (Char('d'), Mod::CONTROL) => {
                if self.buffer.is_empty() {
                    print!("\r\n");
                    io::stdout().flush()?;
                    return Ok(KeyAction::Eof);
                }
                self.delete_at_cursor();
                self.redraw(prompt)?;
            }

            // ── Ctrl-C: clear buffer, re-show prompt ──────────────────────────
            // In raw mode on Unix, ISIG is off so Ctrl-C arrives as a key event
            // rather than SIGINT — the ctrlc crate handler does not fire here.
            (Char('c'), Mod::CONTROL) => {
                print!("^C\r\n{prompt}");
                io::stdout().flush()?;
                self.buffer.clear();
                self.cursor = 0;
                self.history_idx = self.history.len();
                self.saved_buffer.clear();
            }

            // ── Ctrl-L: clear screen ──────────────────────────────────────────
            (Char('l'), Mod::CONTROL) => {
                execute!(
                    io::stdout(),
                    terminal::Clear(ClearType::All),
                    cursor::MoveTo(0, 0),
                )?;
                self.redraw(prompt)?;
            }

            // ── Ctrl-A / Home: jump to start of line ──────────────────────────
            (Char('a'), Mod::CONTROL) | (Home, _) => {
                self.cursor = 0;
                self.sync_cursor(prompt)?;
            }

            // ── Ctrl-E / End: jump to end of line ─────────────────────────────
            (Char('e'), Mod::CONTROL) | (End, _) => {
                self.cursor = self.buffer.len();
                self.sync_cursor(prompt)?;
            }

            // ── Ctrl-K: kill from cursor to end of line ───────────────────────
            (Char('k'), Mod::CONTROL) => {
                self.buffer.truncate(self.cursor);
                self.redraw(prompt)?;
            }

            // ── Ctrl-U: kill from start of line to cursor ─────────────────────
            (Char('u'), Mod::CONTROL) => {
                self.buffer.drain(..self.cursor);
                self.cursor = 0;
                self.redraw(prompt)?;
            }

            // ── Ctrl-W: delete previous word ──────────────────────────────────
            (Char('w'), Mod::CONTROL) => {
                self.delete_word_before_cursor();
                self.redraw(prompt)?;
            }

            // ── Arrow keys ────────────────────────────────────────────────────
            (Left, _) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.sync_cursor(prompt)?;
                }
            }
            (Right, _) => {
                if self.cursor < self.buffer.len() {
                    self.cursor += 1;
                    self.sync_cursor(prompt)?;
                }
            }

            // ── History navigation ────────────────────────────────────────────
            (Up, _) => {
                self.history_prev();
                self.redraw(prompt)?;
            }
            (Down, _) => {
                self.history_next();
                self.redraw(prompt)?;
            }

            // ── Backspace / Delete ────────────────────────────────────────────
            (Backspace, _) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.buffer.remove(self.cursor);
                    self.redraw(prompt)?;
                }
            }
            (Delete, _) => {
                self.delete_at_cursor();
                self.redraw(prompt)?;
            }

            // ── Printable characters ──────────────────────────────────────────
            (Char(c), Mod::NONE) | (Char(c), Mod::SHIFT) => {
                self.buffer.insert(self.cursor, c);
                self.cursor += 1;
                self.redraw(prompt)?;
            }

            // ── Everything else: ignore ───────────────────────────────────────
            _ => {}
        }

        Ok(KeyAction::Continue)
    }

    /// Erase the current line and redraw prompt + buffer, then reposition cursor.
    fn redraw(&self, prompt: &str) -> io::Result<()> {
        let line: String = self.buffer.iter().collect();
        // Prompt length measured in chars (not bytes) for correct column math.
        let col = (prompt.chars().count() + self.cursor) as u16;
        execute!(
            io::stdout(),
            cursor::MoveToColumn(0),
            terminal::Clear(ClearType::CurrentLine),
        )?;
        print!("{prompt}{line}");
        io::stdout().flush()?;
        execute!(io::stdout(), cursor::MoveToColumn(col))?;
        Ok(())
    }

    /// Move the terminal cursor to match `self.cursor` without redrawing text.
    /// Used for pure cursor moves (Left/Right/Home/End) to avoid flicker.
    fn sync_cursor(&self, prompt: &str) -> io::Result<()> {
        let col = (prompt.chars().count() + self.cursor) as u16;
        execute!(io::stdout(), cursor::MoveToColumn(col))?;
        Ok(())
    }

    fn delete_at_cursor(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    fn delete_word_before_cursor(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Skip spaces immediately before the cursor, then the non-space word.
        let mut end = self.cursor;
        while end > 0 && self.buffer[end - 1] == ' ' {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && self.buffer[start - 1] != ' ' {
            start -= 1;
        }
        self.buffer.drain(start..self.cursor);
        self.cursor = start;
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        // On the first Up press, snapshot whatever the user has been typing.
        if self.history_idx == self.history.len() {
            self.saved_buffer = self.buffer.iter().collect();
        }
        if self.history_idx > 0 {
            self.history_idx -= 1;
            self.buffer = self.history[self.history_idx].chars().collect();
            self.cursor = self.buffer.len();
        }
    }

    fn history_next(&mut self) {
        if self.history_idx >= self.history.len() {
            return;
        }
        self.history_idx += 1;
        if self.history_idx == self.history.len() {
            // Restore the buffer that was in progress before the user pressed Up.
            self.buffer = self.saved_buffer.chars().collect();
        } else {
            self.buffer = self.history[self.history_idx].chars().collect();
        }
        self.cursor = self.buffer.len();
    }
}

// ── Internal return type ──────────────────────────────────────────────────────

enum KeyAction {
    Continue,
    Submit(String),
    Eof,
}

// ── History persistence ───────────────────────────────────────────────────────

fn history_file_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|home| PathBuf::from(home).join(".jsh_history"))
}

fn load_history(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn append_to_history_file(path: &std::path::Path, line: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// Build a `LineEditor` with a fixed history, bypassing file I/O.
    fn editor_with_history(entries: &[&str]) -> LineEditor {
        let mut e = LineEditor::new();
        e.history = entries.iter().map(|s| s.to_string()).collect();
        e.history_idx = e.history.len();
        e.history_path = None; // don't touch the real ~/.jsh_history in tests
        e
    }

    #[test]
    fn empty_lines_not_added_to_history() {
        let mut e = editor_with_history(&[]);
        e.add_to_history("");
        e.add_to_history("   ");
        assert!(e.history.is_empty());
    }

    #[test]
    fn consecutive_duplicates_not_added_to_history() {
        let mut e = editor_with_history(&[]);
        e.add_to_history("ls");
        e.add_to_history("ls");
        e.add_to_history("ls");
        assert_eq!(e.history.len(), 1);
    }

    #[test]
    fn non_consecutive_duplicates_are_kept() {
        let mut e = editor_with_history(&[]);
        e.add_to_history("ls");
        e.add_to_history("pwd");
        e.add_to_history("ls");
        assert_eq!(e.history.len(), 3);
    }

    #[test]
    fn history_navigation_saves_and_restores_buffer() {
        let mut e = editor_with_history(&["echo hello", "ls -la"]);
        e.buffer = "pwd".chars().collect();
        e.cursor = 3;

        e.history_prev(); // → "ls -la"
        assert_eq!(e.buffer.iter().collect::<String>(), "ls -la");
        assert_eq!(e.saved_buffer, "pwd");

        e.history_prev(); // → "echo hello"
        assert_eq!(e.buffer.iter().collect::<String>(), "echo hello");

        e.history_prev(); // already at start — no change
        assert_eq!(e.buffer.iter().collect::<String>(), "echo hello");

        e.history_next(); // → "ls -la"
        assert_eq!(e.buffer.iter().collect::<String>(), "ls -la");

        e.history_next(); // → restore "pwd"
        assert_eq!(e.buffer.iter().collect::<String>(), "pwd");

        e.history_next(); // already at end — no change
        assert_eq!(e.buffer.iter().collect::<String>(), "pwd");
    }

    #[test]
    fn ctrl_w_deletes_previous_word() {
        let mut e = editor_with_history(&[]);
        e.buffer = "echo hello world".chars().collect();
        e.cursor = e.buffer.len();
        e.delete_word_before_cursor();
        assert_eq!(e.buffer.iter().collect::<String>(), "echo hello ");
        assert_eq!(e.cursor, "echo hello ".len());
    }

    #[test]
    fn ctrl_w_skips_trailing_spaces() {
        let mut e = editor_with_history(&[]);
        e.buffer = "echo hello   ".chars().collect();
        e.cursor = e.buffer.len();
        e.delete_word_before_cursor();
        assert_eq!(e.buffer.iter().collect::<String>(), "echo ");
        assert_eq!(e.cursor, "echo ".len());
    }

    #[test]
    fn ctrl_w_at_start_is_noop() {
        let mut e = editor_with_history(&[]);
        e.buffer = "hello".chars().collect();
        e.cursor = 0;
        e.delete_word_before_cursor();
        assert_eq!(e.buffer.iter().collect::<String>(), "hello");
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn key_events_edit_buffer_like_terminal() {
        let mut e = editor_with_history(&[]);
        let prompt = "jsh> ";
        let k = |code: KeyCode, mods: KeyModifiers| KeyEvent::new(code, mods);

        e.handle_key(k(KeyCode::Char('h'), KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Char('i'), KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Left, KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Char('i'), KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Right, KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Backspace, KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Home, KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Char('H'), KeyModifiers::SHIFT), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::End, KeyModifiers::NONE), prompt)
            .unwrap();

        assert_eq!(e.buffer.iter().collect::<String>(), "Hhi");
        assert_eq!(e.cursor, e.buffer.len());
    }

    #[test]
    fn key_events_support_kill_line_shortcuts() {
        let mut e = editor_with_history(&[]);
        let prompt = "jsh> ";
        let k = |code: KeyCode, mods: KeyModifiers| KeyEvent::new(code, mods);

        e.handle_key(k(KeyCode::Char('a'), KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Char('b'), KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Char('c'), KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Left, KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Backspace, KeyModifiers::NONE), prompt)
            .unwrap();

        assert_eq!(e.buffer.iter().collect::<String>(), "ac");
        assert_eq!(e.cursor, 1);

        e.handle_key(k(KeyCode::End, KeyModifiers::NONE), prompt)
            .unwrap();
        e.handle_key(k(KeyCode::Char('u'), KeyModifiers::CONTROL), prompt)
            .unwrap();

        assert_eq!(e.buffer.iter().collect::<String>(), "");
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn history_capped_at_max_size() {
        let mut e = editor_with_history(&[]);
        for i in 0..MAX_HISTORY_SIZE + 5 {
            // Each entry must be unique to avoid consecutive-duplicate filtering.
            e.add_to_history(&format!("cmd-{i}"));
        }
        assert_eq!(e.history.len(), MAX_HISTORY_SIZE);
        // Oldest entries should have been evicted; newest should still be present.
        assert_eq!(e.history.last().unwrap(), &format!("cmd-{}", MAX_HISTORY_SIZE + 4));
    }
}
