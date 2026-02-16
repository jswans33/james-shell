# James-Shell Development Cheatsheet

A quick-reference guide for day-to-day development on the james-shell project.

---

## Cargo Commands

| Command | Description |
|---|---|
| `cargo build` | Compile the project in debug mode |
| `cargo build --release` | Compile with optimizations for release |
| `cargo run` | Build and run the binary |
| `cargo run -- --flag` | Run with arguments passed to the binary |
| `cargo test` | Run all tests |
| `cargo test test_name` | Run tests matching a name |
| `cargo test -- --nocapture` | Run tests with stdout/stderr visible |
| `cargo clippy` | Run the linter for common mistakes and style |
| `cargo fmt` | Format all source files with rustfmt |
| `cargo fmt -- --check` | Check formatting without modifying files |
| `cargo doc --open` | Generate and open HTML documentation |
| `cargo check` | Type-check without producing a binary (faster) |
| `cargo clean` | Remove the `target/` directory |
| `cargo add crate_name` | Add a dependency to `Cargo.toml` |
| `cargo update` | Update dependencies to latest compatible versions |
| `cargo bench` | Run benchmarks (requires nightly or criterion) |
| `cargo tree` | Display the dependency graph |
| `cargo expand` | Show macro-expanded source (requires cargo-expand) |

---

## Rust Syntax Quick Reference

### Variable Binding

```rust
let x = 5;                     // immutable binding
let mut y = 10;                 // mutable binding
let (a, b) = (1, 2);           // destructuring
let _unused = 42;              // underscore prefix suppresses unused warnings
let x: i32 = 5;               // explicit type annotation
const MAX: usize = 1024;      // compile-time constant
static BANNER: &str = "hello"; // static lifetime variable
```

### Functions

```rust
// Basic function with return type
fn add(a: i32, b: i32) -> i32 {
    a + b   // no semicolon = implicit return
}

// Function returning nothing (unit type)
fn greet(name: &str) {
    println!("Hello, {name}");
}

// Function returning Result
fn parse_port(s: &str) -> Result<u16, std::num::ParseIntError> {
    s.parse::<u16>()
}

// Closures
let double = |x: i32| x * 2;
let sum = |a, b| a + b;
let mut count = 0;
let mut inc = || { count += 1; count };  // FnMut closure
```

### Control Flow

```rust
// if / else if / else
if x > 0 {
    "positive"
} else if x == 0 {
    "zero"
} else {
    "negative"
};

// if let (destructure a single pattern)
if let Some(val) = optional {
    println!("{val}");
}

// match (exhaustive pattern matching)
match value {
    0 => println!("zero"),
    1..=9 => println!("single digit"),
    n if n < 0 => println!("negative: {n}"),
    _ => println!("other"),
}

// loop (infinite, break to exit with value)
let result = loop {
    if done() { break 42; }
};

// while
while condition {
    // body
}

// while let
while let Some(item) = iter.next() {
    // body
}

// for (over any iterator)
for item in collection.iter() {
    // body
}

for i in 0..10 {       // exclusive range: 0 to 9
    // body
}

for i in 0..=10 {      // inclusive range: 0 to 10
    // body
}
```

### Error Handling Patterns

```rust
// The ? operator: propagate errors early
fn read_config(path: &str) -> Result<String, io::Error> {
    let contents = fs::read_to_string(path)?;  // returns Err if it fails
    Ok(contents)
}

// match on Result
match file.read_to_string(&mut buf) {
    Ok(n) => println!("Read {n} bytes"),
    Err(e) => eprintln!("Error: {e}"),
}

// unwrap_or / unwrap_or_else / unwrap_or_default
let port = env::var("PORT").unwrap_or("8080".into());
let val = result.unwrap_or_else(|e| { log_error(e); default() });
let v: Vec<i32> = result.unwrap_or_default();

// map / and_then for chaining
let len: Option<usize> = some_string.map(|s| s.len());
let parsed: Result<u16, _> = input.parse::<u16>().and_then(|n| validate(n));

// Convert between Option and Result
let val: Result<i32, &str> = opt.ok_or("missing value");
let val: Option<i32> = res.ok();
```

### String Operations

```rust
// Creation and conversion
let s: String = String::from("hello");
let s: String = "hello".to_string();
let s: &str = &my_string;                // String -> &str (deref coercion)
let s: String = format!("{} {}", a, b);  // formatted string

// Common methods
s.len()                        // byte length
s.is_empty()                   // true if length is 0
s.contains("sub")              // substring search
s.starts_with("pre")           // prefix check
s.ends_with("suf")             // suffix check
s.trim()                       // strip leading/trailing whitespace
s.trim_start() / s.trim_end()  // strip one side
s.split(' ')                   // split into iterator of &str
s.splitn(3, ' ')               // split into at most 3 parts
s.replace("old", "new")        // return new string with replacements
s.to_uppercase()               // return uppercased copy
s.to_lowercase()               // return lowercased copy
s.chars()                      // iterator over characters
s.as_bytes()                   // &[u8] view of the string

// Building strings
let mut s = String::new();
s.push('c');                   // append a char
s.push_str("text");            // append a &str

// OsString / OsStr (for OS-native strings, paths, env vars)
use std::ffi::{OsStr, OsString};
let os: &OsStr = OsStr::new("hello");
let os: OsString = OsString::from("hello");
```

### Common Iterator Methods

```rust
iter.map(|x| x * 2)           // transform each element
iter.filter(|x| x > &0)       // keep elements matching predicate
iter.enumerate()               // yield (index, value) pairs
iter.zip(other)                // pair elements from two iterators
iter.chain(other)              // concatenate two iterators
iter.take(5)                   // first 5 elements
iter.skip(3)                   // skip first 3 elements
iter.peekable()                // allow peeking at next without consuming
iter.flat_map(|x| x.into_iter()) // map and flatten
iter.collect::<Vec<_>>()       // gather into a collection
iter.fold(0, |acc, x| acc + x)// reduce with accumulator
iter.find(|x| x == &&target)  // first matching element (Option)
iter.position(|x| x == target)// index of first match (Option)
iter.any(|x| x > 10)          // true if any element matches
iter.all(|x| x > 0)           // true if all elements match
iter.count()                   // number of elements
iter.sum::<i32>()              // sum of numeric elements
iter.min() / iter.max()        // minimum / maximum element
iter.cloned()                  // clone each element (for &T -> T)
```

---

## Shell Concepts Quick Reference

### File Descriptor Table

| FD | Name   | Constant        | Description                        |
|----|--------|-----------------|------------------------------------|
| 0  | stdin  | `STDIN_FILENO`  | Standard input (keyboard by default) |
| 1  | stdout | `STDOUT_FILENO` | Standard output (terminal by default) |
| 2  | stderr | `STDERR_FILENO` | Standard error (terminal by default) |

In Rust, use `std::io::stdin()`, `std::io::stdout()`, and `std::io::stderr()`.

### Signal Table

| Number | Name      | Default Action | Keyboard    | Description                        |
|--------|-----------|----------------|-------------|------------------------------------|
| 1      | SIGHUP    | Terminate      | --          | Hangup / terminal closed           |
| 2      | SIGINT    | Terminate      | Ctrl-C      | Interrupt from keyboard            |
| 3      | SIGQUIT   | Core dump      | Ctrl-\\     | Quit from keyboard                 |
| 9      | SIGKILL   | Terminate      | --          | Forced kill (cannot be caught)     |
| 13     | SIGPIPE   | Terminate      | --          | Broken pipe                        |
| 14     | SIGALRM   | Terminate      | --          | Timer alarm                        |
| 15     | SIGTERM   | Terminate      | --          | Graceful termination request       |
| 17     | SIGCHLD   | Ignore         | --          | Child process stopped or exited    |
| 18     | SIGCONT   | Continue       | --          | Resume stopped process             |
| 19     | SIGSTOP   | Stop           | --          | Forced stop (cannot be caught)     |
| 20     | SIGTSTP   | Stop           | Ctrl-Z      | Stop from keyboard                 |
| 21     | SIGTTIN   | Stop           | --          | Background process reading stdin   |
| 22     | SIGTTOU   | Stop           | --          | Background process writing stdout  |
| 28     | SIGWINCH  | Ignore         | --          | Terminal window size changed        |

Note: Signal numbers vary by platform. The numbers above are for Linux (x86_64). On Windows, signals are emulated with limited support (primarily `SIGINT` via `Ctrl-C`).

### Exit Code Conventions

| Code    | Meaning                                            |
|---------|----------------------------------------------------|
| 0       | Success                                            |
| 1       | General / miscellaneous error                      |
| 2       | Misuse of shell builtin or invalid usage           |
| 126     | Command found but not executable (permission denied) |
| 127     | Command not found                                  |
| 128     | Invalid exit argument                              |
| 128 + N | Fatal signal N (e.g., 130 = killed by SIGINT)     |
| 255     | Exit status out of range                           |

### Redirection Syntax

| Syntax            | Description                                          |
|-------------------|------------------------------------------------------|
| `cmd > file`      | Redirect stdout to file (overwrite)                  |
| `cmd >> file`     | Redirect stdout to file (append)                     |
| `cmd < file`      | Redirect stdin from file                             |
| `cmd 2> file`     | Redirect stderr to file (overwrite)                  |
| `cmd 2>> file`    | Redirect stderr to file (append)                     |
| `cmd 2>&1`        | Redirect stderr to wherever stdout is going          |
| `cmd 1>&2`        | Redirect stdout to wherever stderr is going          |
| `cmd &> file`     | Redirect both stdout and stderr to file (overwrite)  |
| `cmd &>> file`    | Redirect both stdout and stderr to file (append)     |
| `cmd > /dev/null` | Discard stdout                                       |
| `cmd 2> /dev/null`| Discard stderr                                       |
| `cmd <<EOF`       | Here document (multi-line stdin until EOF)            |
| `cmd <<<'text'`   | Here string (single string to stdin)                 |
| `cmd n> file`     | Redirect file descriptor n to file                   |
| `cmd n>&m`        | Duplicate file descriptor m onto n                   |
| `cmd n<&-`        | Close file descriptor n for input                    |
| `cmd n>&-`        | Close file descriptor n for output                   |

### Expansion Order

The shell processes expansions in this order:

```
1. Brace expansion        {a,b,c} → a b c
2. Tilde expansion         ~/dir → /home/user/dir
3. Parameter/variable      $VAR, ${VAR:-default}
4. Command substitution    $(cmd) or `cmd`
5. Arithmetic expansion    $((1 + 2))
6. Word splitting          on unquoted results of 3, 4, 5
7. Glob / pathname         *.rs, src/**/*.rs
8. Quote removal           remove remaining quotes
```

Double quotes (`"..."`) suppress steps 1, 6, and 7 but allow 3, 4, and 5.
Single quotes (`'...'`) suppress all expansions.

### Special Variables

| Variable    | Description                                         |
|-------------|-----------------------------------------------------|
| `$?`        | Exit status of the last command                     |
| `$$`        | PID of the current shell                            |
| `$!`        | PID of the last background process                  |
| `$0`        | Name of the shell or script                         |
| `$1`..`$9`  | Positional parameters (script/function arguments)   |
| `$#`        | Number of positional parameters                     |
| `$@`        | All positional parameters (individually quoted)     |
| `$*`        | All positional parameters (as a single word)        |
| `$-`        | Current shell option flags                          |
| `$_`        | Last argument of the previous command               |
| `$HOME`     | Home directory of the current user                  |
| `$PATH`     | Executable search path                              |
| `$PWD`      | Current working directory                           |
| `$OLDPWD`   | Previous working directory                          |
| `$USER`     | Current username                                    |
| `$SHELL`    | Path to the user's default shell                    |
| `$IFS`      | Internal field separator (default: space, tab, newline) |
| `$TERM`     | Terminal type                                       |

---

## Cross-Platform Patterns

### Platform-Conditional Compilation

```rust
// Conditional compilation with cfg attributes
#[cfg(unix)]
fn get_pid() -> u32 {
    std::process::id()
}

#[cfg(windows)]
fn get_pid() -> u32 {
    std::process::id()  // same API, different OS implementation
}

// cfg! macro for runtime branching (both branches must compile)
fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

// Platform-specific modules
#[cfg(unix)]
mod unix_impl;

#[cfg(windows)]
mod windows_impl;

// Conditional dependencies in Cargo.toml
// [target.'cfg(unix)'.dependencies]
// nix = "0.29"
//
// [target.'cfg(windows)'.dependencies]
// windows-sys = "0.59"

// Platform-specific use statements
#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
```

### Common Platform Differences

| Concept           | Unix / macOS                  | Windows                         |
|-------------------|-------------------------------|---------------------------------|
| Path separator    | `/`                           | `\` (but `/` often works)      |
| PATH delimiter    | `:` (colon)                   | `;` (semicolon)                |
| Null device       | `/dev/null`                   | `NUL`                          |
| Home directory    | `$HOME` or `~`                | `%USERPROFILE%` or `$HOME`     |
| Line endings      | `\n` (LF)                     | `\r\n` (CRLF)                  |
| Executable ext    | (none)                        | `.exe`, `.cmd`, `.bat`         |
| Env var syntax    | `$VAR`                        | `%VAR%` (cmd) or `$VAR` (PS)  |
| Process creation  | `fork()` + `exec()`           | `CreateProcess()`              |
| Signals           | Full POSIX signal set          | Limited (`SIGINT`, `SIGTERM`)  |
| File permissions  | `rwxrwxrwx` (mode bits)       | ACLs                           |
| Temp directory    | `$TMPDIR` or `/tmp`           | `%TEMP%`                       |
| User config dir   | `~/.config/`                  | `%APPDATA%`                    |
| Shebang (`#!`)    | Supported by kernel           | Not natively supported         |

### Cross-Platform Code Patterns

```rust
use std::env;
use std::path::{Path, PathBuf};

// Use std::path for portable path handling (never hardcode / or \)
let config_dir: PathBuf = dirs::config_dir()
    .unwrap_or_else(|| PathBuf::from("."));
let config_file = config_dir.join("james-shell").join("config.toml");

// Use std::env for portable environment variable access
let home = env::var("HOME")
    .or_else(|_| env::var("USERPROFILE"))
    .unwrap_or_else(|_| String::from("."));

// PATH splitting (respects platform delimiter)
let path_var = env::var("PATH").unwrap_or_default();
for dir in env::split_paths(&path_var) {
    // dir is a PathBuf for each PATH component
}

// Building a PATH value
let new_path = env::join_paths(paths)?;  // returns OsString

// Use MAIN_SEPARATOR for display, but prefer Path::join for construction
let sep = std::path::MAIN_SEPARATOR;  // '/' on Unix, '\\' on Windows

// Executable resolution
fn find_executable(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var).find_map(|dir| {
        let full = dir.join(name);
        if full.is_file() {
            return Some(full);
        }
        // On Windows, also check with common extensions
        if cfg!(windows) {
            for ext in &["exe", "cmd", "bat", "com"] {
                let with_ext = dir.join(format!("{name}.{ext}"));
                if with_ext.is_file() {
                    return Some(with_ext);
                }
            }
        }
        None
    })
}

// Line ending handling
fn normalize_line_endings(s: &str) -> String {
    s.replace("\r\n", "\n")
}

// Process spawning (works cross-platform)
use std::process::Command;

let output = Command::new("ls")     // or "dir" on Windows
    .arg("-la")
    .current_dir("/tmp")
    .output()?;

// Platform-specific process extensions
#[cfg(unix)]
{
    use std::os::unix::process::CommandExt;
    // Set process group, uid, etc.
    let mut cmd = Command::new("prog");
    unsafe { cmd.pre_exec(|| { /* runs after fork, before exec */ Ok(()) }); }
}

#[cfg(windows)]
{
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    Command::new("prog")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
}
```
