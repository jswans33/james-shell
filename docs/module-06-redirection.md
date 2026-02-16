# Module 6: I/O Redirection

## What are we building?

Every process has three standard I/O streams: stdin (input), stdout (output), and stderr (errors). By default, all three are connected to the terminal. **I/O redirection** lets the user rewire these streams -- sending output to a file, reading input from a file, or discarding output entirely.

After this module, your shell will support:

```
ls > files.txt            # stdout to file (overwrite)
ls >> files.txt           # stdout to file (append)
sort < data.txt           # stdin from file
ls /nonexistent 2> err.txt   # stderr to file
ls /nonexistent 2>&1      # stderr merged into stdout
echo hello <<< "world"   # here string (feed string as stdin)
ls > /dev/null            # discard output (NUL on Windows)
```

This is where our shell starts feeling genuinely powerful. Redirection is one of the features that makes Unix shells a composable toolkit rather than just a program launcher.

---

## Concept 1: File Descriptors

Every open file, pipe, socket, or terminal connection is represented by a **file descriptor** (fd) -- a small non-negative integer. Every process starts with three:

```
  ┌─────────────┐
  │   Process    │
  │              │
  │  fd 0 stdin  │ ──── keyboard (terminal input)
  │  fd 1 stdout │ ──── screen (terminal output)
  │  fd 2 stderr │ ──── screen (terminal error output)
  │              │
  └─────────────┘
```

When you type `ls`, the child process inherits these three file descriptors from the shell. That is why `ls` output appears on your screen -- its stdout (fd 1) is connected to your terminal.

**Redirection** means: before running the command, replace one of these file descriptors with a file (or pipe, or `/dev/null`).

```
  ls > files.txt

  ┌─────────────┐
  │   ls         │
  │              │
  │  fd 0 stdin  │ ──── keyboard (unchanged)
  │  fd 1 stdout │ ──── files.txt (REDIRECTED to file)
  │  fd 2 stderr │ ──── screen (unchanged)
  │              │
  └─────────────┘
```

Now `ls` writes to `files.txt` instead of the screen. The program itself has no idea -- it just writes to fd 1 as always.

---

## Concept 2: Redirection Operators

Here is every redirection operator we will implement, with examples:

### Output redirection: `>` and `>>`

| Operator | Name | Behavior | Example |
|----------|------|----------|---------|
| `>` | Redirect stdout (truncate) | Create/overwrite the file, write stdout to it | `echo hello > greet.txt` |
| `>>` | Redirect stdout (append) | Create/append to file, write stdout to it | `echo world >> greet.txt` |
| `2>` | Redirect stderr (truncate) | Create/overwrite the file, write stderr to it | `ls /bad 2> err.txt` |
| `2>>` | Redirect stderr (append) | Create/append to file, write stderr to it | `make 2>> build.log` |

### Input redirection: `<`

| Operator | Name | Behavior | Example |
|----------|------|----------|---------|
| `<` | Redirect stdin | Read input from a file instead of the keyboard | `sort < names.txt` |

### Descriptor duplication: `2>&1`

| Operator | Name | Behavior | Example |
|----------|------|----------|---------|
| `2>&1` | Merge stderr into stdout | stderr goes wherever stdout is going | `ls /bad > all.txt 2>&1` |
| `1>&2` | Merge stdout into stderr | stdout goes wherever stderr is going | `echo "error" 1>&2` |

### Here strings: `<<<`

| Operator | Name | Behavior | Example |
|----------|------|----------|---------|
| `<<<` | Here string | Feed a string as stdin | `cat <<< "hello world"` |

### Order matters for `2>&1`

This is a classic gotcha:

```bash
# CORRECT: redirect stdout to file, THEN merge stderr into stdout
ls /bad > all.txt 2>&1
# Result: both stdout and stderr go to all.txt

# WRONG ORDER: merge stderr into stdout (still terminal), THEN redirect stdout
ls /bad 2>&1 > all.txt
# Result: stderr goes to terminal, only stdout goes to all.txt
```

Redirections are processed **left to right**. When `2>&1` executes, it makes fd 2 point wherever fd 1 currently points. If fd 1 has already been redirected to a file, fd 2 follows it there. If fd 1 is still the terminal, fd 2 gets the terminal too.

---

## Concept 3: Representing Redirections in the Parser

We need to update our parser to recognize redirection tokens and store them in the command structure.

### The Redirection struct

```rust
#[derive(Debug, Clone)]
pub enum RedirectTarget {
    File(String),          // > filename
    FileAppend(String),    // >> filename
    Fd(i32),               // 2>&1 (duplicate another fd)
    HereString(String),    // <<< "text"
}

#[derive(Debug, Clone)]
pub struct Redirection {
    pub fd: i32,                   // Which fd to redirect (0, 1, or 2)
    pub target: RedirectTarget,
}
```

### Updating the Command struct

```rust
#[derive(Debug)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
    pub redirections: Vec<Redirection>,    // NEW
}
```

### Parsing redirection tokens

The tokenizer needs to recognize `>`, `>>`, `<`, `2>`, `2>>`, `2>&1`, and `<<<`. Here is the logic:

```rust
fn parse_redirections(tokens: &[String]) -> (Vec<String>, Vec<Redirection>) {
    let mut args = Vec::new();
    let mut redirections = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        let token = &tokens[i];

        match token.as_str() {
            ">" => {
                // stdout > file
                i += 1;
                if i < tokens.len() {
                    redirections.push(Redirection {
                        fd: 1,
                        target: RedirectTarget::File(tokens[i].clone()),
                    });
                } else {
                    eprintln!("jsh: syntax error: expected filename after '>'");
                }
            }
            ">>" => {
                // stdout >> file (append)
                i += 1;
                if i < tokens.len() {
                    redirections.push(Redirection {
                        fd: 1,
                        target: RedirectTarget::FileAppend(tokens[i].clone()),
                    });
                }
            }
            "<" => {
                // stdin < file
                i += 1;
                if i < tokens.len() {
                    redirections.push(Redirection {
                        fd: 0,
                        target: RedirectTarget::File(tokens[i].clone()),
                    });
                }
            }
            "<<<" => {
                // here string
                i += 1;
                if i < tokens.len() {
                    redirections.push(Redirection {
                        fd: 0,
                        target: RedirectTarget::HereString(tokens[i].clone()),
                    });
                }
            }
            "2>" => {
                // stderr > file
                i += 1;
                if i < tokens.len() {
                    redirections.push(Redirection {
                        fd: 2,
                        target: RedirectTarget::File(tokens[i].clone()),
                    });
                }
            }
            "2>>" => {
                // stderr >> file (append)
                i += 1;
                if i < tokens.len() {
                    redirections.push(Redirection {
                        fd: 2,
                        target: RedirectTarget::FileAppend(tokens[i].clone()),
                    });
                }
            }
            "2>&1" => {
                redirections.push(Redirection {
                    fd: 2,
                    target: RedirectTarget::Fd(1),
                });
            }
            "1>&2" => {
                redirections.push(Redirection {
                    fd: 1,
                    target: RedirectTarget::Fd(2),
                });
            }
            _ => {
                args.push(token.clone());
            }
        }

        i += 1;
    }

    (args, redirections)
}
```

### Tokenizer updates

The tokenizer from Module 2 needs to emit `>`, `>>`, `<`, `<<<`, `2>`, `2>>`, `2>&1`, and `1>&2` as separate tokens. The key change is treating `>`, `<`, and digit-redirect combinations as special characters (similar to how we treat whitespace and quotes):

```rust
// Inside the tokenizer state machine:
// When we encounter '>' or '<' in Normal/InWord state,
// finalize the current token and emit the redirect operator.

match (state, ch) {
    (Normal, '>') | (InWord, '>') => {
        finalize_current_token();
        if chars.peek() == Some(&'>') {
            chars.next();
            tokens.push(">>".to_string());
        } else {
            tokens.push(">".to_string());
        }
    }
    (Normal, '<') | (InWord, '<') => {
        finalize_current_token();
        if chars.peek() == Some(&'<') {
            chars.next();
            if chars.peek() == Some(&'<') {
                chars.next();
                tokens.push("<<<".to_string());
            } else {
                tokens.push("<<".to_string());  // here-doc (future)
            }
        } else {
            tokens.push("<".to_string());
        }
    }
    // ... existing rules ...
}
```

For `2>` and `2>&1`, we check if a token is exactly `"2"` followed by a `>` character and merge them:

```rust
// After tokenizing, post-process to merge "2" + ">" into "2>"
fn merge_fd_redirects(tokens: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        if (tokens[i] == "1" || tokens[i] == "2") && i + 1 < tokens.len() {
            let fd = &tokens[i];
            let next = &tokens[i + 1];

            if next == ">" {
                result.push(format!("{}>", fd));
                i += 2;
                continue;
            } else if next == ">>" {
                result.push(format!("{}>>", fd));
                i += 2;
                continue;
            } else if next.starts_with(">&") {
                result.push(format!("{}{}", fd, next));
                i += 2;
                continue;
            }
        }

        result.push(tokens[i].clone());
        i += 1;
    }

    result
}
```

---

## Concept 4: Applying Redirections with `std::process::Command`

Here is the core insight for our cross-platform shell: **we do not use raw `dup2()` system calls**. Instead, we use Rust's `std::process::Command` API, which provides a clean, safe, cross-platform way to set up I/O for child processes.

### The Stdio type

`std::process::Stdio` represents what a file descriptor should be connected to:

```rust
use std::process::{Command, Stdio};
use std::fs::{File, OpenOptions};

// Inherit from the shell (default behavior):
Command::new("ls").stdout(Stdio::inherit());

// Pipe (capture in memory or connect to another process):
Command::new("ls").stdout(Stdio::piped());

// Null (discard all output):
Command::new("ls").stdout(Stdio::null());

// From a file:
let file = File::create("output.txt")?;
Command::new("ls").stdout(Stdio::from(file));
```

### Applying our Redirection structs

```rust
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::process::{Command, Stdio};

fn apply_redirections(
    cmd: &mut Command,
    redirections: &[Redirection],
) -> Result<Option<std::process::ChildStdin>, String> {
    let mut here_string: Option<String> = None;

    for redir in redirections {
        match (&redir.target, redir.fd) {
            // stdout > file (truncate)
            (RedirectTarget::File(path), 1) => {
                let file = File::create(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stdout(Stdio::from(file));
            }
            // stdout >> file (append)
            (RedirectTarget::FileAppend(path), 1) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stdout(Stdio::from(file));
            }
            // stdin < file
            (RedirectTarget::File(path), 0) => {
                let file = File::open(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stdin(Stdio::from(file));
            }
            // stderr 2> file (truncate)
            (RedirectTarget::File(path), 2) => {
                let file = File::create(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stderr(Stdio::from(file));
            }
            // stderr 2>> file (append)
            (RedirectTarget::FileAppend(path), 2) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stderr(Stdio::from(file));
            }
            // 2>&1 -- stderr goes wherever stdout is going
            (RedirectTarget::Fd(target_fd), 2) if *target_fd == 1 => {
                // In Rust's Command API, we handle this by setting
                // stderr to the same Stdio as stdout.
                // Since we can't "read" the current stdout Stdio,
                // we need to track what stdout was set to.
                // See Concept 5 for the full approach.
            }
            // Here string
            (RedirectTarget::HereString(text), 0) => {
                here_string = Some(text.clone());
                cmd.stdin(Stdio::piped());
            }
            _ => {
                return Err(format!("jsh: unsupported redirection: fd {} -> {:?}",
                    redir.fd, redir.target));
            }
        }
    }

    Ok(None) // We will handle here_string writing separately
}
```

---

## Concept 5: Handling `2>&1` (Descriptor Duplication)

The `2>&1` redirect means "make stderr point wherever stdout is currently pointing." This is easy with raw `dup2()` on Unix, but Rust's `Command` API does not directly expose descriptor duplication. Here is how we handle it cross-platform.

### Strategy: track the stdout target and reuse it

```rust
use std::fs::File;
use std::process::{Command, Stdio};

#[derive(Clone)]
enum StdioTarget {
    Inherit,
    Null,
    FilePath(String, bool),  // (path, append)
    Piped,
}

fn apply_redirections_v2(
    cmd: &mut Command,
    redirections: &[Redirection],
) -> Result<(), String> {
    // Track what each fd is set to so we can duplicate it
    let mut stdout_target = StdioTarget::Inherit;
    let mut stderr_target = StdioTarget::Inherit;

    // Process redirections left-to-right (order matters!)
    for redir in redirections {
        match (&redir.target, redir.fd) {
            (RedirectTarget::File(path), 1) => {
                let file = File::create(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stdout(Stdio::from(file));
                stdout_target = StdioTarget::FilePath(path.clone(), false);
            }
            (RedirectTarget::File(path), 2) => {
                let file = File::create(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stderr(Stdio::from(file));
                stderr_target = StdioTarget::FilePath(path.clone(), false);
            }
            // 2>&1: make stderr go where stdout goes
            (RedirectTarget::Fd(1), 2) => {
                match &stdout_target {
                    StdioTarget::Inherit => {
                        cmd.stderr(Stdio::inherit());
                    }
                    StdioTarget::Null => {
                        cmd.stderr(Stdio::null());
                    }
                    StdioTarget::FilePath(path, append) => {
                        // Open the SAME file again for stderr
                        let file = if *append {
                            std::fs::OpenOptions::new()
                                .create(true).append(true).open(path)
                        } else {
                            std::fs::OpenOptions::new()
                                .create(true).write(true).open(path)
                        };
                        let file = file.map_err(|e| format!("jsh: {}: {}", path, e))?;
                        cmd.stderr(Stdio::from(file));
                    }
                    StdioTarget::Piped => {
                        cmd.stderr(Stdio::piped());
                    }
                }
                stderr_target = stdout_target.clone();
            }
            // 1>&2: make stdout go where stderr goes (less common)
            (RedirectTarget::Fd(2), 1) => {
                match &stderr_target {
                    StdioTarget::Inherit => cmd.stdout(Stdio::inherit()),
                    StdioTarget::Null => cmd.stdout(Stdio::null()),
                    StdioTarget::FilePath(path, append) => {
                        let file = if *append {
                            std::fs::OpenOptions::new()
                                .create(true).append(true).open(path)
                        } else {
                            std::fs::OpenOptions::new()
                                .create(true).write(true).open(path)
                        };
                        let file = file.map_err(|e| format!("jsh: {}: {}", path, e))?;
                        cmd.stdout(Stdio::from(file));
                    }
                    StdioTarget::Piped => cmd.stdout(Stdio::piped()),
                };
                stdout_target = stderr_target.clone();
            }
            // ... other cases from Concept 4 ...
            _ => {}
        }
    }

    Ok(())
}
```

### Why open the same file twice?

When we redirect `> all.txt 2>&1`, both stdout and stderr go to `all.txt`. On Unix, the raw approach uses `dup2()` to make fd 2 share the same file descriptor as fd 1 -- they share a single write cursor, so output interleaves correctly.

In our Rust approach, we open the file twice. This creates two independent write cursors, which can cause interleaved output to overlap. For most practical cases this works fine, but if you need precise interleaving, you would use platform-specific code:

```rust
// Unix-only: true fd duplication via the os_pipe or nix crate
#[cfg(unix)]
{
    use std::os::unix::io::AsRawFd;
    // After spawning, use dup2 to duplicate the fd
}
```

For cross-platform correctness at our level, the two-open approach is good enough.

---

## Concept 6: Here Strings (`<<<`)

A here string feeds a string directly into a command's stdin without needing a file:

```bash
cat <<< "hello world"
# Output: hello world

grep "pattern" <<< "does this have pattern in it?"
# Output: does this have pattern in it?
```

### How to implement with `std::process::Command`

The trick is to use `Stdio::piped()` for stdin, then write the string to the pipe after spawning:

```rust
fn execute_with_here_string(
    program: &str,
    args: &[String],
    here_string: &str,
) -> Result<i32, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("jsh: {}: {}", program, e))?;

    // Write the here string to the child's stdin
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        // Write the string followed by a newline (bash convention)
        writeln!(stdin, "{}", here_string)
            .map_err(|e| format!("jsh: write error: {}", e))?;
        // stdin is dropped here, closing the pipe
        // This signals EOF to the child process
    }

    let status = child.wait()
        .map_err(|e| format!("jsh: wait error: {}", e))?;

    Ok(status.code().unwrap_or(-1))
}
```

### Why `stdin.take()`?

`child.stdin` is an `Option<ChildStdin>`. We use `.take()` to move it out of the `Option`, leaving `None` behind. When the `ChildStdin` is dropped at the end of the `if let` block, the pipe is closed, which sends EOF to the child. This is critical -- without closing the pipe, programs like `cat` would wait forever for more input.

---

## Concept 7: `/dev/null` and Cross-Platform Null Devices

Sometimes you want to discard output entirely:

```bash
ls > /dev/null           # Unix: discard stdout
ls > NUL                 # Windows: discard stdout
ls 2> /dev/null          # Discard stderr
ls > /dev/null 2>&1      # Discard everything
```

### The null device

| Platform | Null device | What it does |
|----------|-------------|-------------|
| Unix/Mac | `/dev/null` | Reads return EOF, writes are silently discarded |
| Windows | `NUL` | Same behavior, different name |

### Cross-platform approach in Rust

Rather than worrying about the device name, Rust provides `Stdio::null()`:

```rust
// This works on ALL platforms -- no /dev/null or NUL needed
Command::new("ls")
    .stdout(Stdio::null())    // discard stdout
    .stderr(Stdio::null())    // discard stderr
    .status()?;
```

But users will type `/dev/null` (or `NUL` on Windows) in their commands. We can detect this and use `Stdio::null()` transparently:

```rust
fn is_null_device(path: &str) -> bool {
    if cfg!(windows) {
        path.eq_ignore_ascii_case("NUL")
            || path.eq_ignore_ascii_case("/dev/null")  // Be nice to Unix users on Windows
    } else {
        path == "/dev/null"
    }
}

// In the redirection handler:
if is_null_device(path) {
    cmd.stdout(Stdio::null());
} else {
    let file = File::create(path)?;
    cmd.stdout(Stdio::from(file));
}
```

This means `ls > /dev/null` works on both Windows and Unix with our shell -- even though Windows does not have `/dev/null`. Our shell translates it.

---

## Concept 8: The Complete Execution Flow with Redirections

Here is how the executor changes to support redirections:

```rust
// src/executor.rs

use std::fs::{File, OpenOptions};
use std::process::{Command, Stdio};
use std::io::Write;

pub fn execute(
    program: &str,
    args: &[String],
    redirections: &[Redirection],
) -> i32 {
    let mut cmd = Command::new(program);
    cmd.args(args);

    // Apply redirections
    let mut here_string: Option<String> = None;

    for redir in redirections {
        match apply_single_redirection(&mut cmd, redir, &mut here_string) {
            Ok(()) => {}
            Err(msg) => {
                eprintln!("{}", msg);
                return 1;
            }
        }
    }

    // Spawn the process
    let result = if here_string.is_some() {
        // Need to write to stdin after spawning
        cmd.stdin(Stdio::piped());
        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(text) = &here_string {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = writeln!(stdin, "{}", text);
                    }
                }
                child.wait()
            }
            Err(e) => {
                eprintln!("jsh: {}: {}", program, e);
                return 127;
            }
        }
    } else {
        cmd.status()
    };

    match result {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => {
            eprintln!("jsh: {}: {}", program, e);
            127
        }
    }
}

fn apply_single_redirection(
    cmd: &mut Command,
    redir: &Redirection,
    here_string: &mut Option<String>,
) -> Result<(), String> {
    match (&redir.target, redir.fd) {
        (RedirectTarget::File(path), 1) => {
            if is_null_device(path) {
                cmd.stdout(Stdio::null());
            } else {
                let file = File::create(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stdout(Stdio::from(file));
            }
        }
        (RedirectTarget::FileAppend(path), 1) => {
            let file = OpenOptions::new()
                .create(true).append(true).open(path)
                .map_err(|e| format!("jsh: {}: {}", path, e))?;
            cmd.stdout(Stdio::from(file));
        }
        (RedirectTarget::File(path), 0) => {
            let file = File::open(path)
                .map_err(|e| format!("jsh: {}: {}", path, e))?;
            cmd.stdin(Stdio::from(file));
        }
        (RedirectTarget::File(path), 2) => {
            if is_null_device(path) {
                cmd.stderr(Stdio::null());
            } else {
                let file = File::create(path)
                    .map_err(|e| format!("jsh: {}: {}", path, e))?;
                cmd.stderr(Stdio::from(file));
            }
        }
        (RedirectTarget::FileAppend(path), 2) => {
            let file = OpenOptions::new()
                .create(true).append(true).open(path)
                .map_err(|e| format!("jsh: {}: {}", path, e))?;
            cmd.stderr(Stdio::from(file));
        }
        (RedirectTarget::HereString(text), 0) => {
            *here_string = Some(text.clone());
        }
        (RedirectTarget::Fd(target_fd), _) => {
            // Descriptor duplication -- handled by the v2 tracking approach
            // (see Concept 5 for the full implementation)
            let _ = target_fd; // placeholder
        }
        _ => {
            return Err(format!(
                "jsh: unsupported redirection: fd {} -> {:?}",
                redir.fd, redir.target
            ));
        }
    }
    Ok(())
}
```

### Updated architecture diagram

```
  "ls -la > files.txt 2>&1"
          |
          v
  +-----------------+
  |   Tokenizer     |  ["ls", "-la", ">", "files.txt", "2>&1"]
  +-----------------+
          |
          v
  +-----------------+
  | Redirect Parser |  args: ["ls", "-la"]
  |                 |  redirections: [
  |                 |    { fd: 1, target: File("files.txt") },
  |                 |    { fd: 2, target: Fd(1) },
  |                 |  ]
  +-----------------+
          |
          v
  +-----------------+
  |    Expander     |  Module 5: expand variables in args AND filenames
  +-----------------+
          |
          v
  +-----------------+
  |    Executor     |  Command::new("ls")
  |                 |    .args(&["-la"])
  |                 |    .stdout(File::create("files.txt"))
  |                 |    .stderr(same file)
  |                 |    .status()
  +-----------------+
```

Note that expansion (Module 5) applies to redirection filenames too. `echo hello > $LOGFILE` should expand `$LOGFILE` before opening the file.

---

## Key Rust concepts used

- **`std::fs::File` and `OpenOptions`** -- creating, opening, and appending to files with fine-grained control
- **`std::process::Stdio`** -- the `inherit()`, `piped()`, `null()`, and `from()` constructors for configuring child process I/O
- **`Stdio::from(file)`** -- converting an open `File` into a `Stdio`, transferring ownership of the file descriptor to the child process
- **`Option::take()`** -- moving a value out of an `Option`, critical for the here-string stdin pattern
- **`child.stdin.take()`** -- obtaining the writable end of a piped stdin, then dropping it to send EOF
- **Enum variants with data** -- `RedirectTarget::File(String)`, `RedirectTarget::Fd(i32)` etc. for type-safe redirection representation
- **`cfg!(windows)`** -- runtime check for OS, used to normalize null device paths
- **`map_err`** -- converting `io::Error` into user-friendly error messages

---

## Milestone

After implementing redirection, your shell should handle these scenarios:

```
jsh> echo hello > greet.txt
jsh> cat greet.txt
hello

jsh> echo world >> greet.txt
jsh> cat greet.txt
hello
world

jsh> sort < greet.txt
hello
world

jsh> ls /nonexistent 2> errors.txt
jsh> cat errors.txt
ls: cannot access '/nonexistent': No such file or directory

jsh> ls /nonexistent > /dev/null 2>&1
jsh>

jsh> cat <<< "hello from a here string"
hello from a here string

jsh> echo secret > /dev/null
jsh>
```

On Windows, the same commands work. `/dev/null` is transparently mapped to `NUL`. File paths use the native separator. The `Stdio` API handles all platform differences behind the scenes.

---

## What's next?

Module 7 adds **pipes** -- connecting the stdout of one command to the stdin of the next. That is when `ls | grep .rs | sort` starts working, and our shell becomes a genuine data-processing tool.
