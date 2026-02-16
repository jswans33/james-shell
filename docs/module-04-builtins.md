# Module 4: Built-in Commands

## What are we building?

Some commands **cannot** be external programs. They must run inside the shell process itself. These are called **builtins**. After this module, your shell will have `cd`, `pwd`, `exit`, `echo`, `export`, `unset`, and `type`.

---

## Concept 1: Why builtins exist

Consider `cd /tmp`. If `cd` were an external program, here's what would happen:

1. Shell forks a child process
2. Child runs `cd /tmp` — the child's working directory changes to `/tmp`
3. Child exits
4. Shell is still in the original directory — **nothing changed!**

A child process **cannot change its parent's state**. The child gets a *copy* of the environment, and any changes it makes die with it.

This is why `cd` must be a builtin — it needs to call `std::env::set_current_dir()` directly inside the shell process.

The same logic applies to:
- **`export`** — modifies the shell's environment variables
- **`exit`** — terminates the shell process itself
- **`unset`** — removes environment variables from the shell

---

## Concept 2: The dispatch pattern

When the user types a command, the shell must decide *how* to run it:

```
User types "foo" →  Is "foo" a builtin?
                        ├── YES → run the builtin function directly
                        └── NO  → search PATH for an external program
                                    ├── FOUND → run it as a child process
                                    └── NOT FOUND → "command not found"
```

In code, this looks like:

```rust
fn execute(&mut self, cmd: &Command) {
    match cmd.program.as_str() {
        "cd"     => self.builtin_cd(&cmd.args),
        "pwd"    => self.builtin_pwd(),
        "exit"   => self.builtin_exit(&cmd.args),
        "echo"   => self.builtin_echo(&cmd.args),
        "export" => self.builtin_export(&cmd.args),
        "unset"  => self.builtin_unset(&cmd.args),
        "type"   => self.builtin_type(&cmd.args),
        _        => self.run_external(cmd),
    }
}
```

Builtins are checked **first**. This means if you create a builtin called `echo`, it shadows the system's `/bin/echo`. That's exactly how bash works.

---

## Concept 3: Implementing each builtin

### `cd <directory>`

```rust
fn builtin_cd(&mut self, args: &[String]) {
    let target = match args.first() {
        Some(dir) => dir.clone(),
        None => {
            // cd with no args → go home
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))  // Windows fallback
                .unwrap_or_else(|_| ".".to_string())
        }
    };

    if let Err(e) = std::env::set_current_dir(&target) {
        eprintln!("cd: {}: {}", target, e);
    }
}
```

Key points:
- `cd` with no args goes to `$HOME` (Unix) or `%USERPROFILE%` (Windows)
- `set_current_dir` changes the **process's** working directory
- Errors (directory doesn't exist) are printed to **stderr** (`eprintln!`)

### `pwd`

```rust
fn builtin_pwd(&self) {
    match std::env::current_dir() {
        Ok(path) => println!("{}", path.display()),
        Err(e) => eprintln!("pwd: {}", e),
    }
}
```

### `exit [code]`

```rust
fn builtin_exit(&self, args: &[String]) {
    let code = args.first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
```

### `export VAR=value`

```rust
fn builtin_export(&mut self, args: &[String]) {
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            std::env::set_var(key, value);
        } else {
            // export VAR (no value) — just mark it for export
            // For now, we only handle VAR=value
            eprintln!("export: usage: export VAR=value");
        }
    }
}
```

### `type <command>`

`type` tells you what a command is — builtin or external (and where):

```rust
fn builtin_type(&self, args: &[String]) {
    for arg in args {
        match arg.as_str() {
            "cd" | "pwd" | "exit" | "echo" | "export" | "unset" | "type" => {
                println!("{} is a shell builtin", arg);
            }
            _ => {
                // Search PATH for the command
                match find_in_path(arg) {
                    Some(path) => println!("{} is {}", arg, path.display()),
                    None => println!("{}: not found", arg),
                }
            }
        }
    }
}
```

---

## Concept 4: PATH searching

For `type` and for "command not found" errors, we need to search PATH ourselves:

```rust
fn find_in_path(cmd: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    let separator = if cfg!(windows) { ';' } else { ':' };

    for dir in path_var.split(separator) {
        let full_path = std::path::Path::new(dir).join(cmd);
        if full_path.exists() {
            return Some(full_path);
        }
        // On Windows, also try with .exe extension
        if cfg!(windows) {
            let with_exe = full_path.with_extension("exe");
            if with_exe.exists() {
                return Some(with_exe);
            }
        }
    }
    None
}
```

Key points:
- PATH is separated by `:` on Unix and `;` on Windows
- `cfg!(windows)` is a compile-time check for the target OS
- Windows needs to check `.exe`, `.cmd`, `.bat` extensions

---

## Concept 5: stdout vs stderr

Notice that error messages use `eprintln!` (stderr) while normal output uses `println!` (stdout). This is important:

- **stdout** (fd 1) — normal program output, can be piped/redirected
- **stderr** (fd 2) — error messages, warnings, diagnostics

```bash
# This pipes stdout but errors still show on screen:
jsh> cd nonexistent 2>/dev/null    # hides the error
jsh> ls | grep foo                 # only stdout goes through the pipe
```

Getting this right now means redirection (Module 6) and pipes (Module 7) will work correctly later.

---

## Concept 6: Environment variable inheritance

When a shell runs an external command, the child process gets a **copy** of the shell's environment. This is how environment variables work:

```
Shell (PID 100)                    Child (PID 101)
├── PATH=/usr/bin:...    fork()    ├── PATH=/usr/bin:...  (copied)
├── HOME=/home/jswan   ────────→   ├── HOME=/home/jswan   (copied)
├── FOO=bar                        ├── FOO=bar            (copied)
│                                  │
│   (parent keeps going)           │   (runs program, then exits)
```

That's why `export FOO=bar` then running a program means the program can see `FOO`. But if the program changes `FOO`, the shell doesn't see it — the child's copy is independent.

---

## Key Rust concepts used

- **`match` on string slices** — dispatching to builtin functions
- **`std::env`** — `current_dir()`, `set_current_dir()`, `var()`, `set_var()`, `remove_var()`
- **`cfg!(windows)`** — conditional compilation for cross-platform code
- **`Option` chaining** — `.and_then()`, `.unwrap_or_else()`
- **`split_once()`** — splitting a string at the first occurrence of a delimiter
- **`PathBuf` and `Path`** — Rust's cross-platform path types

---

## Milestone

```
jsh> type cd
cd is a shell builtin
jsh> type ls
ls is /usr/bin/ls
jsh> pwd
/home/jswan/projects/james-shell
jsh> cd /tmp
jsh> pwd
/tmp
jsh> export GREETING=hello
jsh> echo $GREETING
$GREETING            ← Note: literal! Variable expansion is Module 5
jsh> exit
$                    ← back to your real shell
```

---

## What's next?

Module 5 adds **environment variable expansion** — turning `$HOME` into `/home/jswan` before the command runs. That's when `echo $GREETING` will actually work.
