# james-shell Crate Reference Guide

> Comprehensive reference for every external crate used across the 20 modules of james-shell.
> Version recommendations are current as of February 2026.

---

## Table of Contents

- [Core (Modules 1-7)](#core-modules-1-7)
  - [ctrlc](#1-ctrlc)
  - [glob](#2-glob)
- [Systems (Modules 8-10)](#systems-modules-8-10)
  - [nix](#3-nix)
  - [signal-hook](#4-signal-hook)
  - [crossterm](#5-crossterm)
  - [rustyline](#6-rustyline)
- [Utilities (Modules 5-12)](#utilities-modules-5-12)
  - [dirs](#7-dirs)
  - [shellexpand](#8-shellexpand)
  - [which](#9-which)
- [Testing (Module 13)](#testing-module-13)
  - [assert_cmd](#10-assert_cmd)
  - [predicates](#11-predicates)
  - [criterion](#12-criterion)
- [Beyond Bash (Modules 14-20)](#beyond-bash-modules-14-20)
  - [serde + serde_json](#13-serde--serde_json)
  - [serde_yaml_ng](#14-serde_yaml_ng)
  - [toml](#15-toml)
  - [csv](#16-csv)
  - [ureq](#17-ureq)
  - [wasmtime](#18-wasmtime)
- [Development Tools](#development-tools)
  - [cargo clippy](#cargo-clippy)
  - [cargo fmt](#cargo-fmt)
  - [cargo audit](#cargo-audit)
  - [cargo fuzz](#cargo-fuzz)
  - [cargo watch](#cargo-watch)

---

## Core (Modules 1-7)

### 1. ctrlc

**Version:** `3.4`

**What it does:**
Provides a simple, cross-platform way to set a handler for Ctrl-C (SIGINT) events. Works on both Unix and Windows without platform-specific code.

**Used in:** Module 1 (REPL Loop), Module 8 (Job Control), Module 9 (Signals)

**Cargo.toml:**
```toml
[dependencies]
ctrlc = "3.4"
```

**Minimal Example:**
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl-C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    while running.load(Ordering::SeqCst) {
        // REPL loop body
    }
}
```

**Key Types/Functions:**
- `ctrlc::set_handler(FnMut())` -- registers the Ctrl-C handler (can only be called once)
- `ctrlc::Error` -- error type for handler registration failures

**Why this over alternatives:**
`ctrlc` is the simplest option for basic Ctrl-C handling. Unlike `signal-hook`, which provides comprehensive signal management, `ctrlc` has zero configuration and a single function call API. We use it for the REPL's immediate interrupt needs and graduate to `signal-hook` when we need full signal control in Modules 8-9.

---

### 2. glob

**Version:** `0.3`

**What it does:**
Matches file paths against Unix-style glob patterns (`*`, `?`, `[...]`). Returns an iterator of matching `PathBuf` entries from the filesystem.

**Used in:** Module 5 (Expansion), Module 12 (Advanced Features)

**Cargo.toml:**
```toml
[dependencies]
glob = "0.3"
```

**Minimal Example:**
```rust
use glob::glob;

fn main() {
    // Expand *.rs to all Rust files in current directory
    for entry in glob("src/**/*.rs").expect("Invalid glob pattern") {
        match entry {
            Ok(path) => println!("Found: {}", path.display()),
            Err(e) => eprintln!("Glob error: {}", e),
        }
    }
}
```

**Key Types/Functions:**
- `glob::glob(pattern) -> Result<Paths>` -- match files against a glob pattern
- `glob::Pattern` -- a compiled glob pattern for reuse
- `glob::Pattern::matches(str)` -- test if a string matches without filesystem access
- `glob::MatchOptions` -- control case sensitivity and dot-file behavior

**Why this over alternatives:**
`glob` is part of the Rust nursery (maintained by the Rust team) and closely mirrors POSIX shell globbing behavior, making it ideal for a shell. `globset` from the ripgrep family is faster for bulk matching but adds unnecessary complexity for file expansion. `fast-glob` optimizes for speed at the cost of a less standard API.

---

## Systems (Modules 8-10)

### 3. nix

**Version:** `0.29` (Unix-only)

**What it does:**
Provides safe Rust wrappers around POSIX/Unix system calls including `fork`, `exec`, `pipe`, `waitpid`, signals, and terminal control (`termios`). Turns raw libc calls into type-safe Rust APIs.

**Used in:** Module 3 (Execution), Module 6 (Redirection), Module 7 (Pipes), Module 8 (Job Control), Module 9 (Signals), Module 10 (Line Editing)

**Cargo.toml:**
```toml
[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["process", "signal", "term", "fs"] }
```

**Minimal Example:**
```rust
use nix::unistd::{fork, ForkResult, execvp};
use nix::sys::wait::waitpid;
use std::ffi::CString;

fn main() {
    match unsafe { fork() }.expect("Fork failed") {
        ForkResult::Parent { child } => {
            let status = waitpid(child, None).expect("waitpid failed");
            println!("Child exited with: {:?}", status);
        }
        ForkResult::Child => {
            let cmd = CString::new("ls").unwrap();
            let args = [CString::new("ls").unwrap(), CString::new("-la").unwrap()];
            execvp(&cmd, &args).expect("execvp failed");
        }
    }
}
```

**Key Types/Functions:**
- `nix::unistd::fork()` -- create a child process
- `nix::unistd::execvp()` -- replace process with new program
- `nix::unistd::pipe()` -- create a Unix pipe
- `nix::unistd::dup2()` -- duplicate file descriptors (for redirection)
- `nix::unistd::setpgid()` / `nix::unistd::tcsetpgrp()` -- process group control
- `nix::sys::wait::waitpid()` -- wait for child process status
- `nix::sys::signal::kill()` -- send signals to processes
- `nix::sys::termios` -- terminal attribute control (raw mode, echo)

**Why this over alternatives:**
`nix` is the de facto standard for Unix syscalls in Rust. Using raw `libc` bindings is unsafe and error-prone; `nix` wraps every call in safe Rust types with proper error handling. No other crate provides this breadth of POSIX coverage. The feature-flag system keeps binary size down by only compiling what you use.

---

### 4. signal-hook

**Version:** `0.3`

**What it does:**
Provides ergonomic, composable signal handling for Unix systems. Supports registering multiple handlers for the same signal, iterator-based signal consumption, and flag-based signal checking -- all without undefined behavior.

**Used in:** Module 8 (Job Control), Module 9 (Signals)

**Cargo.toml:**
```toml
[dependencies]
signal-hook = "0.3"
```

**Minimal Example:**
```rust
use signal_hook::consts::{SIGINT, SIGTSTP, SIGCHLD};
use signal_hook::iterator::Signals;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut signals = Signals::new([SIGINT, SIGTSTP, SIGCHLD])?;

    // Process signals in a loop (typically in a dedicated thread)
    for signal in signals.forever() {
        match signal {
            SIGINT  => println!("Interrupt received"),
            SIGTSTP => println!("Suspend received"),
            SIGCHLD => println!("Child process state changed"),
            _       => unreachable!(),
        }
    }

    Ok(())
}
```

**Key Types/Functions:**
- `signal_hook::iterator::Signals::new(signals)` -- create a signal iterator
- `signal_hook::iterator::Signals::forever()` -- blocking iterator over received signals
- `signal_hook::flag::register(signal, Arc<AtomicBool>)` -- set a flag on signal receipt
- `signal_hook::consts::*` -- signal constants (SIGINT, SIGTERM, SIGCHLD, etc.)
- `signal_hook::low_level::register(signal, FnMut())` -- low-level handler registration

**Why this over alternatives:**
`ctrlc` only handles SIGINT. `nix` can set signal handlers but the API is low-level and unsafe. `signal-hook` supports the full range of Unix signals with a safe, composable API. Its iterator-based approach is particularly well-suited for a shell's event loop where you need to respond to SIGCHLD (child state change), SIGTSTP (suspend), and SIGINT (interrupt) simultaneously.

---

### 5. crossterm

**Version:** `0.28`

**What it does:**
Cross-platform terminal manipulation library that handles raw mode, cursor movement, styled/colored output, screen clearing, and terminal event reading. Works on Windows, macOS, and Linux without conditional compilation.

**Used in:** Module 10 (Line Editing), Module 17 (Tab Completion UI)

**Cargo.toml:**
```toml
[dependencies]
crossterm = "0.28"
```

**Minimal Example:**
```rust
use crossterm::{
    execute,
    style::{Color, Print, SetForegroundColor, ResetColor},
    terminal::{enable_raw_mode, disable_raw_mode},
    cursor::MoveTo,
    event::{read, Event, KeyCode},
};
use std::io::stdout;

fn main() -> crossterm::Result<()> {
    enable_raw_mode()?;

    execute!(stdout(), MoveTo(0, 0), SetForegroundColor(Color::Green),
             Print("james-shell> "), ResetColor)?;

    loop {
        if let Event::Key(key_event) = read()? {
            match key_event.code {
                KeyCode::Char('q') => break,
                KeyCode::Char(c) => execute!(stdout(), Print(c))?,
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    Ok(())
}
```

**Key Types/Functions:**
- `crossterm::terminal::{enable_raw_mode, disable_raw_mode}` -- toggle raw terminal mode
- `crossterm::event::{read, poll, Event, KeyCode, KeyEvent}` -- read keyboard/mouse events
- `crossterm::cursor::{MoveTo, MoveLeft, MoveRight, position}` -- cursor positioning
- `crossterm::style::{Print, SetForegroundColor, Color, Stylize}` -- colored output
- `crossterm::terminal::{Clear, ClearType}` -- screen/line clearing
- `execute!()` / `queue!()` -- macros to send terminal commands

**Why this over alternatives:**
`termion` is Unix-only, ruling it out for Windows support. `console` lacks raw mode and event reading. `crossterm` is the only crate that provides full terminal control across all three major platforms. It is also the terminal backend for `ratatui` (the standard TUI framework), so it is well-maintained and battle-tested.

---

### 6. rustyline

**Version:** `15.0`

**What it does:**
A readline/linenoise replacement for Rust that provides line editing with Emacs and Vi keybindings, persistent history, programmable tab completion, syntax highlighting hooks, and multi-line input. Handles all the complexity of interactive line input.

**Used in:** Module 1 (REPL Loop), Module 10 (Line Editing), Module 17 (Tab Completion)

**Cargo.toml:**
```toml
[dependencies]
rustyline = "15.0"
```

**Minimal Example:**
```rust
use rustyline::{DefaultEditor, Result};
use rustyline::error::ReadlineError;

fn main() -> Result<()> {
    let mut rl = DefaultEditor::new()?;
    rl.load_history("history.txt").ok(); // ignore if no history yet

    loop {
        match rl.readline("james> ") {
            Ok(line) => {
                rl.add_history_entry(&line)?;
                println!("Executing: {}", line);
            }
            Err(ReadlineError::Interrupted) => println!("Ctrl-C"),
            Err(ReadlineError::Eof) => break,
            Err(err) => { eprintln!("Error: {:?}", err); break; }
        }
    }

    rl.save_history("history.txt")?;
    Ok(())
}
```

**Key Types/Functions:**
- `rustyline::DefaultEditor` -- editor with default configuration
- `rustyline::Editor<H, I>` -- configurable editor with custom helper and history
- `rustyline::error::ReadlineError` -- error enum (Interrupted, Eof, etc.)
- `rustyline::hint::Hinter` -- trait for inline suggestions
- `rustyline::completion::Completer` -- trait for tab completion
- `rustyline::highlight::Highlighter` -- trait for syntax highlighting
- `rustyline::validate::Validator` -- trait for input validation (multi-line)
- `rustyline::Config` -- configuration builder (edit mode, history size, etc.)

**Why this over alternatives:**
`linefeed` is less actively maintained. `liner` has fewer features. `rustyline` is the most feature-complete readline alternative in Rust with an extensible trait system for completion, hinting, highlighting, and validation. Its `Helper` trait lets us plug in shell-specific behavior (command completion, path completion, syntax highlighting) cleanly.

---

## Utilities (Modules 5-12)

### 7. dirs

**Version:** `6.0`

**What it does:**
Returns platform-specific paths for standard directories (home, config, cache, data, etc.). Uses XDG base directories on Linux, Known Folder API on Windows, and standard directories on macOS.

**Used in:** Module 5 (Expansion), Module 10 (Line Editing -- history file location), Module 11 (Scripting -- config file location)

**Cargo.toml:**
```toml
[dependencies]
dirs = "6.0"
```

**Minimal Example:**
```rust
use std::path::PathBuf;

fn main() {
    if let Some(home) = dirs::home_dir() {
        println!("Home directory: {}", home.display());
    }
    if let Some(config) = dirs::config_dir() {
        let shell_config = config.join("james-shell");
        println!("Config directory: {}", shell_config.display());
    }
    if let Some(data) = dirs::data_local_dir() {
        let history = data.join("james-shell").join("history.txt");
        println!("History file: {}", history.display());
    }
}
```

**Key Types/Functions:**
- `dirs::home_dir() -> Option<PathBuf>` -- user's home directory
- `dirs::config_dir() -> Option<PathBuf>` -- OS-appropriate config directory
- `dirs::data_local_dir() -> Option<PathBuf>` -- local application data
- `dirs::cache_dir() -> Option<PathBuf>` -- cache directory
- `dirs::executable_dir() -> Option<PathBuf>` -- user-specific executable directory

**Why this over alternatives:**
`directories` (from the same author) provides more structure via `ProjectDirs` and `UserDirs` but adds complexity we do not need. `home` only provides the home directory. `dirs` is the sweet spot: lightweight, cross-platform, and provides all the standard directories a shell needs for config, history, and cache file placement.

---

### 8. shellexpand

**Version:** `3.1`

**What it does:**
Performs shell-style tilde expansion (`~` to home directory) and environment variable expansion (`$VAR`, `${VAR}`) in strings. Returns a `Cow<str>` to avoid allocation when no expansion is needed.

**Used in:** Module 5 (Expansion), Module 11 (Scripting)

**Cargo.toml:**
```toml
[dependencies]
shellexpand = "3.1"
```

**Minimal Example:**
```rust
fn main() {
    // Tilde expansion: ~ -> /home/user
    let expanded = shellexpand::tilde("~/documents/notes.txt");
    println!("Tilde: {}", expanded);

    // Environment variable expansion
    std::env::set_var("PROJECT", "james-shell");
    let expanded = shellexpand::env("$HOME/src/$PROJECT").unwrap();
    println!("Env: {}", expanded);

    // Full expansion: tilde + env vars combined
    let expanded = shellexpand::full("~/$PROJECT/config").unwrap();
    println!("Full: {}", expanded);
}
```

**Key Types/Functions:**
- `shellexpand::tilde(input) -> Cow<str>` -- expand `~` and `~user`
- `shellexpand::env(input) -> Result<Cow<str>>` -- expand `$VAR` and `${VAR}`
- `shellexpand::full(input) -> Result<Cow<str>>` -- tilde + env expansion combined
- `shellexpand::tilde_with_context(input, home_fn)` -- custom home directory lookup
- `shellexpand::env_with_context(input, env_fn)` -- custom variable lookup function

**Why this over alternatives:**
No other crate provides both tilde and variable expansion in one package. Writing this by hand is error-prone (handling `${VAR:-default}`, `~user`, and edge cases). The `_with_context` variants let us plug in our shell's own variable store rather than relying solely on environment variables, which is critical for shell-local variables.

---

### 9. which

**Version:** `7.0`

**What it does:**
Locates an executable by name by searching the system PATH, mimicking the behavior of the Unix `which` command. Works cross-platform on Linux, macOS, and Windows.

**Used in:** Module 3 (Execution), Module 5 (Expansion), Module 17 (Tab Completion)

**Cargo.toml:**
```toml
[dependencies]
which = "7.0"
```

**Minimal Example:**
```rust
use which::which;
use std::path::Path;

fn main() {
    // Find a single executable
    match which("git") {
        Ok(path) => println!("git is at: {}", path.display()),
        Err(_)   => println!("git not found in PATH"),
    }

    // Check if a command exists before trying to run it
    let cmd = "cargo";
    if which(cmd).is_ok() {
        println!("{} is available", cmd);
    }

    // Find all matching executables (not just the first)
    for path in which::which_all("python").unwrap() {
        println!("Found python at: {}", path.display());
    }
}
```

**Key Types/Functions:**
- `which::which(name) -> Result<PathBuf>` -- find first matching executable
- `which::which_all(name) -> Result<impl Iterator<Item=PathBuf>>` -- find all matches
- `which::which_in(name, paths, cwd) -> Result<PathBuf>` -- search custom PATH
- `which::which_global(name) -> Result<PathBuf>` -- skip cwd, search PATH only
- `which::Error` -- error type (CannotFindBinaryPath, CannotCanonicalize)

**Why this over alternatives:**
`pathsearch` is less maintained and has fewer features. Rolling our own PATH search means handling platform differences (`;` vs `:` separators, `.exe` extensions on Windows, PATHEXT). `which` handles all of this correctly and is the standard solution used by `cargo` itself.

---

## Testing (Module 13)

### 10. assert_cmd

**Version:** `2.0`

**What it does:**
Simplifies integration testing of CLI applications by wrapping `std::process::Command` with ergonomic assertion methods. Lets you spawn your binary, feed it stdin, and assert on stdout, stderr, and exit codes.

**Used in:** Module 13 (Testing)

**Cargo.toml:**
```toml
[dev-dependencies]
assert_cmd = "2.0"
```

**Minimal Example:**
```rust
use assert_cmd::Command;

#[test]
fn test_echo_builtin() {
    let mut cmd = Command::cargo_bin("james-shell").unwrap();
    cmd.args(["-c", "echo hello world"])
       .assert()
       .success()
       .stdout("hello world\n");
}

#[test]
fn test_invalid_command() {
    let mut cmd = Command::cargo_bin("james-shell").unwrap();
    cmd.args(["-c", "nonexistent_command"])
       .assert()
       .failure()
       .stderr(predicates::str::contains("not found"));
}
```

**Key Types/Functions:**
- `assert_cmd::Command::cargo_bin(name)` -- build and locate your binary
- `.assert()` -- convert output to an `Assert` for chained assertions
- `.success()` / `.failure()` -- assert on exit code
- `.stdout(predicate)` / `.stderr(predicate)` -- assert on output content
- `.write_stdin(data)` -- provide stdin input to the process

**Why this over alternatives:**
Raw `std::process::Command` works but requires manual output capture and comparison boilerplate. `assert_cmd` reduces a 15-line test to 5 lines. It integrates seamlessly with the `predicates` crate for flexible matching and automatically finds your binary via `cargo_bin`. It is the standard tool for CLI testing in the Rust ecosystem.

---

### 11. predicates

**Version:** `3.1`

**What it does:**
Provides composable boolean predicate functions for use in assertions. Supports string matching, regex, file existence, numeric comparisons, and logical combinators (and, or, not). Designed to pair with `assert_cmd`.

**Used in:** Module 13 (Testing)

**Cargo.toml:**
```toml
[dev-dependencies]
predicates = "3.1"
```

**Minimal Example:**
```rust
use predicates::prelude::*;
use assert_cmd::Command;

#[test]
fn test_prompt_output() {
    let pred = predicate::str::contains("james>")
        .and(predicate::str::contains("$").not());

    let mut cmd = Command::cargo_bin("james-shell").unwrap();
    cmd.write_stdin("exit\n")
       .assert()
       .success()
       .stdout(pred);
}

#[test]
fn test_error_message() {
    let pred = predicate::str::is_match(r"error:.*not found").unwrap();

    let mut cmd = Command::cargo_bin("james-shell").unwrap();
    cmd.args(["-c", "fakecmd"])
       .assert()
       .stderr(pred);
}
```

**Key Types/Functions:**
- `predicate::str::contains(s)` -- substring match
- `predicate::str::is_match(regex)` -- regex match
- `predicate::str::starts_with(s)` / `ends_with(s)` -- prefix/suffix match
- `predicate::str::is_empty()` -- empty string check
- `predicate::eq(value)` -- exact equality
- `.and()` / `.or()` / `.not()` -- logical combinators on any predicate
- `predicate::path::exists()` -- file existence check

**Why this over alternatives:**
`predicates` is the companion crate to `assert_cmd` and provides a unified API for all assertion types. Without it, you would write ad-hoc string comparisons scattered across tests. The composable `.and()` / `.or()` / `.not()` API reads like plain English, making test intent clear. No other assertion crate integrates as cleanly with `assert_cmd`.

---

### 12. criterion

**Version:** `0.5`

**What it does:**
A statistics-driven benchmarking framework that provides reliable, reproducible performance measurements. Generates HTML reports with graphs, detects performance regressions, and handles warm-up, outlier detection, and confidence intervals automatically.

**Used in:** Module 13 (Testing -- performance benchmarks)

**Cargo.toml:**
```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "shell_benchmarks"
harness = false
```

**Minimal Example:**
```rust
// benches/shell_benchmarks.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn parse_command(input: &str) -> Vec<&str> {
    input.split_whitespace().collect()
}

fn bench_parser(c: &mut Criterion) {
    c.bench_function("parse simple command", |b| {
        b.iter(|| parse_command(black_box("ls -la /home/user")))
    });

    c.bench_function("parse pipeline", |b| {
        b.iter(|| parse_command(black_box("cat file.txt | grep pattern | wc -l")))
    });
}

criterion_group!(benches, bench_parser);
criterion_main!(benches);
```

**Key Types/Functions:**
- `Criterion` -- benchmark configuration and runner
- `criterion_group!()` -- macro to group benchmark functions
- `criterion_main!()` -- macro to generate benchmark main function
- `Criterion::bench_function(name, closure)` -- benchmark a single function
- `Criterion::bench_with_input(name, input, closure)` -- benchmark with parameterized input
- `black_box(value)` -- prevent compiler from optimizing away benchmark code
- `BenchmarkGroup` -- group related benchmarks for comparison

**Why this over alternatives:**
The built-in `#[bench]` attribute is nightly-only and produces unreliable results. `criterion` works on stable Rust and uses rigorous statistical methods (linear regression, bootstrap resampling) to detect real performance changes. Its HTML reports make it easy to track performance over time. `divan` is a newer alternative with a simpler API, but `criterion` has the larger ecosystem and more documentation.

---

## Beyond Bash (Modules 14-20)

### 13. serde + serde_json

**Versions:** `serde = "1.0"`, `serde_json = "1.0"`

**What they do:**
`serde` is Rust's standard serialization framework -- it defines traits for converting between Rust types and data formats. `serde_json` implements those traits for JSON, providing parsing, serialization, and a dynamic `Value` type for working with JSON of unknown shape.

**Used in:** Module 14 (Structured Types), Module 15 (Typed Pipelines), Module 16 (Data Parsers), Module 18 (Error Handling)

**Cargo.toml:**
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

**Minimal Example:**
```rust
use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug)]
struct ShellConfig {
    prompt: String,
    history_size: usize,
    aliases: Vec<(String, String)>,
}

fn main() -> serde_json::Result<()> {
    // Typed: deserialize into a struct
    let json = r#"{"prompt":"james> ","history_size":1000,"aliases":[]}"#;
    let config: ShellConfig = serde_json::from_str(json)?;
    println!("Prompt: {}", config.prompt);

    // Dynamic: work with unknown JSON shapes
    let data: Value = serde_json::from_str(r#"{"users": [1, 2, 3]}"#)?;
    println!("First user: {}", data["users"][0]);

    // Serialize back to JSON
    let output = serde_json::to_string_pretty(&config)?;
    println!("{}", output);
    Ok(())
}
```

**Key Types/Functions:**
- `#[derive(Serialize, Deserialize)]` -- auto-generate serialization code
- `serde_json::from_str(s)` / `from_reader(r)` -- parse JSON from string or reader
- `serde_json::to_string(v)` / `to_string_pretty(v)` -- serialize to JSON string
- `serde_json::Value` -- dynamic JSON type (Object, Array, String, Number, Bool, Null)
- `serde_json::json!()` -- macro for building JSON inline
- `serde_json::Map<String, Value>` -- JSON object type

**Why this over alternatives:**
`serde` is the undisputed standard for serialization in Rust -- virtually every data format crate builds on it. Using `serde` means our types work with JSON, YAML, TOML, and CSV using the same derive macros. `simd-json` is faster for parsing but adds SIMD dependencies and complexity. For a shell that processes configuration and pipeline data, `serde_json`'s correctness and ergonomics matter more than raw parsing speed.

---

### 14. serde_yaml_ng

**Version:** `0.10`

**What it does:**
Provides YAML serialization and deserialization using the serde framework. This is the actively maintained successor to the deprecated `serde_yaml` crate, offering the same API with continued bug fixes and updates.

**Used in:** Module 16 (Data Parsers), Module 14 (Structured Types)

**Cargo.toml:**
```toml
[dependencies]
serde_yaml_ng = "0.10"
```

**Note:** The original `serde_yaml` crate (by dtolnay) was deprecated in March 2024 and is no longer maintained. `serde_yaml_ng` is an independent continuation that preserves the API while remaining actively maintained. Another option is `serde_yml`, though it has received some community criticism regarding code quality.

**Minimal Example:**
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct ServerConfig {
    host: String,
    port: u16,
    features: Vec<String>,
}

fn main() -> Result<(), serde_yaml_ng::Error> {
    let yaml = r#"
        host: localhost
        port: 8080
        features:
          - logging
          - metrics
    "#;

    let config: ServerConfig = serde_yaml_ng::from_str(yaml)?;
    println!("Server: {}:{}", config.host, config.port);

    // Serialize back to YAML
    let output = serde_yaml_ng::to_string(&config)?;
    println!("{}", output);
    Ok(())
}
```

**Key Types/Functions:**
- `serde_yaml_ng::from_str(s)` / `from_reader(r)` -- parse YAML
- `serde_yaml_ng::to_string(v)` / `to_writer(w, v)` -- serialize to YAML
- `serde_yaml_ng::Value` -- dynamic YAML value type
- `serde_yaml_ng::Mapping` -- YAML mapping (key-value pairs)
- `serde_yaml_ng::Error` -- error type for parse/serialize failures

**Why this over alternatives:**
With `serde_yaml` deprecated, the Rust YAML ecosystem is in flux. `serde_yaml_ng` is a straightforward continuation by an independent maintainer with a clean fork. `yaml-rust2` provides a lower-level YAML parser but lacks serde integration. Using `serde_yaml_ng` means our code stays compatible with the well-known `serde_yaml` API and we can swap to a future standard when one emerges.

---

### 15. toml

**Version:** `0.8`

**What it does:**
A native Rust encoder and decoder for TOML-formatted files, with full serde integration. Handles TOML's type system (strings, integers, floats, booleans, datetimes, arrays, tables) and provides both typed deserialization and a dynamic `Value` type.

**Used in:** Module 16 (Data Parsers), Module 11 (Scripting -- shell config files)

**Cargo.toml:**
```toml
[dependencies]
toml = "0.8"
```

**Minimal Example:**
```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
struct ShellConfig {
    prompt: String,
    history_file: String,
    #[serde(default)]
    aliases: HashMap<String, String>,
}

fn main() -> Result<(), toml::de::Error> {
    let toml_str = r#"
        prompt = "james> "
        history_file = "~/.james_history"

        [aliases]
        ll = "ls -la"
        gs = "git status"
    "#;

    let config: ShellConfig = toml::from_str(toml_str)?;
    println!("Prompt: {}", config.prompt);
    for (alias, cmd) in &config.aliases {
        println!("  {} -> {}", alias, cmd);
    }
    Ok(())
}
```

**Key Types/Functions:**
- `toml::from_str(s) -> Result<T>` -- deserialize TOML string into a typed struct
- `toml::to_string(v)` / `to_string_pretty(v)` -- serialize to TOML string
- `toml::Value` -- dynamic TOML value type
- `toml::Table` -- TOML table (key-value mapping)
- `toml::de::Error` -- deserialization error with span information

**Why this over alternatives:**
`toml` is the canonical TOML crate in Rust (used by Cargo itself, via toml_edit). `toml_edit` preserves formatting and comments but adds complexity unnecessary for reading config files. `basic-toml` is a stripped-down parser that lacks serialization. For reading and writing shell configuration files, `toml` provides the right balance of features and simplicity.

---

### 16. csv

**Version:** `1.3`

**What it does:**
A fast, flexible CSV reader and writer with serde integration. Handles quoting, escaping, headers, different delimiters, and streaming record-by-record processing without loading entire files into memory.

**Used in:** Module 16 (Data Parsers), Module 15 (Typed Pipelines)

**Cargo.toml:**
```toml
[dependencies]
csv = "1.3"
```

**Minimal Example:**
```rust
use csv::ReaderBuilder;
use serde::Deserialize;
use std::io::Cursor;

#[derive(Debug, Deserialize)]
struct Record {
    name: String,
    age: u32,
    city: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = "name,age,city\nAlice,30,NYC\nBob,25,LA\n";
    let mut reader = ReaderBuilder::new().from_reader(Cursor::new(data));

    for result in reader.deserialize::<Record>() {
        let record = result?;
        println!("{}: age {}, from {}", record.name, record.age, record.city);
    }
    Ok(())
}
```

**Key Types/Functions:**
- `csv::Reader::from_path(path)` / `from_reader(r)` -- create a CSV reader
- `csv::ReaderBuilder::new()` -- configure delimiter, quoting, headers, etc.
- `reader.records()` -- iterate over rows as `StringRecord`
- `reader.deserialize::<T>()` -- iterate over rows as typed structs (via serde)
- `csv::Writer::from_writer(w)` -- create a CSV writer
- `writer.serialize(record)` -- write a typed struct as a CSV row
- `csv::StringRecord` -- a single row of string fields

**Why this over alternatives:**
`csv` (by BurntSushi, the ripgrep author) is the standard CSV crate in Rust. It handles edge cases (embedded newlines, quote escaping, BOM markers) correctly and streams data efficiently. Hand-parsing CSV with `split(',')` breaks on quoted fields. No other CSV crate matches its combination of correctness, performance, and serde integration.

---

### 17. ureq

**Version:** `3.0`

**What it does:**
A lightweight, blocking HTTP client with a simple API. Supports HTTPS (via rustls), JSON request/response bodies, cookies, proxies, and timeouts -- all without an async runtime. Perfect for a synchronous shell's `fetch` command.

**Used in:** Module 19 (Modern Scripting -- fetch/HTTP command), Module 20 (Plugins -- downloading)

**Cargo.toml:**
```toml
[dependencies]
ureq = { version = "3.0", features = ["json"] }
```

**Minimal Example:**
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Simple GET request
    let body: String = ureq::get("https://httpbin.org/get")
        .call()?
        .body_mut()
        .read_to_string()?;
    println!("Response: {}", &body[..100]);

    // GET with JSON deserialization
    let resp: serde_json::Value = ureq::get("https://api.github.com/repos/rust-lang/rust")
        .header("User-Agent", "james-shell/0.1")
        .call()?
        .body_mut()
        .read_json()?;
    println!("Stars: {}", resp["stargazers_count"]);

    // POST with JSON body
    let payload = serde_json::json!({"key": "value"});
    ureq::post("https://httpbin.org/post")
        .send_json(&payload)?;
    Ok(())
}
```

**Key Types/Functions:**
- `ureq::get(url)` / `post(url)` / `put(url)` / `delete(url)` -- build requests
- `.header(name, value)` -- set request headers
- `.query(key, value)` -- add query parameters
- `.call()` -- send request, get `Response`
- `.send_json(value)` -- send JSON body (POST/PUT)
- `response.body_mut().read_to_string()` -- read response body as string
- `response.body_mut().read_json::<T>()` -- deserialize response as JSON
- `response.status()` -- HTTP status code

**Why this over alternatives:**
`reqwest` is the most popular HTTP client but requires the `tokio` async runtime, adding significant dependency weight. Our shell is synchronous, so `ureq`'s blocking API is a natural fit. `attohttpc` is similar but less actively maintained. `curl` bindings exist but require the C library. `ureq` keeps the dependency tree small (uses `rustls` for TLS, no OpenSSL needed) and provides an idiomatic Rust API.

---

### 18. wasmtime

**Version:** `29.0`

**What it does:**
A fast, secure, standards-compliant WebAssembly runtime that can execute `.wasm` modules from Rust. Provides sandboxed execution, memory isolation, and a host function API for exposing shell capabilities to WASM plugins.

**Used in:** Module 20 (Plugins -- WASM plugin system)

**Cargo.toml:**
```toml
[dependencies]
wasmtime = "29.0"
```

**Minimal Example:**
```rust
use wasmtime::*;

fn main() -> wasmtime::Result<()> {
    // Create the engine and store
    let engine = Engine::default();
    let mut store = Store::new(&engine, ());

    // Load a WASM module (compiled from any language targeting WASM)
    let module = Module::from_file(&engine, "plugin.wasm")?;

    // Define host functions the plugin can call
    let log_func = Func::wrap(&mut store, |caller: Caller<'_, ()>, msg_ptr: i32| {
        println!("Plugin says: (message at ptr {})", msg_ptr);
    });

    // Instantiate the module with imports
    let instance = Instance::new(&mut store, &module, &[log_func.into()])?;

    // Call an exported function
    let run = instance.get_typed_func::<(), ()>(&mut store, "run")?;
    run.call(&mut store, ())?;

    Ok(())
}
```

**Key Types/Functions:**
- `wasmtime::Engine` -- compilation engine (shared across modules)
- `wasmtime::Store<T>` -- runtime state container with user-defined host data
- `wasmtime::Module::from_file()` / `from_binary()` -- load and compile WASM
- `wasmtime::Instance::new(store, module, imports)` -- instantiate a module
- `wasmtime::Func::wrap()` -- create host functions callable from WASM
- `wasmtime::Linker<T>` -- simplifies linking multiple imports
- `instance.get_typed_func()` -- get a typed handle to an exported function
- `wasmtime::Memory` -- access to the module's linear memory

**Why this over alternatives:**
`wasmer` is the main competitor but has historically had more breaking changes and less focus on standards compliance. `wasmtime` is developed by the Bytecode Alliance (Mozilla, Fastly, Intel) and is the reference implementation for WASI. Its security model (verified cranelift compilation, sandboxed memory) makes it suitable for running untrusted plugin code. For a shell plugin system where security matters, `wasmtime` is the safest choice.

---

## Development Tools

These are not library dependencies but essential cargo subcommands for development workflow.

### cargo clippy

**What it does:**
An official Rust linter that catches common mistakes, suggests idiomatic improvements, and enforces best practices. Goes far beyond what the compiler warns about.

**Installation:**
```bash
# Included with rustup by default
rustup component add clippy
```

**Usage:**
```bash
# Run clippy on the project
cargo clippy

# Treat all warnings as errors (useful in CI)
cargo clippy -- -D warnings

# Fix auto-fixable lints
cargo clippy --fix

# Check all targets including tests and benchmarks
cargo clippy --all-targets --all-features
```

**Key lint categories for a shell project:**
- `clippy::unwrap_used` -- find panic-prone unwrap calls (shells must not crash)
- `clippy::todo` -- find leftover TODO markers
- `clippy::perf` -- performance-related suggestions
- `clippy::pedantic` -- stricter code quality lints

**Configuration (in `Cargo.toml` or `.clippy.toml`):**
```toml
# In Cargo.toml
[lints.clippy]
unwrap_used = "warn"
expect_used = "warn"
pedantic = { level = "warn", priority = -1 }
```

---

### cargo fmt

**What it does:**
The official Rust code formatter (rustfmt). Enforces a consistent code style across the entire project automatically. Eliminates style debates in code review.

**Installation:**
```bash
# Included with rustup by default
rustup component add rustfmt
```

**Usage:**
```bash
# Format all files in the project
cargo fmt

# Check formatting without modifying files (useful in CI)
cargo fmt -- --check

# Format a specific file
rustfmt src/main.rs
```

**Configuration (`rustfmt.toml` in project root):**
```toml
edition = "2021"
max_width = 100
tab_spaces = 4
use_small_heuristics = "Max"
imports_granularity = "Crate"
group_imports = "StdExternalCrate"
```

---

### cargo audit

**What it does:**
Audits `Cargo.lock` against the RustSec Advisory Database to find dependencies with known security vulnerabilities. Essential for a shell that will execute on users' systems.

**Installation:**
```bash
cargo install cargo-audit
```

**Usage:**
```bash
# Audit all dependencies for known vulnerabilities
cargo audit

# Generate a JSON report
cargo audit --json

# Fix vulnerabilities by updating dependencies (where possible)
cargo audit fix

# Audit and check for yanked crates too
cargo audit --deny yanked
```

**CI Integration:**
```yaml
# In GitHub Actions
- name: Security audit
  run: |
    cargo install cargo-audit
    cargo audit
```

---

### cargo fuzz

**What it does:**
Coverage-guided fuzzing for Rust using libFuzzer. Generates random inputs to find crashes, panics, and undefined behavior in your code. Critical for a shell's parser and expansion engine where malformed input is common.

**Installation:**
```bash
cargo install cargo-fuzz
```

**Usage:**
```bash
# Initialize fuzzing targets directory
cargo fuzz init

# Create a new fuzz target
cargo fuzz add fuzz_parser

# Run the fuzzer (requires nightly Rust)
cargo +nightly fuzz run fuzz_parser

# Run for a limited time (e.g., 60 seconds)
cargo +nightly fuzz run fuzz_parser -- -max_total_time=60

# List existing fuzz targets
cargo fuzz list
```

**Example fuzz target (`fuzz/fuzz_targets/fuzz_parser.rs`):**
```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // Feed random strings to the parser
    // Panics and crashes will be caught and reported
    let _ = james_shell::parser::parse(data);
});
```

**What to fuzz in james-shell:**
- Command parser (Module 2) -- malformed input, unclosed quotes, deeply nested structures
- Glob expansion (Module 5) -- pathological patterns, deeply nested directories
- Variable expansion (Module 5) -- recursive variables, deeply nested `${}`
- Script parser (Module 11) -- malformed scripts, unexpected EOF

---

### cargo watch

**What it does:**
Watches your source files for changes and automatically re-runs a cargo command. Dramatically speeds up the edit-compile-test cycle during development.

**Installation:**
```bash
cargo install cargo-watch
```

**Usage:**
```bash
# Re-run tests on every file change
cargo watch -x test

# Re-run a specific test
cargo watch -x "test test_parser"

# Run clippy on every change
cargo watch -x clippy

# Chain commands: check, then test, then clippy
cargo watch -x check -x test -x clippy

# Clear screen before each run
cargo watch -c -x test

# Only watch specific directories
cargo watch -w src -w tests -x test

# Run the shell binary on every change (for quick iteration)
cargo watch -x run

# Ignore specific files
cargo watch --ignore "*.log" -x test
```

**Recommended development workflow:**
```bash
# Terminal 1: continuous testing
cargo watch -c -x "test -- --nocapture"

# Terminal 2: continuous linting
cargo watch -c -x clippy
```

---

## Consolidated Cargo.toml

Here is the complete dependency section for reference. You will not need all crates from day one -- add them as you reach each module.

```toml
[dependencies]
# Core (Modules 1-7)
ctrlc = "3.4"
glob = "0.3"

# Systems (Modules 8-10) -- nix is Unix-only
signal-hook = "0.3"
crossterm = "0.28"
rustyline = "15.0"

[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["process", "signal", "term", "fs"] }

[dependencies]
# Utilities (Modules 5-12)
dirs = "6.0"
shellexpand = "3.1"
which = "7.0"

# Beyond Bash (Modules 14-20)
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml_ng = "0.10"
toml = "0.8"
csv = "1.3"
ureq = { version = "3.0", features = ["json"] }
wasmtime = "29.0"

[dev-dependencies]
# Testing (Module 13)
assert_cmd = "2.0"
predicates = "3.1"
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "shell_benchmarks"
harness = false
```

---

## Version Pinning Strategy

We use **caret (default) version requirements** throughout (e.g., `"1.0"` means `>=1.0.0, <2.0.0`). This gives us automatic patch and minor updates while protecting against breaking changes. For maximum reproducibility:

1. **Always commit `Cargo.lock`** -- since james-shell is a binary (not a library), the lock file should be version-controlled.
2. **Run `cargo update` periodically** -- pick up security patches and bug fixes.
3. **Run `cargo audit` before releases** -- catch known vulnerabilities.
4. **Pin exact versions only if a specific bug requires it** -- e.g., `crossterm = "=0.28.1"`.

---

## Platform Compatibility Matrix

| Crate          | Linux | macOS | Windows | Notes                              |
|----------------|:-----:|:-----:|:-------:|-------------------------------------|
| ctrlc          |   Y   |   Y   |    Y    | Cross-platform                      |
| glob           |   Y   |   Y   |    Y    | Cross-platform                      |
| nix            |   Y   |   Y   |    N    | Unix-only; use cfg(unix) gate       |
| signal-hook    |   Y   |   Y   |    P    | Partial Windows (SIGINT only)       |
| crossterm      |   Y   |   Y   |    Y    | Cross-platform                      |
| rustyline      |   Y   |   Y   |    Y    | Cross-platform                      |
| dirs           |   Y   |   Y   |    Y    | Uses platform-native APIs           |
| shellexpand    |   Y   |   Y   |    Y    | Cross-platform                      |
| which          |   Y   |   Y   |    Y    | Handles PATHEXT on Windows          |
| assert_cmd     |   Y   |   Y   |    Y    | Cross-platform                      |
| predicates     |   Y   |   Y   |    Y    | Cross-platform                      |
| criterion      |   Y   |   Y   |    Y    | Cross-platform                      |
| serde          |   Y   |   Y   |    Y    | Cross-platform                      |
| serde_json     |   Y   |   Y   |    Y    | Cross-platform                      |
| serde_yaml_ng  |   Y   |   Y   |    Y    | Cross-platform                      |
| toml           |   Y   |   Y   |    Y    | Cross-platform                      |
| csv            |   Y   |   Y   |    Y    | Cross-platform                      |
| ureq           |   Y   |   Y   |    Y    | Uses rustls (no OpenSSL needed)     |
| wasmtime       |   Y   |   Y   |    Y    | Cross-platform (x86_64, aarch64)    |

**Legend:** Y = Full support, P = Partial support, N = Not supported
