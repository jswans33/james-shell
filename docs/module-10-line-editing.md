# Module 10: Line Editing & History

## What are we building?

Until now, our shell reads input with `stdin().read_line()`. This works, but it is primitive — you cannot press the left arrow to fix a typo, you cannot press up-arrow to recall the last command, and you certainly cannot tab-complete filenames. After this module, your shell will have a full **line editor** with cursor movement, command history (persistent across sessions), tab completion for files and commands, and syntax highlighting. We will explore two approaches: building a minimal line editor from scratch using `crossterm`, and using the `rustyline` crate which provides all of this out of the box.

---

## Concept 1: Terminal modes — canonical vs raw

Every terminal operates in one of two modes:

### Canonical mode (cooked mode) — the default

In canonical mode, the terminal driver handles line editing for you:

- Input is buffered until the user presses Enter
- Backspace works (the terminal driver deletes characters)
- The program receives complete lines

```
User types: h e l l o [Backspace] [Backspace] p Enter
Program receives: "help\n"
```

This is what `read_line()` uses. The terminal driver is doing the editing; your program just gets the finished result.

### Raw mode

In raw mode, **every keystroke** is delivered to your program immediately, with no processing:

- No line buffering — each key arrives instantly
- Backspace is just a byte (0x7F or 0x08), not an editing action
- Arrow keys arrive as escape sequences (`\x1B[A` for up-arrow)
- No echo — characters are not automatically shown on screen
- Your program must handle **everything**: display, cursor movement, editing

```
User types: h e l l o [Backspace] [Backspace] p Enter
Program receives: 'h' 'e' 'l' 'l' 'o' '\x7F' '\x7F' 'p' '\r'
Program must: display "hello", erase "lo", display "p", handle Enter
```

### Why raw mode?

A line editor needs raw mode because it must intercept every keystroke:

- **Arrow keys** — move the cursor (canonical mode just ignores these)
- **Tab** — trigger completion (canonical mode inserts a tab character)
- **Up/Down** — navigate history (canonical mode does nothing)
- **Ctrl-A / Ctrl-E** — jump to start/end of line
- **Ctrl-W** — delete previous word

### Enabling raw mode with `crossterm`

```toml
# Cargo.toml
[dependencies]
crossterm = "0.28"
```

```rust
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};

fn main() {
    enable_raw_mode().expect("Failed to enable raw mode");

    // ... your line editor runs here ...

    disable_raw_mode().expect("Failed to disable raw mode");
}
```

**Critical:** You must **always** restore canonical mode before your program exits (including on panic). If you leave the terminal in raw mode, the user's terminal will be broken — no echo, no line editing, Enter key does not work properly. Use a guard pattern:

```rust
struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(RawModeGuard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

// Usage: raw mode is automatically restored when `_guard` is dropped
fn read_line_edited() -> io::Result<String> {
    let _guard = RawModeGuard::new()?;
    // ... read keystrokes ...
    // When this function returns (even via panic!), drop restores canonical mode
}
```

---

## Concept 2: ANSI escape codes

In raw mode, the terminal communicates using **escape codes** — special byte sequences that control cursor position, text color, screen clearing, and more. These are called ANSI escape codes (or VT100 sequences) and are supported on virtually every modern terminal, including Windows Terminal, cmd.exe (Windows 10+), and all Unix terminals.

### Anatomy of an escape sequence

```
ESC [ <parameters> <command>

ESC = \x1B (byte 27, the Escape character)
[   = CSI (Control Sequence Introducer)
```

### Common escape codes for a line editor

| Sequence | Effect | Use in shell |
|----------|--------|-------------|
| `\x1B[A` | Cursor up | History: previous command |
| `\x1B[B` | Cursor down | History: next command |
| `\x1B[C` | Cursor right | Move cursor right |
| `\x1B[D` | Cursor left | Move cursor left |
| `\x1B[H` | Cursor to Home | Jump to start of line |
| `\x1B[F` | Cursor to End | Jump to end of line |
| `\x1B[K` | Erase to end of line | Clear after cursor (for redraw) |
| `\x1B[2K` | Erase entire line | Full line redraw |
| `\x1B[<n>G` | Cursor to column n | Position cursor precisely |
| `\x1B[<n>m` | Set text color/style | Syntax highlighting |

### Reading arrow keys

When the user presses an arrow key, the terminal sends a **three-byte sequence**:

```
Up arrow:    \x1B [ A     (bytes: 27, 91, 65)
Down arrow:  \x1B [ B     (bytes: 27, 91, 66)
Right arrow: \x1B [ C     (bytes: 27, 91, 67)
Left arrow:  \x1B [ D     (bytes: 27, 91, 68)
```

Home and End may be:
```
Home:  \x1B [ H   or   \x1B [ 1 ~
End:   \x1B [ F   or   \x1B [ 4 ~
Delete: \x1B [ 3 ~
```

Your line editor must parse these sequences to determine which key was pressed. This is where `crossterm` helps enormously — it parses escape sequences for you and returns structured `KeyEvent` values.

---

## Concept 3: Reading keystrokes with `crossterm`

`crossterm` provides a cross-platform API for reading individual keystrokes:

```rust
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

fn read_key() -> io::Result<KeyEvent> {
    loop {
        if let Event::Key(key_event) = event::read()? {
            return Ok(key_event);
        }
        // Ignore non-key events (mouse, resize, etc.)
    }
}
```

`KeyEvent` contains:
- `code: KeyCode` — which key (Char, Enter, Backspace, Left, Up, Tab, Home, etc.)
- `modifiers: KeyModifiers` — which modifiers are held (Ctrl, Alt, Shift)

Example: detecting specific keys:

```rust
match read_key()? {
    KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::NONE, .. } => {
        // Regular character typed
        insert_char(c);
    }
    KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. } => {
        // Ctrl-C
        handle_ctrl_c();
    }
    KeyEvent { code: KeyCode::Enter, .. } => {
        // Enter — submit the line
        return Ok(line);
    }
    KeyEvent { code: KeyCode::Backspace, .. } => {
        delete_char_before_cursor();
    }
    KeyEvent { code: KeyCode::Left, .. } => {
        move_cursor_left();
    }
    KeyEvent { code: KeyCode::Right, .. } => {
        move_cursor_right();
    }
    KeyEvent { code: KeyCode::Up, .. } => {
        history_prev();
    }
    KeyEvent { code: KeyCode::Down, .. } => {
        history_next();
    }
    KeyEvent { code: KeyCode::Home, .. } => {
        move_cursor_to_start();
    }
    KeyEvent { code: KeyCode::End, .. } => {
        move_cursor_to_end();
    }
    KeyEvent { code: KeyCode::Tab, .. } => {
        tab_complete();
    }
    _ => {
        // Unknown key — ignore
    }
}
```

---

## Concept 4: Building a minimal line editor

A line editor maintains a **buffer** (the current line being edited) and a **cursor position** within that buffer. Every keystroke either modifies the buffer, moves the cursor, or triggers a special action.

### State

```rust
struct LineEditor {
    buffer: Vec<char>,     // current line content
    cursor: usize,         // cursor position (0 = before first char)
    prompt: String,        // "jsh> "
    history: Vec<String>,  // command history
    history_index: usize,  // current position in history navigation
    saved_buffer: String,  // saved current line when navigating history
}
```

We use `Vec<char>` instead of `String` because:
- Inserting/deleting at arbitrary positions is `O(n)` either way, but `Vec<char>` makes indexing by character position simple
- `String` indexes by **bytes**, not characters — multi-byte UTF-8 makes cursor math painful
- For a single line of input (usually <200 chars), performance is irrelevant

### Core operations

```rust
impl LineEditor {
    fn insert_char(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
        self.redraw();
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
            self.redraw();
        }
    }

    fn delete_char_at_cursor(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
            self.redraw();
        }
    }

    fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.update_cursor_position();
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
            self.update_cursor_position();
        }
    }

    fn move_cursor_to_start(&mut self) {
        self.cursor = 0;
        self.update_cursor_position();
    }

    fn move_cursor_to_end(&mut self) {
        self.cursor = self.buffer.len();
        self.update_cursor_position();
    }

    fn delete_word_before_cursor(&mut self) {
        // Ctrl-W: delete the previous word
        if self.cursor == 0 {
            return;
        }
        // Skip trailing whitespace
        let mut end = self.cursor;
        while end > 0 && self.buffer[end - 1] == ' ' {
            end -= 1;
        }
        // Delete until the next whitespace
        let mut start = end;
        while start > 0 && self.buffer[start - 1] != ' ' {
            start -= 1;
        }
        self.buffer.drain(start..self.cursor);
        self.cursor = start;
        self.redraw();
    }

    fn clear_line(&mut self) {
        // Ctrl-U: clear from cursor to start of line
        self.buffer.drain(..self.cursor);
        self.cursor = 0;
        self.redraw();
    }

    fn clear_to_end(&mut self) {
        // Ctrl-K: clear from cursor to end of line
        self.buffer.truncate(self.cursor);
        self.redraw();
    }
}
```

### Redrawing the line

After any edit, we need to redraw the current line. The simplest approach: move to the start of the line, clear it, print the prompt and buffer, then move the cursor to the right position.

```rust
use crossterm::{
    cursor,
    terminal::{Clear, ClearType},
    execute,
};
use std::io::{self, Write};

impl LineEditor {
    fn redraw(&self) {
        let mut stdout = io::stdout();
        let line: String = self.buffer.iter().collect();
        let cursor_col = self.prompt.len() + self.cursor;

        execute!(
            stdout,
            cursor::MoveToColumn(0),                     // go to start of line
            Clear(ClearType::CurrentLine),                // erase entire line
        ).unwrap();

        // Print prompt and buffer
        print!("{}{}", self.prompt, line);
        stdout.flush().unwrap();

        // Move cursor to the correct position
        execute!(
            stdout,
            cursor::MoveToColumn(cursor_col as u16),
        ).unwrap();
    }

    fn update_cursor_position(&self) {
        let cursor_col = self.prompt.len() + self.cursor;
        execute!(
            io::stdout(),
            cursor::MoveToColumn(cursor_col as u16),
        ).unwrap();
    }
}
```

---

## Concept 5: Command history

History lets users recall and re-execute previous commands using the up and down arrow keys.

### In-memory history

```rust
impl LineEditor {
    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        if self.history_index == self.history.len() {
            // Save current buffer before navigating into history
            self.saved_buffer = self.buffer.iter().collect();
        }

        if self.history_index > 0 {
            self.history_index -= 1;
            let entry = self.history[self.history_index].clone();
            self.buffer = entry.chars().collect();
            self.cursor = self.buffer.len();
            self.redraw();
        }
    }

    fn history_next(&mut self) {
        if self.history_index >= self.history.len() {
            return; // Already at the bottom
        }

        self.history_index += 1;

        if self.history_index == self.history.len() {
            // Restore the saved buffer (what was being typed before up-arrow)
            self.buffer = self.saved_buffer.chars().collect();
        } else {
            let entry = self.history[self.history_index].clone();
            self.buffer = entry.chars().collect();
        }

        self.cursor = self.buffer.len();
        self.redraw();
    }

    fn add_to_history(&mut self, line: &str) {
        // Don't add empty lines or duplicates of the last entry
        if line.is_empty() {
            return;
        }
        if self.history.last().map(|s| s.as_str()) == Some(line) {
            return;
        }
        self.history.push(line.to_string());
    }

    fn reset_history_navigation(&mut self) {
        self.history_index = self.history.len();
        self.saved_buffer.clear();
    }
}
```

### How history navigation works

Imagine the history contains `["ls", "pwd", "echo hello"]` and the user has typed `cat` but not pressed Enter:

```
history_index = 3 (past the end)
buffer = "cat"

Press Up: history_index = 2, save "cat", buffer = "echo hello"
Press Up: history_index = 1, buffer = "pwd"
Press Up: history_index = 0, buffer = "ls"
Press Up: (already at 0, no change)
Press Down: history_index = 1, buffer = "pwd"
Press Down: history_index = 2, buffer = "echo hello"
Press Down: history_index = 3, restore "cat"
Press Down: (already at bottom, no change)
```

---

## Concept 6: Persistent history file

Command history should survive across shell sessions. We store it in a file, typically `~/.jsh_history`.

```rust
use std::fs;
use std::path::PathBuf;

fn history_file_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".jsh_history")
}

fn load_history() -> Vec<String> {
    let path = history_file_path();
    match fs::read_to_string(&path) {
        Ok(contents) => {
            contents
                .lines()
                .map(|s| s.to_string())
                .collect()
        }
        Err(_) => Vec::new(), // File doesn't exist yet — that's fine
    }
}

fn save_history(history: &[String]) {
    let path = history_file_path();
    let contents = history.join("\n");
    if let Err(e) = fs::write(&path, contents) {
        eprintln!("jsh: warning: could not save history: {}", e);
    }
}

fn append_to_history_file(line: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;

    let path = history_file_path();
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{}", line);
    }
}
```

### Load on startup, append on each command

```rust
impl Shell {
    fn new() -> Self {
        let history = load_history();
        Shell {
            line_editor: LineEditor {
                history,
                // ...
            },
            // ...
        }
    }

    fn run(&mut self) {
        loop {
            // ... prompt, read line ...

            let line = self.line_editor.read_line("jsh> ")?;

            self.line_editor.add_to_history(&line);
            append_to_history_file(&line);  // persist immediately

            // ... parse and execute ...
        }
    }
}
```

Appending immediately (rather than saving the entire history on exit) ensures history is preserved even if the shell crashes. It also means multiple shell instances do not overwrite each other's history.

### History size limit

Without a limit, the history file grows forever. Cap it:

```rust
const MAX_HISTORY_SIZE: usize = 10_000;

fn add_to_history(&mut self, line: &str) {
    if line.is_empty() {
        return;
    }
    if self.history.last().map(|s| s.as_str()) == Some(line) {
        return;
    }
    self.history.push(line.to_string());

    // Trim if over the limit
    if self.history.len() > MAX_HISTORY_SIZE {
        let excess = self.history.len() - MAX_HISTORY_SIZE;
        self.history.drain(..excess);
    }
}
```

---

## Concept 7: Tab completion

Tab completion is one of the most productivity-enhancing features of a shell. When the user presses Tab, the shell tries to complete the current word.

### What to complete

| Context | Complete with |
|---------|--------------|
| First word on line | Executable names from PATH + builtins |
| After the first word | Filenames and directories |
| After `cd ` | Only directories |
| After `$` | Environment variable names |

### Filename completion

```rust
use std::fs;
use std::path::Path;

fn complete_filename(partial: &str) -> Vec<String> {
    let (dir, prefix) = if partial.contains('/') || partial.contains('\\') {
        // partial = "src/ma" → dir = "src", prefix = "ma"
        let path = Path::new(partial);
        let dir = path.parent().unwrap_or(Path::new("."));
        let prefix = path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        (dir.to_path_buf(), prefix)
    } else {
        // partial = "Car" → dir = ".", prefix = "Car"
        (Path::new(".").to_path_buf(), partial.to_string())
    };

    let mut completions = Vec::new();

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) {
                let mut completion = if dir == Path::new(".") {
                    name.clone()
                } else {
                    format!("{}/{}", dir.display(), name)
                };

                // Append '/' for directories
                if entry.path().is_dir() {
                    completion.push('/');
                }

                completions.push(completion);
            }
        }
    }

    completions.sort();
    completions
}
```

### Command completion (for the first word)

```rust
fn complete_command(partial: &str) -> Vec<String> {
    let mut completions = Vec::new();

    // 1. Check builtins
    let builtins = ["cd", "pwd", "exit", "echo", "export",
                    "unset", "type", "jobs", "fg", "bg", "wait"];
    for builtin in &builtins {
        if builtin.starts_with(partial) {
            completions.push(builtin.to_string());
        }
    }

    // 2. Search PATH for executables
    let path_var = std::env::var("PATH").unwrap_or_default();
    let separator = if cfg!(windows) { ';' } else { ':' };

    for dir in path_var.split(separator) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();

                // On Windows, strip .exe extension for display
                let display_name = if cfg!(windows) {
                    name.strip_suffix(".exe")
                        .or_else(|| name.strip_suffix(".cmd"))
                        .or_else(|| name.strip_suffix(".bat"))
                        .unwrap_or(&name)
                        .to_string()
                } else {
                    name.clone()
                };

                if display_name.starts_with(partial) {
                    if !completions.contains(&display_name) {
                        completions.push(display_name);
                    }
                }
            }
        }
    }

    completions.sort();
    completions
}
```

### Integrating Tab into the line editor

```rust
impl LineEditor {
    fn tab_complete(&mut self) {
        // Extract the word under the cursor
        let line: String = self.buffer.iter().collect();
        let before_cursor = &line[..self.cursor_byte_pos()];

        // Find the start of the current word
        let word_start = before_cursor.rfind(' ')
            .map(|i| i + 1)
            .unwrap_or(0);
        let partial = &before_cursor[word_start..];

        // Determine if this is a command position or argument position
        let is_first_word = !before_cursor[..word_start].contains(|c: char| !c.is_whitespace());

        let completions = if is_first_word {
            complete_command(partial)
        } else {
            complete_filename(partial)
        };

        match completions.len() {
            0 => {
                // No completions — do nothing (optionally beep)
            }
            1 => {
                // Exactly one match — insert it
                let completion = &completions[0];
                let suffix = &completion[partial.len()..];

                // Insert the completion suffix
                for c in suffix.chars() {
                    self.insert_char(c);
                }
                // Add a space after if it's not a directory
                if !completion.ends_with('/') {
                    self.insert_char(' ');
                }
            }
            _ => {
                // Multiple matches — find common prefix and show options
                let common = longest_common_prefix(&completions);
                if common.len() > partial.len() {
                    // Insert the common prefix
                    let suffix = &common[partial.len()..];
                    for c in suffix.chars() {
                        self.insert_char(c);
                    }
                } else {
                    // Show all options
                    println!();
                    for comp in &completions {
                        print!("{}  ", comp);
                    }
                    println!();
                    self.redraw();
                }
            }
        }
    }
}

fn longest_common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let first = &strings[0];
    let mut prefix_len = first.len();

    for s in &strings[1..] {
        prefix_len = prefix_len.min(s.len());
        for (i, (a, b)) in first.chars().zip(s.chars()).enumerate() {
            if a != b {
                prefix_len = prefix_len.min(i);
                break;
            }
        }
    }

    first[..prefix_len].to_string()
}
```

---

## Concept 8: Syntax highlighting

Coloring the input as the user types makes the shell much more usable — valid commands appear in one color, invalid ones in another, strings are highlighted, etc.

### ANSI color codes

| Code | Color | Code | Color |
|------|-------|------|-------|
| `\x1B[30m` | Black | `\x1B[90m` | Bright Black (Gray) |
| `\x1B[31m` | Red | `\x1B[91m` | Bright Red |
| `\x1B[32m` | Green | `\x1B[92m` | Bright Green |
| `\x1B[33m` | Yellow | `\x1B[93m` | Bright Yellow |
| `\x1B[34m` | Blue | `\x1B[94m` | Bright Blue |
| `\x1B[35m` | Magenta | `\x1B[95m` | Bright Magenta |
| `\x1B[36m` | Cyan | `\x1B[96m` | Bright Cyan |
| `\x1B[37m` | White | `\x1B[97m` | Bright White |
| `\x1B[0m` | Reset (back to default) | `\x1B[1m` | Bold |
| `\x1B[4m` | Underline | `\x1B[7m` | Inverse |

### Using crossterm for colors

```rust
use crossterm::style::{Color, SetForegroundColor, ResetColor, Print};

fn highlight_line(input: &str, prompt: &str) {
    let mut stdout = io::stdout();
    let tokens = tokenize_for_highlighting(input);

    // Move to start and clear
    execute!(stdout, cursor::MoveToColumn(0), Clear(ClearType::CurrentLine)).unwrap();

    // Print prompt (no color change)
    print!("{}", prompt);

    // Print each token with appropriate color
    for token in &tokens {
        match token.kind {
            TokenKind::Command => {
                if command_exists(&token.text) {
                    // Valid command — green
                    execute!(stdout, SetForegroundColor(Color::Green)).unwrap();
                } else {
                    // Unknown command — red
                    execute!(stdout, SetForegroundColor(Color::Red)).unwrap();
                }
            }
            TokenKind::Argument => {
                // Arguments — default color
                execute!(stdout, ResetColor).unwrap();
            }
            TokenKind::String => {
                // Quoted strings — yellow
                execute!(stdout, SetForegroundColor(Color::Yellow)).unwrap();
            }
            TokenKind::Operator => {
                // |, &, >, < — magenta
                execute!(stdout, SetForegroundColor(Color::Magenta)).unwrap();
            }
            TokenKind::Variable => {
                // $VAR — cyan
                execute!(stdout, SetForegroundColor(Color::Cyan)).unwrap();
            }
            TokenKind::Whitespace => {
                execute!(stdout, ResetColor).unwrap();
            }
        }
        print!("{}", token.text);
    }

    execute!(stdout, ResetColor).unwrap();
    stdout.flush().unwrap();
}

struct HighlightToken {
    text: String,
    kind: TokenKind,
}

enum TokenKind {
    Command,
    Argument,
    String,
    Operator,
    Variable,
    Whitespace,
}

fn command_exists(cmd: &str) -> bool {
    // Check builtins
    let builtins = ["cd", "pwd", "exit", "echo", "export",
                    "unset", "type", "jobs", "fg", "bg", "wait"];
    if builtins.contains(&cmd) {
        return true;
    }
    // Check PATH
    find_in_path(cmd).is_some()
}
```

### Integrating highlighting into redraw

Replace the plain `redraw()` with a highlighted version:

```rust
impl LineEditor {
    fn redraw(&self) {
        let line: String = self.buffer.iter().collect();
        let cursor_col = self.prompt.len() + self.cursor;

        // Use highlighted rendering instead of plain print
        highlight_line(&line, &self.prompt);

        // Position cursor
        execute!(
            io::stdout(),
            cursor::MoveToColumn(cursor_col as u16),
        ).unwrap();
    }
}
```

**Performance note:** `command_exists` does a PATH lookup on every keystroke. For responsiveness, cache the PATH lookup results. A simple `HashSet<String>` populated on shell startup (and refreshed when PATH changes) is sufficient:

```rust
use std::collections::HashSet;

struct Shell {
    known_commands: HashSet<String>,
    // ...
}

impl Shell {
    fn refresh_known_commands(&mut self) {
        self.known_commands.clear();

        // Add builtins
        for builtin in &["cd", "pwd", "exit", "echo", "export",
                         "unset", "type", "jobs", "fg", "bg", "wait"] {
            self.known_commands.insert(builtin.to_string());
        }

        // Scan PATH
        let path_var = std::env::var("PATH").unwrap_or_default();
        let separator = if cfg!(windows) { ';' } else { ':' };

        for dir in path_var.split(separator) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    self.known_commands.insert(name);
                }
            }
        }
    }
}
```

---

## Concept 9: The `rustyline` alternative

Everything described above (raw mode, keystroke reading, cursor movement, history, completion, highlighting) is a significant amount of code. The `rustyline` crate provides **all of it** in a battle-tested package.

### Basic usage

```toml
# Cargo.toml
[dependencies]
rustyline = "14"
```

```rust
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

fn main() {
    let mut rl = DefaultEditor::new().expect("Failed to create editor");

    // Load history from file
    let _ = rl.load_history("~/.jsh_history");

    loop {
        match rl.readline("jsh> ") {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(line)
                    .expect("Failed to add history");

                // Parse and execute the command
                execute_line(line);
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C — print newline, show new prompt
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D — exit
                println!("Goodbye!");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    // Save history on exit
    let _ = rl.save_history("~/.jsh_history");
}
```

That is significantly less code than building a line editor from scratch. `rustyline` gives you:
- Arrow key navigation
- History with up/down arrows
- Ctrl-A, Ctrl-E, Ctrl-W, Ctrl-U, Ctrl-K (Emacs keybindings)
- Vi mode (optional)
- Tab completion (with a custom `Completer` trait)
- Syntax highlighting (with a custom `Highlighter` trait)
- Ctrl-C and Ctrl-D handling
- Persistent history

### Custom tab completion with rustyline

```rust
use rustyline::completion::{Completer, Pair};
use rustyline::Context;
use rustyline::Result;

struct ShellCompleter {
    builtins: Vec<String>,
}

impl Completer for ShellCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        let before_cursor = &line[..pos];

        // Find the start of the current word
        let word_start = before_cursor.rfind(' ')
            .map(|i| i + 1)
            .unwrap_or(0);
        let partial = &before_cursor[word_start..];

        // Determine if first word or argument
        let is_first_word = before_cursor[..word_start]
            .trim()
            .is_empty();

        let candidates = if is_first_word {
            self.complete_command(partial)
        } else {
            self.complete_filename(partial)
        };

        Ok((word_start, candidates))
    }
}

impl ShellCompleter {
    fn complete_command(&self, partial: &str) -> Vec<Pair> {
        let mut pairs = Vec::new();
        for builtin in &self.builtins {
            if builtin.starts_with(partial) {
                pairs.push(Pair {
                    display: builtin.clone(),
                    replacement: builtin.clone(),
                });
            }
        }
        // ... also search PATH ...
        pairs
    }

    fn complete_filename(&self, partial: &str) -> Vec<Pair> {
        complete_filename(partial)
            .into_iter()
            .map(|name| Pair {
                display: name.clone(),
                replacement: name,
            })
            .collect()
    }
}
```

### Custom syntax highlighting with rustyline

```rust
use rustyline::highlight::Highlighter;
use std::borrow::Cow;

struct ShellHighlighter {
    known_commands: HashSet<String>,
}

impl Highlighter for ShellHighlighter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        let tokens = tokenize_for_highlighting(line);
        let mut highlighted = String::new();

        for token in &tokens {
            match token.kind {
                TokenKind::Command => {
                    if self.known_commands.contains(&token.text) {
                        highlighted.push_str(&format!("\x1B[32m{}\x1B[0m", token.text));
                    } else {
                        highlighted.push_str(&format!("\x1B[31m{}\x1B[0m", token.text));
                    }
                }
                TokenKind::String => {
                    highlighted.push_str(&format!("\x1B[33m{}\x1B[0m", token.text));
                }
                TokenKind::Variable => {
                    highlighted.push_str(&format!("\x1B[36m{}\x1B[0m", token.text));
                }
                TokenKind::Operator => {
                    highlighted.push_str(&format!("\x1B[35m{}\x1B[0m", token.text));
                }
                _ => {
                    highlighted.push_str(&token.text);
                }
            }
        }

        Cow::Owned(highlighted)
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true // Always re-highlight (simple approach)
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        // Optionally color the prompt
        Cow::Owned(format!("\x1B[1;34m{}\x1B[0m", prompt))
    }
}
```

### Wiring it all together with rustyline

```rust
use rustyline::config::Config;
use rustyline::{Editor, CompletionType, EditMode};
use rustyline::validate::MatchingBracketValidator;

// Combine all our custom behaviors into one "Helper"
#[derive(Helper, Validator)]
struct ShellHelper {
    #[rustyline(Completer)]
    completer: ShellCompleter,
    #[rustyline(Highlighter)]
    highlighter: ShellHighlighter,
    #[rustyline(Validator)]
    validator: MatchingBracketValidator,
}

fn build_editor() -> Editor<ShellHelper> {
    let config = Config::builder()
        .completion_type(CompletionType::List)  // show list on ambiguous Tab
        .edit_mode(EditMode::Emacs)             // or EditMode::Vi
        .max_history_size(10_000)
        .unwrap()
        .build();

    let helper = ShellHelper {
        completer: ShellCompleter {
            builtins: vec!["cd", "pwd", "exit", "echo", "export",
                           "unset", "type", "jobs", "fg", "bg", "wait"]
                .into_iter().map(String::from).collect(),
        },
        highlighter: ShellHighlighter {
            known_commands: build_known_commands(),
        },
        validator: MatchingBracketValidator::new(),
    };

    let mut rl = Editor::with_config(config)
        .expect("Failed to create editor");
    rl.set_helper(Some(helper));

    rl
}
```

### When to use rustyline vs rolling your own

| Factor | Build from scratch | Use `rustyline` |
|--------|-------------------|-----------------|
| Learning value | Very high (deep terminal knowledge) | Moderate (learn the API) |
| Time to implement | Days to weeks | Hours |
| Correctness | Many edge cases to discover | Battle-tested |
| Customization | Unlimited | Limited to the trait system |
| Dependencies | `crossterm` only | `rustyline` (and its deps) |
| Binary size | Smaller | Larger |

**Recommendation for james-shell:** Start with `rustyline` to get a working shell quickly. Then, as an exercise, try replacing it with a hand-rolled line editor using `crossterm` to learn how it works under the hood. You can feature-gate both approaches:

```toml
[features]
default = ["rustyline-editor"]
rustyline-editor = ["rustyline"]
custom-editor = ["crossterm"]

[dependencies]
rustyline = { version = "14", optional = true }
crossterm = { version = "0.28", optional = true }
```

```rust
#[cfg(feature = "rustyline-editor")]
mod readline_editor;

#[cfg(feature = "custom-editor")]
mod custom_editor;
```

---

## Concept 10: The complete `read_line` flow

Here is the full flow of a single line read with our custom editor:

```
1. Enable raw mode
2. Print prompt (with optional colors)
3. Loop:
   a. Read a keystroke (crossterm::event::read)
   b. Match the key:
      - Printable char → insert into buffer at cursor position
      - Backspace      → delete char before cursor
      - Delete         → delete char at cursor
      - Left arrow     → move cursor left
      - Right arrow    → move cursor right
      - Home / Ctrl-A  → move cursor to start
      - End / Ctrl-E   → move cursor to end
      - Up arrow       → load previous history entry
      - Down arrow     → load next history entry
      - Tab            → trigger tab completion
      - Ctrl-W         → delete previous word
      - Ctrl-U         → clear line before cursor
      - Ctrl-K         → clear line after cursor
      - Ctrl-L         → clear screen, redraw prompt and line
      - Ctrl-C         → clear the buffer, return empty (or signal interrupt)
      - Ctrl-D         → if buffer empty, return EOF; else delete char at cursor
      - Enter          → break out of loop, return the buffer
   c. Redraw the line (with highlighting) and position cursor
4. Disable raw mode
5. Print a newline (since Enter in raw mode does not echo)
6. Return the line
```

The complete function:

```rust
impl LineEditor {
    fn read_line(&mut self, prompt: &str) -> io::Result<Option<String>> {
        self.prompt = prompt.to_string();
        self.buffer.clear();
        self.cursor = 0;
        self.reset_history_navigation();

        let _guard = RawModeGuard::new()?;

        // Print initial prompt
        print!("{}", prompt);
        io::stdout().flush()?;

        loop {
            let key = read_key()?;

            match key {
                KeyEvent { code: KeyCode::Enter, .. } => {
                    println!();  // newline after the line
                    let line: String = self.buffer.iter().collect();
                    return Ok(Some(line));
                }
                KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    if self.buffer.is_empty() {
                        println!();
                        return Ok(None); // EOF
                    } else {
                        self.delete_char_at_cursor();
                    }
                }
                KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    println!("^C");
                    self.buffer.clear();
                    self.cursor = 0;
                    // Start fresh on the next line
                    print!("{}", prompt);
                    io::stdout().flush()?;
                }
                KeyEvent {
                    code: KeyCode::Char('l'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    // Clear screen
                    execute!(
                        io::stdout(),
                        crossterm::terminal::Clear(ClearType::All),
                        cursor::MoveTo(0, 0),
                    )?;
                    self.redraw();
                }
                KeyEvent {
                    code: KeyCode::Char('a'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    self.move_cursor_to_start();
                }
                KeyEvent {
                    code: KeyCode::Char('e'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    self.move_cursor_to_end();
                }
                KeyEvent {
                    code: KeyCode::Char('w'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    self.delete_word_before_cursor();
                }
                KeyEvent {
                    code: KeyCode::Char('u'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    self.clear_line();
                }
                KeyEvent {
                    code: KeyCode::Char('k'),
                    modifiers: KeyModifiers::CONTROL, ..
                } => {
                    self.clear_to_end();
                }
                KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::NONE, .. }
                | KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::SHIFT, .. } => {
                    self.insert_char(c);
                }
                KeyEvent { code: KeyCode::Backspace, .. } => {
                    self.delete_char_before_cursor();
                }
                KeyEvent { code: KeyCode::Delete, .. } => {
                    self.delete_char_at_cursor();
                }
                KeyEvent { code: KeyCode::Left, .. } => {
                    self.move_cursor_left();
                }
                KeyEvent { code: KeyCode::Right, .. } => {
                    self.move_cursor_right();
                }
                KeyEvent { code: KeyCode::Up, .. } => {
                    self.history_prev();
                }
                KeyEvent { code: KeyCode::Down, .. } => {
                    self.history_next();
                }
                KeyEvent { code: KeyCode::Home, .. } => {
                    self.move_cursor_to_start();
                }
                KeyEvent { code: KeyCode::End, .. } => {
                    self.move_cursor_to_end();
                }
                KeyEvent { code: KeyCode::Tab, .. } => {
                    self.tab_complete();
                }
                _ => {
                    // Unknown key — ignore
                }
            }
        }
    }
}
```

---

## Key Rust concepts used

- **`crossterm` crate** — cross-platform terminal manipulation (raw mode, cursor, colors, key events)
- **`rustyline` crate** — full-featured line editor with completion, highlighting, and history
- **`Drop` trait** — RAII guard to ensure raw mode is always disabled, even on panic
- **`Vec<char>`** — character-indexed buffer for correct cursor math with Unicode
- **`Cow<str>`** — used by rustyline's `Highlighter` trait (avoids allocation when no highlighting needed)
- **Trait implementation** — `Completer`, `Highlighter`, `Validator` for rustyline customization
- **Feature flags** — `#[cfg(feature = "...")]` to gate between rustyline and custom editor
- **`execute!` macro** — crossterm macro for batching terminal commands efficiently
- **`std::fs::OpenOptions`** — append mode for history file persistence
- **Iterators and closures** — `filter_map`, `rfind`, chained iterator methods for completion logic

---

## Milestone

```
jsh> echo hello world
hello world
jsh> ec[Tab]                      ← completes to "echo "
jsh> echo hello
hello
jsh> [Up arrow]                   ← recalls "echo hello"
jsh> echo hello[Enter]
hello
jsh> ls sr[Tab]                   ← completes to "ls src/"
jsh> ls src/[Tab][Tab]            ← shows: main.rs
jsh> ls src/main.rs
src/main.rs
jsh> nonexistent[Enter]           ← "nonexistent" appears in red as you type
jsh: command not found: nonexistent
jsh> [Ctrl-A]                     ← cursor jumps to start of line
jsh> [Ctrl-E]                     ← cursor jumps to end of line
jsh> hello[Ctrl-W]                ← "hello" is deleted
jsh> [Ctrl-C]                     ← ^C shown, new prompt appears
jsh> [Ctrl-D]                     ← EOF
Goodbye!
$

# History persists:
$ cargo run
jsh> [Up arrow]                   ← shows last command from previous session
```

---

## What's next?

Module 11 adds **control flow and scripting** — `if`/`then`/`else`, `for`/`while` loops, `&&`/`||` operators, command substitution with `$(...)`, and function definitions. This transforms james-shell from an interactive command runner into a proper scripting language.
