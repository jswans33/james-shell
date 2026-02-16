# Module 3: Process Execution

## What are we building?

Our shell can parse commands but doesn't actually *do* anything. Time to fix that. After this module, typing `ls -la` or `whoami` will run the real program and show its output.

---

## Concept 1: How programs run

When you type `ls -la` in a shell, here's what happens under the hood:

### On Unix (Linux/Mac):
1. **fork()** — the shell creates a copy of itself (a child process)
2. **exec()** — the child replaces itself with the `ls` program
3. **wait()** — the parent (shell) waits for the child to finish
4. The shell reads the child's exit code and shows a new prompt

### On Windows:
1. **CreateProcess()** — Windows creates a new process directly (no fork)
2. **WaitForSingleObject()** — the shell waits for it to finish
3. Same result: exit code, new prompt

### In Rust (cross-platform):
`std::process::Command` wraps both of these behind a single API:

```rust
use std::process::Command;

let status = Command::new("ls")
    .args(&["-la", "/tmp"])
    .status()           // Runs the program and waits for it to finish
    .expect("failed to execute");

println!("Exit code: {}", status.code().unwrap_or(-1));
```

This is one of Rust's strengths — you write one set of code and it uses the right OS calls underneath.

---

## Concept 2: What is a process?

A **process** is a running instance of a program. Every process has:

| Property | Description | Example |
|----------|-------------|---------|
| **PID** | Process ID — unique number | 12345 |
| **Parent PID** | Who created this process | Your shell's PID |
| **Exit code** | 0 = success, non-zero = error | 0, 1, 127 |
| **Environment** | Key-value pairs (PATH, HOME, etc.) | Inherited from parent |
| **Working directory** | Where the process "is" on the filesystem | `/home/jswan` |
| **stdin/stdout/stderr** | Input/output streams | Usually the terminal |

When your shell runs `ls`, it creates a **child process**. That child inherits the shell's environment, working directory, and I/O streams. That's why `ls` shows files in *your* current directory — it inherited the cwd from the shell.

---

## Concept 3: PATH resolution

When you type `ls`, how does the OS know to run `/usr/bin/ls`?

The **PATH** environment variable contains a list of directories to search:

```
PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
```

The shell (or OS) searches each directory in order:
1. `/usr/local/bin/ls` — exists? No.
2. `/usr/bin/ls` — exists? Yes! Run it.

On Windows, PATH works the same way, plus Windows also searches the current directory and appends `.exe`, `.cmd`, etc. automatically.

`std::process::Command` handles PATH resolution for you — just pass the program name and it searches PATH.

### What about command not found?

If the program isn't in any PATH directory, `Command::new("nonexistent").status()` returns an `Err`. We catch this and print a friendly error:

```
jsh: command not found: nonexistent
```

---

## Concept 4: Exit codes

Every process exits with a numeric code:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Misuse of command |
| 126 | Permission denied (file exists but isn't executable) |
| 127 | Command not found |
| 128+N | Killed by signal N (e.g., 130 = Ctrl-C = SIGINT) |

Shells track the last exit code. In bash, `$?` gives you the last exit code. We'll store it in a variable for later use.

```rust
struct Shell {
    last_exit_code: i32,
}
```

---

## Concept 5: `.status()` vs `.output()` vs `.spawn()`

`std::process::Command` has three ways to run a program:

```rust
// .status() — run and wait, output goes directly to terminal
let status = Command::new("ls").status()?;

// .output() — run and wait, capture stdout/stderr into memory
let output = Command::new("ls").output()?;
println!("{}", String::from_utf8_lossy(&output.stdout));

// .spawn() — start the process but DON'T wait (returns a handle)
let child = Command::new("ls").spawn()?;
// ... do other stuff ...
child.wait()?;  // Now wait for it
```

For our shell, we use **`.status()`** — we want output to go straight to the terminal (like a real shell), and we want to wait for the command to finish before showing the next prompt.

We'll use `.spawn()` later in Module 8 (job control) for background processes.

---

## Concept 6: Connecting the parser to the executor

The flow is now:

```
User input → Parser (Module 2) → Command struct → Executor (Module 3) → OS runs program
    ↓              ↓                    ↓                ↓
 "ls -la"    tokenize/parse    Command {           Command::new("ls")
                               program: "ls",         .args(&["-la"])
                               args: ["-la"]          .status()
                               }
```

The executor module takes a `Command` struct and:
1. Creates a `std::process::Command`
2. Sets the program name and args
3. Calls `.status()` to run and wait
4. Returns the exit code

---

## Key Rust concepts used

- **`std::process::Command`** — the main API for running external programs
- **`Result` error handling** — `status()` can fail (command not found, permission denied)
- **Pattern matching on `Result`** — distinguishing "command not found" from "command ran but failed"
- **Struct methods** — implementing `execute()` on a `Shell` struct
- **Modules** — `src/executor.rs` as a new module

---

## Milestone

```
jsh> echo hello world
hello world
jsh> whoami
jswan
jsh> ls
Cargo.toml  docs  src  target
jsh> nonexistent
jsh: command not found: nonexistent
jsh> exit
```

(Note: `exit` won't work yet — that's a builtin, Module 4!)

---

## What's next?

Module 4 adds **built-in commands** — commands like `cd`, `exit`, and `pwd` that must run inside the shell process itself, not as external programs.
