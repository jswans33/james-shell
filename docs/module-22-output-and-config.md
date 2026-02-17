# Module 22: Output Formatting & Global Configuration

## What are we building?

At this point james-shell *produces* structured data (Module 14), *pipes* it
through typed commands (Module 15), and *converts* it to/from external formats
(Module 16). What it lacks is a **user-facing control layer** that lets people
decide *how* that data appears on screen and *what defaults* the shell uses
session-wide.

In this module we add:

1. **Color control** -- `--color=auto|always|never` and `NO_COLOR` support.
2. **Output display modes** -- table, JSON, CSV, compact, and raw modes
   selectable per-command or globally.
3. **Pager integration** -- automatic piping of large output through `less`.
4. **Extended shell options** -- a `shopt`-like system for feature toggles
   beyond `set -e/-x/-u`.
5. **A `config` command** -- inspect and modify settings at runtime.
6. **Continuation prompt** -- `JSH_PROMPT2` for multi-line input.
7. **Locale and encoding** -- UTF-8 handling and terminal capability detection.
8. **Default output format persistence** -- settings saved in `~/.config/jsh/config.toml`.

Together these give the user fine-grained control over james-shell's
personality without touching source code.

> **Prerequisites:** Module 12 (Concept 5: Shell Options, Concept 6: Startup
> Files), Module 14 (Display trait), Module 15 (Concept 8: Pretty-printing
> tables), Module 16 (format conversions).

---

## Concept 1: Color Control

### The problem

Module 10 introduced ANSI colors for syntax highlighting during input.
Module 15 prints tables with raw text. But there is no global toggle: if the
user redirects output to a file, colors leak through as escape codes. If the
user's terminal doesn't support color, output is garbled.

### The `ColorMode` enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// Emit colors only when stdout is a terminal (the default).
    Auto,
    /// Always emit ANSI color codes, even when piped.
    Always,
    /// Never emit color codes.
    Never,
}

impl ColorMode {
    /// Resolve to a concrete yes/no based on the current environment.
    pub fn should_colorize(&self) -> bool {
        match self {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => atty::is(atty::Stream::Stdout),
        }
    }
}
```

### Setting color mode

Three ways, in precedence order (highest wins):

| Source | Syntax |
|--------|--------|
| CLI flag | `jsh --color=never` |
| Environment variable | `NO_COLOR=1` (any non-empty value) |
| Config file | `color_mode = "auto"` in `~/.config/jsh/config.toml` |
| Default | `Auto` |

```rust
use std::env;

pub fn resolve_color_mode(cli_flag: Option<&str>) -> ColorMode {
    // 1. Explicit CLI flag takes priority.
    if let Some(flag) = cli_flag {
        return match flag {
            "always" => ColorMode::Always,
            "never" => ColorMode::Never,
            _ => ColorMode::Auto,
        };
    }

    // 2. NO_COLOR convention (https://no-color.org/).
    if env::var("NO_COLOR").map_or(false, |v| !v.is_empty()) {
        return ColorMode::Never;
    }

    // 3. JSH-specific env var.
    if let Ok(val) = env::var("JSH_COLOR") {
        return match val.as_str() {
            "always" => ColorMode::Always,
            "never" => ColorMode::Never,
            _ => ColorMode::Auto,
        };
    }

    // 4. Fall through to config file or default.
    ColorMode::Auto
}
```

### Wiring it into the shell

Add `color_mode` to `ShellEnv` so every output path can check it:

```rust
pub struct ShellEnv {
    pub color_mode: ColorMode,
    // ... existing fields from Module 12
}
```

Then wrap all ANSI emission behind a helper:

```rust
use std::fmt;

/// Write text with an ANSI style, respecting the shell's color mode.
pub fn styled_write(
    f: &mut fmt::Formatter<'_>,
    env: &ShellEnv,
    ansi_code: &str,
    text: &str,
) -> fmt::Result {
    if env.color_mode.should_colorize() {
        write!(f, "\x1B[{}m{}\x1B[0m", ansi_code, text)
    } else {
        write!(f, "{}", text)
    }
}
```

Every place that currently emits raw `\x1B[` sequences (Module 10 syntax
highlighting, the prompt renderer from Module 12 Concept 2, table headers)
should call through `styled_write` or check `color_mode.should_colorize()`
first.

---

## Concept 2: Output Display Modes

### The problem

`ls | sort-by size` currently *always* renders as a bordered table. But
sometimes the user wants JSON for piping into `jq`, CSV for a spreadsheet,
or a compact single-line format for scripting.

### The `DisplayMode` enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Bordered, aligned table (Module 15 Concept 8). Default for interactive use.
    Table,
    /// One record per line, "key: value" pairs.
    Compact,
    /// JSON (pretty-printed by default).
    Json,
    /// Comma-separated values.
    Csv,
    /// Raw `to_string()` -- no table chrome, no colors.
    Raw,
}
```

### Per-command override with `| to`

The existing `to json`, `to csv` commands from Module 16 already handle
explicit conversion. The display mode controls the **implicit** conversion
that happens when the pipeline result reaches the screen:

```
jsh> ls                             # uses the global default (Table)
jsh> ls | to json                   # explicit override, always JSON
jsh> config set display_mode json   # change the default globally
jsh> ls                             # now renders as JSON
```

### Auto-detection

When `DisplayMode` is not set explicitly, the shell picks a sensible default:

```rust
pub fn auto_display_mode(value: &Value, interactive: bool) -> DisplayMode {
    if !interactive {
        // Scripts always get raw output for easy parsing.
        return DisplayMode::Raw;
    }

    match value {
        Value::Table { rows, .. } if !rows.is_empty() => DisplayMode::Table,
        Value::Record(_) => DisplayMode::Table,
        Value::List(items) if items.iter().all(|v| matches!(v, Value::Record(_))) => {
            // A list of records looks best as a table.
            DisplayMode::Table
        }
        _ => DisplayMode::Raw,
    }
}
```

### The render function

```rust
pub fn render_output(
    value: &Value,
    mode: DisplayMode,
    color: bool,
) -> String {
    match mode {
        DisplayMode::Table => render_table(value, color),
        DisplayMode::Compact => render_compact(value),
        DisplayMode::Json => render_json(value, /* pretty */ true),
        DisplayMode::Csv => render_csv(value),
        DisplayMode::Raw => value.to_string(),
    }
}

fn render_table(value: &Value, color: bool) -> String {
    // Delegate to the Module 15 table formatter, with optional color
    // for headers and borders.
    let mut buf = String::new();
    match value {
        Value::Table { columns, rows } => {
            if color {
                // Bold header row.
                buf.push_str("\x1B[1m");
            }
            // ... (use format_table from Module 15, injecting color resets
            //      after the header line)
            format_table_to_string(columns, rows, color, &mut buf);
        }
        Value::List(items) => {
            // Attempt to promote to a table.
            if let Some((cols, rows)) = try_promote_list(items) {
                return render_table(
                    &Value::Table { columns: cols, rows },
                    color,
                );
            }
            // Fall back to one item per line.
            for item in items {
                buf.push_str(&item.to_string());
                buf.push('\n');
            }
        }
        other => buf.push_str(&other.to_string()),
    }
    buf
}

fn render_compact(value: &Value) -> String {
    match value {
        Value::Table { columns, rows } => {
            rows.iter()
                .enumerate()
                .map(|(i, row)| {
                    let fields: Vec<String> = columns.iter()
                        .filter_map(|c| row.get(c).map(|v| format!("{}: {}", c, v)))
                        .collect();
                    format!("#{} {{{}}}", i, fields.join(", "))
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Value::Record(map) => {
            map.iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join(", ")
        }
        other => other.to_string(),
    }
}

fn render_json(value: &Value, pretty: bool) -> String {
    // Reuse the JSON serializer from Module 16.
    if pretty {
        serde_json::to_string_pretty(&value_to_json(value)).unwrap_or_default()
    } else {
        serde_json::to_string(&value_to_json(value)).unwrap_or_default()
    }
}

fn render_csv(value: &Value) -> String {
    // Reuse the CSV serializer from Module 16.
    value_to_csv(value)
}
```

---

## Concept 3: Pager Integration

### The problem

`ls` in a directory with 500 files dumps 500 rows and the top scrolls off
screen. Traditional shells rely on the user to pipe through `less`. We can do
this automatically.

### When to page

Paging activates when **all** of these are true:

1. stdout is a terminal (`atty::is(atty::Stream::Stdout)`).
2. The rendered output exceeds the terminal height.
3. The user has not disabled it (`config set pager none`).
4. A pager program is available.

### Pager resolution

```rust
use std::env;

pub fn resolve_pager() -> Option<String> {
    // 1. Shell-specific override.
    if let Ok(p) = env::var("JSH_PAGER") {
        if !p.is_empty() {
            return Some(p);
        }
    }

    // 2. Standard PAGER variable.
    if let Ok(p) = env::var("PAGER") {
        if !p.is_empty() {
            return Some(p);
        }
    }

    // 3. Sensible default.
    Some("less -FIRX".to_string())
}
```

The flags for `less`:
- `-F` -- quit immediately if the output fits on one screen.
- `-I` -- case-insensitive search.
- `-R` -- pass through ANSI color codes.
- `-X` -- don't clear the screen on exit.

### Piping to the pager

```rust
use std::io::Write;
use std::process::{Command, Stdio};

pub fn page_output(text: &str, pager_cmd: &str) -> std::io::Result<()> {
    let parts: Vec<&str> = pager_cmd.split_whitespace().collect();
    let (program, args) = parts.split_first()
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty pager command",
        ))?;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        // Write in chunks to avoid blocking on large output.
        stdin.write_all(text.as_bytes())?;
    }

    child.wait()?;
    Ok(())
}
```

### Wiring it into the output path

At the very end of the REPL loop (Module 1), after `render_output` produces a
string:

```rust
fn display_pipeline_result(value: &Value, env: &ShellEnv) {
    if matches!(value, Value::Nothing) {
        return;
    }

    let color = env.color_mode.should_colorize();
    let mode = env.display_mode.unwrap_or_else(|| auto_display_mode(value, true));
    let text = render_output(value, mode, color);

    let lines = text.lines().count();
    let term_height = terminal_height().unwrap_or(24);

    let use_pager = env.pager_enabled
        && atty::is(atty::Stream::Stdout)
        && lines > term_height;

    if use_pager {
        if let Some(pager) = resolve_pager() {
            if let Err(e) = page_output(&text, &pager) {
                eprintln!("jsh: pager failed: {}", e);
                print!("{}", text);
            }
        } else {
            print!("{}", text);
        }
    } else {
        print!("{}", text);
    }
}
```

---

## Concept 4: Extended Shell Options (`shopt`)

### The problem

Module 12 Concept 5 gives us `set -e/-x/-u/-o pipefail` -- four boolean
switches that control execution behavior. But there are many more preferences
that don't fit the POSIX `set` model: should globs match dotfiles? Should `cd`
correct typos? Should the shell auto-list completions?

Bash solves this with `shopt`. We do the same, but with a cleaner naming
convention.

### The `ShellOption` registry

```rust
use std::collections::BTreeMap;

/// Extended shell options, toggled via `shopt`.
#[derive(Debug, Clone)]
pub struct ShoptRegistry {
    options: BTreeMap<String, ShoptEntry>,
}

#[derive(Debug, Clone)]
struct ShoptEntry {
    value: bool,
    description: &'static str,
}

impl ShoptRegistry {
    pub fn new() -> Self {
        let mut reg = ShoptRegistry {
            options: BTreeMap::new(),
        };

        // ── Globbing ───────────────────────────────────────────
        reg.register("dotglob", false,
            "Include dotfiles in glob expansion");
        reg.register("globstar", false,
            "Enable ** recursive glob patterns");
        reg.register("nullglob", false,
            "Expand non-matching globs to nothing instead of the literal pattern");
        reg.register("failglob", false,
            "Error on non-matching globs instead of passing them through");

        // ── Directory navigation ───────────────────────────────
        reg.register("autocd", false,
            "Treat a bare directory name as 'cd <dir>'");
        reg.register("cdspell", false,
            "Auto-correct minor typos in cd arguments");

        // ── Completion ─────────────────────────────────────────
        reg.register("auto_list", true,
            "Automatically list completions on ambiguous input");
        reg.register("complete_aliases", false,
            "Expand aliases during tab completion");

        // ── History ────────────────────────────────────────────
        reg.register("histappend", true,
            "Append to history file rather than overwriting");
        reg.register("histverify", false,
            "Let user edit a recalled history entry before executing");

        // ── Output ─────────────────────────────────────────────
        reg.register("color_header", true,
            "Colorize table headers in output");
        reg.register("row_numbers", true,
            "Show row numbers in table output");
        reg.register("thousands_sep", false,
            "Use thousands separators in large numbers (e.g. 1,234,567)");

        // ── Safety ─────────────────────────────────────────────
        reg.register("interactive_comments", true,
            "Allow # comments in interactive mode");
        reg.register("exec_confirm", false,
            "Prompt before exec replaces the shell process");

        reg
    }

    fn register(&mut self, name: &str, default: bool, desc: &'static str) {
        self.options.insert(name.to_string(), ShoptEntry {
            value: default,
            description: desc,
        });
    }

    pub fn get(&self, name: &str) -> Option<bool> {
        self.options.get(name).map(|e| e.value)
    }

    pub fn set(&mut self, name: &str, value: bool) -> Result<(), String> {
        match self.options.get_mut(name) {
            Some(entry) => {
                entry.value = value;
                Ok(())
            }
            None => Err(format!("shopt: {}: unknown option", name)),
        }
    }

    pub fn list_all(&self) -> Vec<(&str, bool, &str)> {
        self.options
            .iter()
            .map(|(k, e)| (k.as_str(), e.value, e.description))
            .collect()
    }
}
```

### The `shopt` builtin

```rust
pub fn builtin_shopt(args: &[&str], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Print all options and their current state.
        for (name, value, description) in env.shopt.list_all() {
            let state = if value { "on " } else { "off" };
            println!("  {} {} -- {}", state, name, description);
        }
        return 0;
    }

    match args[0] {
        "-s" => {
            // Enable options: shopt -s dotglob globstar
            for name in &args[1..] {
                if let Err(e) = env.shopt.set(name, true) {
                    eprintln!("{}", e);
                    return 1;
                }
            }
            0
        }
        "-u" => {
            // Disable options: shopt -u dotglob
            for name in &args[1..] {
                if let Err(e) = env.shopt.set(name, false) {
                    eprintln!("{}", e);
                    return 1;
                }
            }
            0
        }
        "-q" => {
            // Query: exit 0 if all named options are on, 1 otherwise.
            for name in &args[1..] {
                match env.shopt.get(name) {
                    Some(true) => {}
                    Some(false) => return 1,
                    None => {
                        eprintln!("shopt: {}: unknown option", name);
                        return 2;
                    }
                }
            }
            0
        }
        name => {
            // Toggle shorthand: `shopt dotglob` prints its state.
            match env.shopt.get(name) {
                Some(val) => {
                    let state = if val { "on" } else { "off" };
                    println!("{}: {}", name, state);
                    0
                }
                None => {
                    eprintln!("shopt: {}: unknown option", name);
                    2
                }
            }
        }
    }
}
```

### Integration with `ShellEnv`

```rust
pub struct ShellEnv {
    pub options: ShellOptions,   // set -e/-x/-u (Module 12)
    pub shopt: ShoptRegistry,    // Extended options (this module)
    pub color_mode: ColorMode,   // Concept 1
    pub display_mode: Option<DisplayMode>, // Concept 2, None = auto
    pub pager_enabled: bool,     // Concept 3
    // ... other fields
}
```

---

## Concept 5: The `config` Command

### The problem

`set` and `shopt` modify runtime behavior but don't persist across sessions.
The `.jshrc` startup file (Module 12 Concept 6) handles persistence, but
editing a file by hand is clunky. We want a command that reads, writes, and
persists settings.

### Config file location

Following the XDG Base Directory Specification:

```rust
use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(dir).join("jsh")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".config").join("jsh")
    } else {
        PathBuf::from(".config").join("jsh")
    }
}

pub fn config_file_path() -> PathBuf {
    config_dir().join("config.toml")
}
```

### Config file format

```toml
# ~/.config/jsh/config.toml

[display]
color_mode = "auto"         # "auto", "always", "never"
display_mode = "auto"       # "auto", "table", "json", "csv", "compact", "raw"
pager = "less -FIRX"        # pager command, or "none" to disable
row_numbers = true
color_header = true
thousands_sep = false

[prompt]
prompt = "\\e[32m\\u@\\h\\e[0m:\\e[34m\\w\\e[0m\\g\\$ "
prompt2 = "> "              # continuation prompt

[history]
max_entries = 10000
append = true
ignore_dups = true
ignore_space = true         # lines starting with space are not recorded

[completion]
auto_list = true
complete_aliases = false

[globbing]
dotglob = false
globstar = false
nullglob = false
```

### Loading the config

```rust
use std::fs;
use toml;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
pub struct ShellConfig {
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub prompt: PromptConfig,
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub completion: CompletionConfig,
    #[serde(default)]
    pub globbing: GlobConfig,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DisplayConfig {
    #[serde(default = "default_color_mode")]
    pub color_mode: String,
    #[serde(default = "default_display_mode")]
    pub display_mode: String,
    #[serde(default = "default_pager")]
    pub pager: String,
    #[serde(default = "default_true")]
    pub row_numbers: bool,
    #[serde(default = "default_true")]
    pub color_header: bool,
    #[serde(default)]
    pub thousands_sep: bool,
}

fn default_color_mode() -> String { "auto".into() }
fn default_display_mode() -> String { "auto".into() }
fn default_pager() -> String { "less -FIRX".into() }
fn default_true() -> bool { true }

impl ShellConfig {
    pub fn load() -> Self {
        let path = config_file_path();
        match fs::read_to_string(&path) {
            Ok(contents) => {
                toml::from_str(&contents).unwrap_or_else(|e| {
                    eprintln!(
                        "jsh: warning: bad config at {}: {}",
                        path.display(), e
                    );
                    ShellConfig::default()
                })
            }
            Err(_) => ShellConfig::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_file_path();
        let dir = config_dir();

        fs::create_dir_all(&dir).map_err(|e| {
            format!("could not create {}: {}", dir.display(), e)
        })?;

        let toml_str = toml::to_string_pretty(self).map_err(|e| {
            format!("could not serialize config: {}", e)
        })?;

        fs::write(&path, toml_str).map_err(|e| {
            format!("could not write {}: {}", path.display(), e)
        })
    }
}
```

### The `config` builtin

```rust
pub fn builtin_config(args: &[&str], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        print_usage();
        return 0;
    }

    match args[0] {
        "get" => {
            // config get display.color_mode
            if args.len() < 2 {
                eprintln!("config get: expected a key");
                return 1;
            }
            match config_get(&env.config, args[1]) {
                Some(val) => { println!("{}", val); 0 }
                None => {
                    eprintln!("config get: unknown key: {}", args[1]);
                    1
                }
            }
        }

        "set" => {
            // config set display.color_mode always
            if args.len() < 3 {
                eprintln!("config set: expected a key and value");
                return 1;
            }
            match config_set(&mut env.config, args[1], args[2]) {
                Ok(()) => {
                    // Apply the change immediately.
                    apply_config(env);
                    0
                }
                Err(e) => {
                    eprintln!("config set: {}", e);
                    1
                }
            }
        }

        "save" => {
            match env.config.save() {
                Ok(()) => {
                    println!("Config saved to {}", config_file_path().display());
                    0
                }
                Err(e) => {
                    eprintln!("config save: {}", e);
                    1
                }
            }
        }

        "reset" => {
            env.config = ShellConfig::default();
            apply_config(env);
            println!("Config reset to defaults");
            0
        }

        "list" => {
            // Pretty-print the entire config as TOML.
            let toml_str = toml::to_string_pretty(&env.config)
                .unwrap_or_else(|_| "(serialization error)".into());
            println!("{}", toml_str);
            0
        }

        "path" => {
            println!("{}", config_file_path().display());
            0
        }

        other => {
            eprintln!("config: unknown subcommand: {}", other);
            1
        }
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  config list                 Show all settings");
    eprintln!("  config get <key>            Get a setting value");
    eprintln!("  config set <key> <value>    Change a setting");
    eprintln!("  config save                 Persist settings to disk");
    eprintln!("  config reset                Restore defaults");
    eprintln!("  config path                 Print config file location");
}

/// Apply the in-memory config to the live ShellEnv state.
fn apply_config(env: &mut ShellEnv) {
    // Color mode.
    env.color_mode = match env.config.display.color_mode.as_str() {
        "always" => ColorMode::Always,
        "never" => ColorMode::Never,
        _ => ColorMode::Auto,
    };

    // Display mode.
    env.display_mode = match env.config.display.display_mode.as_str() {
        "table" => Some(DisplayMode::Table),
        "json" => Some(DisplayMode::Json),
        "csv" => Some(DisplayMode::Csv),
        "compact" => Some(DisplayMode::Compact),
        "raw" => Some(DisplayMode::Raw),
        _ => None, // "auto"
    };

    // Pager.
    env.pager_enabled = env.config.display.pager != "none";

    // Sync shopt entries from config.
    let _ = env.shopt.set("row_numbers", env.config.display.row_numbers);
    let _ = env.shopt.set("color_header", env.config.display.color_header);
    let _ = env.shopt.set("thousands_sep", env.config.display.thousands_sep);
    let _ = env.shopt.set("dotglob", env.config.globbing.dotglob);
    let _ = env.shopt.set("globstar", env.config.globbing.globstar);
    let _ = env.shopt.set("nullglob", env.config.globbing.nullglob);
    let _ = env.shopt.set("auto_list", env.config.completion.auto_list);
    let _ = env.shopt.set("complete_aliases", env.config.completion.complete_aliases);
}
```

---

## Concept 6: Continuation Prompt (`JSH_PROMPT2`)

### The problem

When a user types an incomplete command (open quote, trailing `|`, `\`
continuation), the REPL needs a secondary prompt to signal "I'm waiting for
more input." Module 12 Concept 2 only defines the primary prompt
(`JSH_PROMPT`).

### Implementation

```rust
pub struct ShellEnv {
    // Primary prompt (Module 12).
    pub prompt: String,
    // Continuation prompt (new).
    pub prompt2: String,
    // ... other fields
}

impl ShellEnv {
    pub fn new() -> Self {
        ShellEnv {
            prompt: r"\e[32m\u@\h\e[0m:\e[34m\w\e[0m\g\$ ".to_string(),
            prompt2: "> ".to_string(),
            // ...
        }
    }
}
```

In the REPL loop (Module 1), when `is_incomplete()` returns true:

```rust
loop {
    let prompt = render_prompt(&env.prompt, &env);
    let mut input = readline(&prompt)?;

    while is_incomplete(&input) {
        let cont_prompt = render_prompt(&env.prompt2, &env);
        let more = readline(&cont_prompt)?;
        input.push('\n');
        input.push_str(&more);
    }

    // ... tokenize, parse, execute
}
```

The continuation prompt supports the same escape sequences as the primary
prompt (Module 12 Concept 2: `\u`, `\h`, `\w`, etc.), so users can colorize
it:

```bash
# ~/.jshrc
let JSH_PROMPT2 = "\e[33m... \e[0m"
```

---

## Concept 7: Locale and Encoding

### Terminal capability detection

```rust
pub struct TerminalCaps {
    /// Terminal supports 256 colors.
    pub color_256: bool,
    /// Terminal supports 24-bit true color.
    pub true_color: bool,
    /// Terminal supports Unicode (including emoji).
    pub unicode: bool,
    /// Number of columns.
    pub width: usize,
    /// Number of rows.
    pub height: usize,
}

impl TerminalCaps {
    pub fn detect() -> Self {
        let term = std::env::var("TERM").unwrap_or_default();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();

        let color_256 = term.contains("256color")
            || colorterm.contains("256color");

        let true_color = colorterm == "truecolor"
            || colorterm == "24bit";

        // Check locale for UTF-8 support.
        let lang = std::env::var("LANG").unwrap_or_default();
        let lc_all = std::env::var("LC_ALL").unwrap_or_default();
        let unicode = lc_all.contains("UTF-8")
            || lc_all.contains("utf-8")
            || lang.contains("UTF-8")
            || lang.contains("utf-8");

        let (width, height) = (terminal_width(), terminal_height().unwrap_or(24));

        TerminalCaps {
            color_256,
            true_color,
            unicode,
            width,
            height,
        }
    }
}
```

### Unicode-aware column width

When computing table column widths (Module 15 Concept 8), a naive `len()`
counts bytes, not display columns. East Asian characters are double-width, and
ANSI escapes are zero-width. Use the `unicode-width` crate:

```rust
use unicode_width::UnicodeWidthStr;

fn display_width(s: &str) -> usize {
    // Strip ANSI escape sequences before measuring.
    let stripped = strip_ansi_escapes(s);
    UnicodeWidthStr::width(stripped.as_str())
}

fn strip_ansi_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;

    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if ch == '\x1B' {
            in_escape = true;
        } else {
            result.push(ch);
        }
    }

    result
}
```

Replace every `cell_text.len()` in `format_table` (Module 15) with
`display_width(&cell_text)`.

---

## Concept 8: Putting It All Together -- Startup Sequence

When james-shell launches, the full initialization order is:

```
1.  Parse CLI flags (--color, --log-level, -c, etc.)
2.  Detect terminal capabilities (Concept 7)
3.  Load config file (~/.config/jsh/config.toml) (Concept 5)
4.  Merge CLI flags over config (CLI wins)
5.  Initialize ShellEnv with merged settings
6.  Initialize ShoptRegistry with config values (Concept 4)
7.  Set color mode (Concept 1)
8.  Resolve pager (Concept 3)
9.  Load history file (Module 10)
10. Source startup files: /etc/jshrc, ~/.jshrc, $JSH_ENV (Module 12)
11. Display welcome message (if interactive and not suppressed)
12. Enter REPL loop (Module 1)
```

```rust
pub fn main() {
    let cli = parse_cli_args();

    // Steps 2-8: build the environment.
    let caps = TerminalCaps::detect();
    let mut config = ShellConfig::load();

    // CLI overrides.
    if let Some(color) = cli.color {
        config.display.color_mode = color;
    }

    let mut env = ShellEnv::new_from_config(config, caps);

    // Steps 9-10: history and startup files.
    if cli.interactive {
        load_history(&mut env);
        load_startup_files(&mut env, true);
    }

    // Step 11-12: run.
    if let Some(command) = cli.command {
        // Non-interactive: jsh -c "command"
        run_command(&command, &mut env);
    } else if let Some(script) = cli.script {
        // Script mode: jsh script.jsh
        run_script(&script, &mut env);
    } else {
        // Interactive REPL.
        repl_loop(&mut env);
    }
}
```

---

## Concept 9: CLI Flags Reference

The full set of command-line flags, collecting options from this module and
others:

| Flag | Short | Description |
|------|-------|-------------|
| `--color <mode>` | | Set color mode: `auto`, `always`, `never` |
| `--display <mode>` | | Default display mode: `auto`, `table`, `json`, `csv`, `compact`, `raw` |
| `--no-pager` | | Disable automatic paging |
| `--log-level <level>` | `-v` | Set log level (Module 21) |
| `--log-file <path>` | | Write logs to file (Module 21) |
| `--no-rc` | | Skip loading startup files |
| `--config <path>` | | Use alternate config file |
| `-c <command>` | | Execute a command string and exit |
| `-e` | | Enable `errexit` |
| `-x` | | Enable `xtrace` |
| `-u` | | Enable `nounset` |

```rust
pub struct CliArgs {
    pub interactive: bool,
    pub command: Option<String>,
    pub script: Option<String>,
    pub color: Option<String>,
    pub display_mode: Option<String>,
    pub no_pager: bool,
    pub no_rc: bool,
    pub config_path: Option<String>,
    pub log_level: Option<String>,
    pub log_file: Option<String>,
    pub errexit: bool,
    pub xtrace: bool,
    pub nounset: bool,
}
```

---

## Quick Reference

### Color control

```
jsh --color=never               # CLI flag
NO_COLOR=1 jsh                  # Environment variable
config set display.color_mode never   # Runtime + persistable
```

### Display mode

```
jsh> config set display.display_mode json
jsh> ls                          # Now renders as JSON
jsh> ls | to csv                 # Per-command override still works
jsh> config set display.display_mode auto
```

### Pager

```
jsh> config set display.pager "less -FIRX"
jsh> config set display.pager none       # disable
jsh> JSH_PAGER=bat jsh                   # use bat instead
```

### Extended options

```
jsh> shopt                       # list all
jsh> shopt -s dotglob globstar   # enable
jsh> shopt -u dotglob            # disable
jsh> shopt -q globstar && echo "globstar is on"
```

### Config management

```
jsh> config list                 # show everything
jsh> config get display.pager    # read one key
jsh> config set history.max_entries 50000
jsh> config save                 # persist to disk
jsh> config reset                # restore defaults
jsh> config path                 # show file location
```

---

## Exercises

1. **Colorize table headers.** Modify `format_table` from Module 15 to bold
   the header row when `shopt color_header` is on and color mode allows it.

2. **Implement `--color=auto` detection.** Add the `atty` crate to your
   `Cargo.toml` and wire `resolve_color_mode` into `ShellEnv` initialization.
   Verify that `ls | cat` produces no ANSI escapes but `ls` in a terminal does.

3. **Add `shopt autocd`.** When enabled and the user types a bare directory
   name that is not a command, treat it as `cd <dir>`. You will need to hook
   into the "command not found" path in the executor (Module 3).

4. **Build config persistence.** Implement `config set` + `config save` so
   that `config set display.color_mode never && config save` persists across
   restarts. Verify by restarting the shell.

5. **Pager integration.** Wire up `page_output` and verify that `ls` in a
   directory with 200+ files automatically pages, and that `ls | grep foo`
   does **not** page (stdout is not a terminal in that case).

6. **Unicode table widths.** Add the `unicode-width` crate and replace
   `cell_text.len()` with `display_width()` in the table formatter. Test with
   CJK filenames and emoji.

7. **Continuation prompt.** Implement `JSH_PROMPT2` so that typing `echo "hello`
   (missing close quote) shows `> ` on the next line. Make it customizable
   in `config.toml` and via `let JSH_PROMPT2 = "..."`.

---

> **See also:**
> - Module 10 (Line Editing) -- ANSI color codes and syntax highlighting.
> - Module 12 Concept 2 (Prompt Customization) -- the primary `JSH_PROMPT`.
> - Module 12 Concept 5 (Shell Options) -- `set -e/-x/-u/-o pipefail`.
> - Module 12 Concept 6 (Startup Files) -- `.jshrc` load order.
> - Module 15 Concept 8 (Pretty-printing tables) -- the table renderer this
>   module extends.
> - Module 16 (Data Parsers) -- `to json`, `to csv` format converters.
> - Module 21 (Diagnostics) -- `JSH_LOG` and `--log-level`.
