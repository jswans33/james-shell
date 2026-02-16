# Building a Unix Shell from Scratch in Rust

A self-paced syllabus for learning systems programming by building a fully functional shell. Each module builds on the last, with a working (if incomplete) shell at every stage.

---

## Module 0: Foundations (Prerequisites)

**Goal:** Ensure Rust fundamentals are solid before diving into systems code.

**Topics:**
- Ownership, borrowing, lifetimes
- Error handling with `Result` and `?` operator
- Pattern matching and enums
- Traits and trait objects
- Working with `String` vs `&str`
- `std::io`, `std::fs`, `std::process` basics

**Checkpoint:** You can comfortably write a CLI tool that reads files, parses arguments, and handles errors gracefully.

**Resources:** *The Rust Programming Language* (Ch. 1–13), Rustlings exercises

---

## Module 1: The REPL Loop

**Goal:** A program that prints a prompt, reads a line, and echoes it back.

**Topics:**
- Reading from stdin line-by-line
- Flushing stdout (why `print!` without `\n` doesn't appear)
- Graceful EOF/Ctrl-D handling
- Basic signal awareness (Ctrl-C shouldn't kill your shell)

**Project milestone:**
```
> hello world
You typed: hello world
> ^D
Goodbye!
```

**Key concepts:** Buffered I/O, terminal raw vs cooked mode

---

## Module 2: Command Parsing & Tokenization

**Goal:** Turn a raw input string into structured command data.

**Topics:**
- Splitting on whitespace (the naive approach)
- Handling quoted strings (`echo "hello world"` is one argument)
- Escape characters (`echo hello\ world`)
- Designing a `Command` struct: program name, args, redirections, background flag
- Writing a proper tokenizer/lexer (state machine approach)

**Project milestone:** Parser that correctly handles:
```
> echo "hello   world" foo\ bar 'single quotes'
Command { program: "echo", args: ["hello   world", "foo bar", "single quotes"] }
```

**Key concepts:** Lexer state machines, iterator patterns in Rust

---

## Module 3: Process Execution

**Goal:** Actually run external programs.

**Topics:**
- `std::process::Command` — the high-level API (start here)
- Understanding `fork()` + `exec()` — the Unix model underneath
- `nix::unistd::fork()`, `nix::unistd::execvp()` — doing it yourself
- Parent waiting for child with `waitpid()`
- Exit status codes and reporting them
- `$?` equivalent — tracking last exit status

**Project milestone:** Your shell can run any program installed on the system:
```
> ls -la /tmp
(actual ls output appears)
> whoami
jswan
```

**Key concepts:** Process creation, exec family, wait semantics, zombie processes

---

## Module 4: Built-in Commands

**Goal:** Commands that *must* run inside the shell process, not as children.

**Topics:**
- Why `cd` can't be an external command (child process can't change parent's cwd)
- Implementing builtins: `cd`, `exit`, `pwd`, `export`, `unset`, `echo`, `type`, `which`
- Dispatching: check builtins first, then `$PATH` lookup
- `std::env::set_current_dir`, `std::env::set_var`

**Project milestone:**
```
> cd /tmp
> pwd
/tmp
> export FOO=bar
> echo $FOO
bar
```

**Key concepts:** Process environment inheritance, PATH resolution

---

## Module 5: Environment Variables & Expansion

**Goal:** Shell variable expansion before command execution.

**Topics:**
- `$VAR` and `${VAR}` expansion
- `$HOME`, `$PATH`, `$PWD`, `$?`, `$$`, `$0`
- Tilde expansion (`~` → home directory)
- Glob/wildcard expansion (`*.rs` → list of files)
- Word splitting after expansion
- The order of expansions (tilde → variable → glob → word split)

**Project milestone:**
```
> echo ~/*.toml
/home/jswan/Cargo.toml
> echo $HOME
/home/jswan
```

**Key concepts:** Expansion pipeline, glob patterns (`glob` crate)

---

## Module 6: I/O Redirection

**Goal:** Redirect stdin/stdout/stderr to and from files.

**Topics:**
- File descriptors: 0 (stdin), 1 (stdout), 2 (stderr)
- `>` (write), `>>` (append), `<` (read), `2>` (stderr redirect)
- `2>&1` — duplicating file descriptors
- `dup2()` system call — how redirection actually works
- Here documents (`<<EOF`) and here strings (`<<<`)
- `/dev/null` redirection

**Project milestone:**
```
> ls /nonexistent 2> errors.txt
> cat errors.txt
ls: cannot access '/nonexistent': No such file or directory
> wc -l < Cargo.toml
12
```

**Key concepts:** File descriptors, `dup2`, open/close semantics

---

## Module 7: Pipes

**Goal:** Connect stdout of one process to stdin of the next.

**Topics:**
- The `pipe()` system call — creates a pair of connected file descriptors
- Forking multiple children for a pipeline
- Closing unused pipe ends (critical to avoid hangs)
- Pipe chains: `cmd1 | cmd2 | cmd3`
- Waiting for all processes in a pipeline
- Pipeline exit status (last command, or `PIPESTATUS` array)

**Project milestone:**
```
> cat /etc/passwd | grep jswan | cut -d: -f1
jswan
> ls -la | sort -k5 -n | tail -5
(last 5 files by size)
```

**Key concepts:** `pipe()`, `dup2()`, closing FDs, process groups

---

## Module 8: Job Control

**Goal:** Background processes, job listing, fg/bg.

**Topics:**
- Background execution with `&`
- Process groups and session IDs (`setpgid`, `setsid`)
- The `jobs` builtin — tracking background processes
- `fg` and `bg` builtins
- `Ctrl-Z` (SIGTSTP) — stopping foreground jobs
- `wait` builtin
- Reaping completed background jobs (SIGCHLD handling)

**Project milestone:**
```
> sleep 30 &
[1] 12345
> jobs
[1]+ Running    sleep 30 &
> fg 1
sleep 30
^Z
[1]+ Stopped    sleep 30
> bg 1
[1]+ sleep 30 &
```

**Key concepts:** Process groups, terminal control, POSIX signals

---

## Module 9: Signal Handling

**Goal:** Properly handle Unix signals in the shell.

**Topics:**
- SIGINT (Ctrl-C) — kill foreground job, not shell
- SIGTSTP (Ctrl-Z) — stop foreground job
- SIGCHLD — background job finished
- SIGQUIT, SIGHUP, SIGPIPE
- `signal` vs `sigaction` — why sigaction is better
- Signal masking during critical sections
- The `signal-hook` or `nix` crate for Rust signal handling

**Project milestone:** Ctrl-C kills the running program but returns you to a prompt. Ctrl-Z stops a job. The shell itself never dies from these signals.

**Key concepts:** Signal disposition, async-signal-safety, signal masks

---

## Module 10: Line Editing & History

**Goal:** A usable interactive experience with readline-like behavior.

**Topics:**
- Terminal raw mode vs canonical mode (`termios`)
- Reading individual keystrokes
- Implementing cursor movement (left/right arrow, Home/End)
- Command history (up/down arrows)
- Persistent history file (`~/.shell_history`)
- Or: use the `rustyline` crate and understand what it does for you
- Tab completion — filename and command completion
- Custom completers (contextual completion based on position)

**Project milestone:** Full interactive editing with history, tab completion for files and commands, and persistent history across sessions.

**Key concepts:** Terminal modes, ANSI escape codes, trie data structures for completion

---

## Module 11: Control Flow & Scripting

**Goal:** Transform the shell from interactive-only to a scripting language.

**Topics:**
- Conditional execution: `&&` (and), `||` (or), `;` (sequence)
- `if`/`then`/`elif`/`else`/`fi` blocks
- `while`/`until`/`for` loops
- `case`/`esac` pattern matching
- Command substitution: `$(command)` and backticks
- Subshells: `(commands in subshell)`
- Functions: `name() { body; }`
- Local variables in functions
- Return values vs exit codes
- Script execution (`./script.sh`, shebang lines)
- The `source` / `.` builtin

**Project milestone:** Your shell can execute a script like:
```bash
#!/path/to/your/shell
for f in *.rs; do
    if grep -q "unsafe" "$f"; then
        echo "WARNING: $f uses unsafe"
    fi
done
```

**Key concepts:** AST design, recursive descent parsing, scope/environment chains

---

## Module 12: Advanced Features

**Goal:** Polish and power-user features.

**Topics:**
- Aliases and alias expansion
- Prompt customization (`PS1` equivalent, with escape codes for git branch, exit status, etc.)
- `trap` — user-defined signal handlers
- Arithmetic expansion: `$((1 + 2))`
- Process substitution: `<(command)` and `>(command)`
- Coprocesses
- `set` options (`-e`, `-x`, `-u`, `-o pipefail`)
- `ulimit` and resource management
- Startup files (`.shellrc`, `.profile` equivalents)
- `exec` builtin

**Project milestone:** A daily-drivable shell with your own custom prompt, aliases, and startup config.

---

## Module 13: Testing & Robustness

**Goal:** Make sure it actually works correctly.

**Topics:**
- Unit testing parsers and expanders
- Integration testing with `assert_cmd` and `predicates` crates
- POSIX compliance test suites
- Fuzzing input with `cargo-fuzz`
- Edge cases: empty input, massive input, binary data in pipes, deeply nested subshells
- Memory safety verification (Rust helps a lot here, but `unsafe` blocks need auditing)
- Benchmark against bash/zsh for pipeline throughput

**Project milestone:** A test suite with 200+ tests covering parsing, execution, redirection, pipes, job control, and scripting.

---

## Recommended Reading & Resources

**Books:**
- *Advanced Programming in the UNIX Environment* (Stevens & Rago) — the systems programming bible
- *The Linux Programming Interface* (Kerrisk) — comprehensive and modern
- *The Rust Programming Language* — for Rust fundamentals
- *Programming Rust* (Blandy, Orendorff, Tindall) — deeper Rust

**Code to Study:**
- [nushell](https://github.com/nushell/nushell) — a modern shell written in Rust (object-oriented pipelines)
- [starship](https://github.com/starship/starship) — cross-shell prompt in Rust (great for Module 12)
- [bash source](https://git.savannah.gnu.org/cgit/bash.git) — see how the real thing works (warning: it's C and it's dense)

**Key Crates:**
| Crate | Purpose |
|-------|---------|
| `nix` | Unix syscalls (fork, exec, pipe, signal, termios) |
| `rustyline` | Line editing, history, completion |
| `glob` | Wildcard/glob expansion |
| `shellexpand` | Tilde and variable expansion |
| `dirs` | Home/config directory resolution |
| `libc` | Raw FFI to libc when `nix` doesn't cover it |
| `signal-hook` | Ergonomic signal handling |
| `termion` or `crossterm` | Terminal manipulation |
| `assert_cmd` | Integration testing CLI apps |

---

## Suggested Timeline

| Phase | Modules | Estimated Time |
|-------|---------|---------------|
| **Phase 1: Walking** | 0–3 | 2–3 weeks |
| **Phase 2: Running** | 4–7 | 3–4 weeks |
| **Phase 3: Flying** | 8–10 | 3–4 weeks |
| **Phase 4: Scripting** | 11–12 | 4–6 weeks |
| **Phase 5: Hardening** | 13 | 2–3 weeks |

Total: ~14–20 weeks at a steady pace, longer if you're fitting it around other work.

---

*Each module should produce a tagged git commit so you can always go back and see the shell at each stage of evolution.*
