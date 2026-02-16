# Module 9: Signal Handling

## What are we building?

Right now, pressing Ctrl-C kills your shell (or at best, the `ctrlc` crate prints a newline). We need surgical precision: Ctrl-C should kill the **foreground job** but leave the shell alive. Ctrl-Z should stop the foreground job and return you to the prompt. And when a background job finishes, the shell should notice and report it. After this module, your shell will handle all of these correctly — with full signal support on Unix and sensible fallbacks on Windows.

---

## Concept 1: What are signals?

Signals are the operating system's way of delivering **asynchronous notifications** to a process. They interrupt whatever the process is currently doing and invoke a handler function.

Think of signals like a tap on the shoulder. The process is busy doing its work, and the OS says "hey, something happened." The process pauses, handles the signal, and then (usually) resumes where it left off.

### The most important Unix signals for a shell

| Signal | Number | Default Action | Trigger | Shell behavior |
|--------|--------|---------------|---------|----------------|
| **SIGINT** | 2 | Terminate | Ctrl-C | Kill foreground job, not the shell |
| **SIGTSTP** | 20 | Stop (freeze) | Ctrl-Z | Stop foreground job, return to prompt |
| **SIGCHLD** | 17 | Ignore | Child exits/stops | Reap background jobs, notify user |
| **SIGQUIT** | 3 | Core dump | Ctrl-\\ | Kill foreground job with core dump |
| **SIGHUP** | 1 | Terminate | Terminal closed | Send SIGHUP to all jobs, then exit |
| **SIGPIPE** | 13 | Terminate | Write to broken pipe | Ignore (let commands handle it) |
| **SIGTERM** | 15 | Terminate | `kill` command | Clean shutdown of the shell |
| **SIGCONT** | 18 | Continue | `fg`/`bg` command | Resume a stopped process |

### How signals are delivered

```
  Terminal                          OS Kernel                      Process
  ────────                          ─────────                      ───────
  User presses Ctrl-C
        │
        ▼
  Terminal driver detects            Kernel identifies the
  interrupt character ──────────►   foreground process group ─────► SIGINT delivered
                                                                    to every process
                                                                    in the group
```

The terminal driver translates keystrokes into signals. Ctrl-C becomes SIGINT, Ctrl-Z becomes SIGTSTP, Ctrl-\\ becomes SIGQUIT. The kernel then delivers the signal to the **foreground process group** — not to a single process, but to every process in the group. This is why a pipeline like `cat | sort | uniq` is killed by a single Ctrl-C: all three processes are in the same foreground process group.

---

## Concept 2: Signal disposition — what happens when a signal arrives

Every signal has a **disposition** — a rule for what happens when it is delivered. There are three possible dispositions:

1. **Default** — the kernel's built-in behavior (usually terminate or ignore)
2. **Ignore** — the signal is silently discarded
3. **Catch** — a user-defined handler function runs

A shell needs to carefully set the disposition for each signal:

```
Signal    Shell's disposition          Why
──────    ────────────────────         ───
SIGINT    Ignore (in shell itself)     Shell must survive Ctrl-C
SIGTSTP   Ignore (in shell itself)     Shell must not stop itself
SIGCHLD   Catch (handler)              To reap background jobs
SIGQUIT   Ignore (in shell itself)     Shell must survive Ctrl-\
SIGHUP    Catch (handler)              Forward to jobs, then exit
SIGPIPE   Ignore                       Shell shouldn't die on broken pipe
```

But here is the critical part: **child processes should NOT inherit the shell's signal dispositions**. When the shell spawns `ls`, that `ls` process should get the **default** handlers back. Otherwise, `ls` would also ignore Ctrl-C, which is wrong.

```
Shell process (SIGINT → Ignore)
    │
    │ fork()
    ▼
Child process (SIGINT → Ignore)   ← BAD! Child inherited "Ignore"
    │
    │ Reset to default BEFORE exec()
    ▼
Child process (SIGINT → Default)  ← GOOD! Ctrl-C will kill the child
    │
    │ exec("ls")
    ▼
ls is running with default signal handlers
```

In Rust, you reset child signal dispositions using `pre_exec`:

```rust
#[cfg(unix)]
fn spawn_with_default_signals(cmd: &ParsedCommand) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;

    unsafe {
        Command::new(&cmd.program)
            .args(&cmd.args)
            .pre_exec(|| {
                // Reset signal handlers to default for the child
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                libc::signal(libc::SIGQUIT, libc::SIG_DFL);
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);

                // Put the child in its own process group
                libc::setpgid(0, 0);

                Ok(())
            })
            .spawn()
    }
}
```

`pre_exec` runs in the child process **after** `fork()` but **before** `exec()`. It is the perfect place to reset signal dispositions and set up process groups.

---

## Concept 3: The `ctrlc` crate vs `signal-hook`

We have been using the `ctrlc` crate since Module 1. It is simple and cross-platform, but limited:

```rust
// ctrlc — simple, handles only Ctrl-C
ctrlc::set_handler(|| {
    print!("\n");
})
.expect("Error setting Ctrl-C handler");
```

For a real shell, we need more control. The `signal-hook` crate handles **any** Unix signal and offers multiple strategies for reacting to them:

```toml
# Cargo.toml
[dependencies]
signal-hook = "0.3"

[target.'cfg(unix)'.dependencies]
signal-hook = "0.3"
```

### Comparison

| Feature | `ctrlc` | `signal-hook` |
|---------|---------|---------------|
| Ctrl-C (SIGINT) | Yes | Yes |
| SIGTSTP, SIGCHLD, etc. | No | Yes |
| Windows support | Yes (Ctrl-C, Ctrl-Break) | Partial (via `signal-hook-registry`) |
| Multiple signals | No | Yes |
| Signal iteration (fd-based) | No | Yes |
| Complexity | Very low | Medium |

### Strategy for james-shell

```rust
// Use signal-hook on Unix for full signal control
#[cfg(unix)]
fn setup_signal_handlers() { /* signal-hook */ }

// Keep ctrlc on Windows for Ctrl-C / Ctrl-Break
#[cfg(windows)]
fn setup_signal_handlers() { /* ctrlc crate */ }
```

---

## Concept 4: SIGINT (Ctrl-C) — kill the foreground job, not the shell

This is the most visible signal behavior in a shell. The desired behavior:

1. User is running `sleep 60` in the foreground
2. User presses Ctrl-C
3. `sleep 60` is killed (receives SIGINT, default action is terminate)
4. Shell is **not** killed (it is ignoring SIGINT)
5. Shell prints a new prompt

### Unix implementation with process groups

The key insight: the terminal sends SIGINT to the **foreground process group**. If the shell is in a **different** process group from the foreground job, the signal goes to the job, not the shell.

```rust
#[cfg(unix)]
use signal_hook::consts::SIGINT;
use signal_hook::iterator::Signals;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn setup_signals_unix() -> Arc<AtomicBool> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let i = interrupted.clone();

    // Ignore SIGINT in the shell itself
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
    }

    // Alternative: use signal-hook to set a flag
    // (useful if you want to detect Ctrl-C between commands)
    signal_hook::flag::register(SIGINT, i)
        .expect("Failed to register SIGINT handler");

    interrupted
}
```

With process groups set up correctly (Module 8), Ctrl-C works automatically:

```
Terminal sends SIGINT to foreground process group
    │
    ├──► sleep (PID 12345, PGID 12345) ← in foreground group → KILLED
    │
    └──► shell (PID 10000, PGID 10000) ← different group     → NOT affected
```

The shell does not need to do anything special to survive Ctrl-C — it just needs to not be in the foreground process group. The `tcsetpgrp()` call in Module 8 handles this.

### What if there is no foreground job?

When the user is typing at the prompt and presses Ctrl-C, there is no foreground process group to receive the signal. In this case, the shell should:

1. Cancel the current input line
2. Print a newline
3. Show a fresh prompt

```rust
fn handle_sigint_at_prompt(&mut self) {
    // Check if the interrupted flag was set
    if self.interrupted.load(Ordering::Relaxed) {
        self.interrupted.store(false, Ordering::Relaxed);
        println!();   // newline to move past the ^C
        // The REPL loop will print a new prompt
    }
}
```

---

## Concept 5: SIGTSTP (Ctrl-Z) — stop the foreground job

SIGTSTP is similar to SIGINT, but instead of killing the process, it **stops** (freezes) it. The process remains in memory but does not execute. It can be resumed later with SIGCONT.

### Shell's role

1. The shell ignores SIGTSTP (so it does not stop itself)
2. The foreground job receives SIGTSTP (default action: stop)
3. The shell's `waitpid()` returns with `WIFSTOPPED` status
4. The shell moves the job into the job table as `Stopped`
5. The shell prints `[1] Stopped  sleep 60`
6. The shell shows a new prompt

```rust
#[cfg(unix)]
fn setup_shell_signals() {
    unsafe {
        // Shell ignores these — children will get defaults via pre_exec
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}
```

The `waitpid` integration (from Module 8, Concept 9) detects the stopped status:

```rust
#[cfg(unix)]
fn wait_foreground(&mut self, pid: u32, child: Child, command: String) {
    let mut status: i32 = 0;
    loop {
        let result = unsafe {
            libc::waitpid(pid as i32, &mut status, libc::WUNTRACED)
        };

        if result < 0 {
            // Error — check errno
            break;
        }

        if libc::WIFEXITED(status) {
            // Normal exit
            self.last_exit_code = libc::WEXITSTATUS(status);
            break;
        }

        if libc::WIFSIGNALED(status) {
            // Killed by a signal (e.g., SIGINT from Ctrl-C)
            let sig = libc::WTERMSIG(status);
            self.last_exit_code = 128 + sig;
            if sig == libc::SIGINT {
                println!();  // newline after ^C
            }
            break;
        }

        if libc::WIFSTOPPED(status) {
            // Stopped by a signal (e.g., SIGTSTP from Ctrl-Z)
            let sig = libc::WSTOPSIG(status);
            let job_id = self.add_job_with_status(child, command, JobStatus::Stopped);
            println!("\n[{}]  Stopped  {}", job_id, self.jobs[&job_id].command);
            self.last_exit_code = 128 + sig;
            return; // Don't drop the child — it's in the job table now
        }
    }

    // Take back terminal control
    unsafe {
        libc::tcsetpgrp(libc::STDIN_FILENO, libc::getpgrp());
    }
}
```

### The WIFSTOPPED / WIFEXITED / WIFSIGNALED macros

These macros decode the raw status integer from `waitpid`:

| Macro | Meaning | Follow-up |
|-------|---------|-----------|
| `WIFEXITED(status)` | Process called `exit()` or returned from `main` | `WEXITSTATUS(status)` gives the exit code |
| `WIFSIGNALED(status)` | Process was killed by a signal | `WTERMSIG(status)` gives the signal number |
| `WIFSTOPPED(status)` | Process was stopped by a signal | `WSTOPSIG(status)` gives the signal number |
| `WIFCONTINUED(status)` | Process was resumed by SIGCONT | (informational) |

These exist because `waitpid` packs multiple pieces of information into a single `i32`. The macros extract the relevant bits.

---

## Concept 6: SIGCHLD — background job notification

When a child process exits or stops, the kernel sends **SIGCHLD** to the parent. This is how the shell learns that a background job has finished.

### Approach 1: Polling with `try_wait()` (simple, cross-platform)

This is what we implemented in Module 8 — check before each prompt:

```rust
fn reap_jobs(&mut self) {
    for (_, job) in self.jobs.iter_mut() {
        if job.status == JobStatus::Running {
            if let Ok(Some(status)) = job.child.try_wait() {
                job.status = JobStatus::Done(status.code().unwrap_or(1));
                println!("[{}]  Done  {}", job.id, job.command);
            }
        }
    }
    // ... remove done jobs ...
}
```

**Pros:** Works on all platforms. Simple to implement.
**Cons:** Only checks at the prompt. If you are running a long foreground command, you will not see the notification until it finishes.

### Approach 2: SIGCHLD handler (Unix, immediate notification)

For immediate notification, handle SIGCHLD asynchronously:

```rust
#[cfg(unix)]
fn setup_sigchld_handler() {
    use signal_hook::consts::SIGCHLD;
    use signal_hook::iterator::Signals;
    use std::thread;

    let mut signals = Signals::new(&[SIGCHLD])
        .expect("Failed to register SIGCHLD");

    thread::spawn(move || {
        for _sig in signals.forever() {
            // SIGCHLD received — a child changed state
            // We can't directly call reap_jobs() here because
            // we don't have &mut self. Instead, set a flag.
            SIGCHLD_RECEIVED.store(true, Ordering::Relaxed);
        }
    });
}

static SIGCHLD_RECEIVED: AtomicBool = AtomicBool::new(false);
```

Then check the flag at strategic points:

```rust
fn check_sigchld(&mut self) {
    if SIGCHLD_RECEIVED.swap(false, Ordering::Relaxed) {
        self.reap_jobs();
    }
}
```

### Why we cannot call `reap_jobs()` directly from a signal handler

Signal handlers run in an **interrupt context**. They can fire at any time — even in the middle of allocating memory, writing to stdout, or modifying the job table. If the handler tries to do any of those things, you get **undefined behavior** (data corruption, deadlocks, crashes).

This is the concept of **async-signal-safety** (see Concept 8). The safe pattern is:

1. Signal handler sets a flag (atomic write — always safe)
2. Main loop checks the flag periodically
3. Main loop does the actual work (reaping, printing, etc.)

---

## Concept 7: Other important signals

### SIGHUP — terminal hangup

Sent when the terminal is closed (e.g., closing an SSH session or a terminal window). A shell should:

1. Send SIGHUP to all jobs (so they can clean up)
2. Send SIGCONT to stopped jobs (so they can receive the SIGHUP)
3. Exit

```rust
#[cfg(unix)]
fn handle_sighup(&mut self) {
    // Send SIGHUP then SIGCONT to all jobs
    for (_, job) in &self.jobs {
        unsafe {
            libc::kill(job.pid as i32, libc::SIGHUP);
            libc::kill(job.pid as i32, libc::SIGCONT);
        }
    }

    // Exit the shell
    std::process::exit(0);
}
```

The reason we send SIGCONT after SIGHUP: a stopped process cannot handle signals until it is resumed. Without SIGCONT, a stopped job would never see the SIGHUP.

### SIGQUIT (Ctrl-\\)

Similar to SIGINT but produces a core dump. The shell ignores it; children get the default handler. Rarely used in practice but important for completeness.

### SIGPIPE

Sent when a process writes to a pipe whose reading end has been closed. Example: `yes | head -1` — after `head` reads one line and exits, `yes` gets SIGPIPE on its next write. The shell should ignore SIGPIPE so that a broken pipe in a pipeline does not crash the shell.

```rust
#[cfg(unix)]
unsafe {
    libc::signal(libc::SIGPIPE, libc::SIG_IGN);
}
```

---

## Concept 8: Async-signal-safety

This concept is critical and often misunderstood. A function is **async-signal-safe** if it can be called from within a signal handler without causing undefined behavior.

### What is NOT safe in a signal handler

- `malloc` / `free` (memory allocation) — Rust's `Vec::push`, `HashMap::insert`, `String::new`, `Box::new`, etc.
- `printf` / `println!` — uses internal buffers that could be locked
- Locking a `Mutex` — could deadlock if the signal interrupted code that holds the lock
- Anything that allocates or uses global mutable state

### What IS safe in a signal handler

- Writing to an `AtomicBool` or `AtomicI32`
- Calling `write()` system call directly (not buffered I/O)
- Calling `_exit()` (not `exit()`)
- Setting a global flag variable (if atomic)

### The `signal-hook` crate's approach

`signal-hook` provides several safe patterns:

```rust
use signal_hook::consts::{SIGINT, SIGCHLD, SIGTSTP};
use std::sync::atomic::{AtomicBool, Ordering};

// Pattern 1: Flag-based (safest, simplest)
let sigint_received = Arc::new(AtomicBool::new(false));
signal_hook::flag::register(SIGINT, sigint_received.clone())?;

// Check the flag in your main loop:
if sigint_received.swap(false, Ordering::Relaxed) {
    // Handle SIGINT
}
```

```rust
// Pattern 2: Self-pipe trick (advanced)
// signal-hook writes a byte to a pipe when a signal arrives.
// You can poll/select on the pipe fd in your event loop.
use signal_hook::low_level::pipe;

let (read_fd, write_fd) = nix::unistd::pipe()?;
pipe::register(SIGCHLD, write_fd)?;

// Now you can poll read_fd alongside stdin for multiplexed I/O
```

```rust
// Pattern 3: Iterator-based (runs in a separate thread)
use signal_hook::iterator::Signals;

let mut signals = Signals::new(&[SIGCHLD, SIGHUP])?;

std::thread::spawn(move || {
    for sig in signals.forever() {
        match sig {
            SIGCHLD => { /* set a flag */ }
            SIGHUP  => { /* set a flag */ }
            _ => {}
        }
    }
});
```

---

## Concept 9: Windows — console control events

Windows does not have Unix signals. Instead, it has **console control events**. There are only a few:

| Event | Trigger | Rough Unix equivalent |
|-------|---------|----------------------|
| `CTRL_C_EVENT` | Ctrl-C | SIGINT |
| `CTRL_BREAK_EVENT` | Ctrl-Break | SIGQUIT |
| `CTRL_CLOSE_EVENT` | Console window closed | SIGHUP |
| `CTRL_LOGOFF_EVENT` | User logs off | SIGHUP |
| `CTRL_SHUTDOWN_EVENT` | System shutting down | SIGTERM |

### Using the `ctrlc` crate on Windows

The `ctrlc` crate handles `CTRL_C_EVENT` cross-platform. For a shell, this is sufficient for basic Ctrl-C handling:

```rust
#[cfg(windows)]
fn setup_signal_handlers_windows() {
    // ctrlc handles CTRL_C_EVENT on Windows
    ctrlc::set_handler(|| {
        // On Windows, we can't forward SIGINT to a process group
        // because Windows doesn't have process groups in the same way.
        // Instead, we set a flag and let the main loop handle it.
        INTERRUPTED.store(true, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
```

### Windows `GenerateConsoleCtrlEvent`

On Windows, you can send Ctrl-C to a **process group** (yes, Windows has a concept of process groups for console events, though it is different from Unix):

```rust
#[cfg(windows)]
fn send_ctrl_c_to_process(pid: u32) {
    use windows_sys::Win32::System::Console::GenerateConsoleCtrlEvent;
    use windows_sys::Win32::System::Console::CTRL_C_EVENT;

    unsafe {
        // Send Ctrl-C to the process group identified by pid
        GenerateConsoleCtrlEvent(CTRL_C_EVENT, pid);
    }
}
```

However, this has a significant caveat: `GenerateConsoleCtrlEvent` sends the event to **all processes attached to the same console**, which includes the shell itself. To work around this, you need to:

1. Create the child process with the `CREATE_NEW_PROCESS_GROUP` flag
2. Temporarily disable your own Ctrl-C handler
3. Send the event
4. Re-enable your handler

This is messy. The practical approach for james-shell:

```rust
#[cfg(windows)]
fn run_foreground_windows(&mut self, cmd: &ParsedCommand) {
    // On Windows, let Ctrl-C propagate naturally to the child.
    // The child and shell share the same console, so Ctrl-C goes to both.
    // Our ctrlc handler prevents the shell from dying.
    match Command::new(&cmd.program)
        .args(&cmd.args)
        .status()
    {
        Ok(status) => {
            self.last_exit_code = status.code().unwrap_or(1);
        }
        Err(e) => {
            eprintln!("jsh: {}: {}", cmd.program, e);
            self.last_exit_code = 127;
        }
    }
}
```

---

## Concept 10: Signal masking during critical sections

Sometimes the shell needs to do something that must not be interrupted by a signal. For example, when modifying the job table, a SIGCHLD arriving mid-modification could corrupt data (if the handler also touches the job table).

**Signal masking** temporarily blocks specific signals from being delivered. The signals are held in a queue and delivered when unmasked.

```rust
#[cfg(unix)]
fn with_signals_blocked<F, R>(signals: &[i32], f: F) -> R
where
    F: FnOnce() -> R,
{
    use std::mem::MaybeUninit;

    unsafe {
        // Create a signal set with the signals we want to block
        let mut block_set: libc::sigset_t = MaybeUninit::zeroed().assume_init();
        libc::sigemptyset(&mut block_set);
        for &sig in signals {
            libc::sigaddset(&mut block_set, sig);
        }

        // Block them, saving the old mask
        let mut old_set: libc::sigset_t = MaybeUninit::zeroed().assume_init();
        libc::sigprocmask(libc::SIG_BLOCK, &block_set, &mut old_set);

        // Run the critical section
        let result = f();

        // Restore the old signal mask
        libc::sigprocmask(libc::SIG_SETMASK, &old_set, std::ptr::null_mut());

        result
    }
}

// Usage:
with_signals_blocked(&[libc::SIGCHLD, libc::SIGINT], || {
    // Safe to modify the job table here — no signals will interrupt us
    self.jobs.insert(id, job);
});
```

In practice, if you use the flag-based approach (set an atomic flag in the handler, check it in the main loop), you often do not need signal masking. The flag approach naturally serializes signal handling into the main loop. Masking is more important when you use raw `sigaction` handlers that directly modify shared state.

---

## Concept 11: Putting it all together — the cross-platform signal strategy

Here is the complete signal setup for james-shell:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Global flags for signal notification
static SIGCHLD_RECEIVED: AtomicBool = AtomicBool::new(false);

struct Shell {
    interrupted: Arc<AtomicBool>,
    // ... jobs, last_exit_code, etc.
}

impl Shell {
    fn new() -> Self {
        let interrupted = Arc::new(AtomicBool::new(false));
        setup_signal_handlers(interrupted.clone());

        Shell {
            interrupted,
            // ...
        }
    }
}

#[cfg(unix)]
fn setup_signal_handlers(interrupted: Arc<AtomicBool>) {
    use signal_hook::consts::*;

    // Shell ignores SIGINT, SIGTSTP, SIGQUIT, SIGPIPE
    // (Children will get default handlers via pre_exec)
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    // Use signal-hook for SIGCHLD (to reap background jobs)
    signal_hook::flag::register(signal_hook::consts::SIGCHLD, Arc::new(AtomicBool::new(false)))
        .expect("Failed to register SIGCHLD handler");

    // We also register a SIGINT flag for detecting Ctrl-C at the prompt
    // (even though the shell ignores SIGINT, the flag is set by signal-hook
    //  before the disposition takes effect)
    signal_hook::flag::register(signal_hook::consts::SIGINT, interrupted)
        .expect("Failed to register SIGINT flag");
}

#[cfg(windows)]
fn setup_signal_handlers(interrupted: Arc<AtomicBool>) {
    let i = interrupted;
    ctrlc::set_handler(move || {
        i.store(true, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");
}
```

### The REPL loop with signal awareness

```rust
impl Shell {
    fn run(&mut self) {
        loop {
            // Check for completed background jobs
            self.check_and_reap_jobs();

            // Check if Ctrl-C was pressed at the prompt
            self.interrupted.store(false, Ordering::Relaxed);

            // Show prompt
            print!("jsh> ");
            io::stdout().flush().unwrap();

            // Read input
            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(0) => {
                    println!("\nGoodbye!");
                    break;
                }
                Ok(_) => {
                    // Check if Ctrl-C was pressed during read_line
                    if self.interrupted.swap(false, Ordering::Relaxed) {
                        println!(); // newline after ^C
                        continue;   // skip to new prompt
                    }
                }
                Err(e) => {
                    // read_line can be interrupted by a signal on Unix
                    if e.kind() == io::ErrorKind::Interrupted {
                        println!(); // Ctrl-C during read
                        continue;
                    }
                    eprintln!("jsh: read error: {}", e);
                    break;
                }
            }

            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            let cmd = self.parse(input);
            self.execute(&cmd);
        }

        // Shell is exiting — send SIGHUP to all jobs
        self.send_hup_to_all_jobs();
    }

    #[cfg(unix)]
    fn send_hup_to_all_jobs(&self) {
        for (_, job) in &self.jobs {
            unsafe {
                libc::kill(job.pid as i32, libc::SIGHUP);
                libc::kill(job.pid as i32, libc::SIGCONT);
            }
        }
    }

    #[cfg(windows)]
    fn send_hup_to_all_jobs(&self) {
        // No equivalent on Windows — background processes will keep running
        // (or be killed when the console closes)
    }
}
```

### Spawning children with correct signal setup

```rust
#[cfg(unix)]
fn spawn_foreground_child(&self, cmd: &ParsedCommand) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;

    unsafe {
        Command::new(&cmd.program)
            .args(&cmd.args)
            .pre_exec(|| {
                // 1. Reset signal handlers to default
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                libc::signal(libc::SIGQUIT, libc::SIG_DFL);
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);

                // 2. Create a new process group
                libc::setpgid(0, 0);

                Ok(())
            })
            .spawn()
    }
}

#[cfg(windows)]
fn spawn_foreground_child(&self, cmd: &ParsedCommand) -> io::Result<Child> {
    Command::new(&cmd.program)
        .args(&cmd.args)
        .spawn()
}
```

---

## Concept 12: `signal` vs `sigaction` — why `sigaction` is better

On Unix, there are two APIs for setting signal handlers:

### `signal()` — the old way

```c
signal(SIGINT, SIG_IGN);  // ignore
signal(SIGINT, SIG_DFL);  // default
signal(SIGINT, handler);  // custom handler
```

Problems with `signal()`:
- **Resets to default after each delivery** on some systems (you must reinstall the handler inside the handler itself)
- **Race condition** between signal delivery and reinstallation
- **Cannot block other signals** during handler execution
- Behavior varies between Unix variants

### `sigaction()` — the modern way

```c
struct sigaction sa;
sa.sa_handler = handler;
sa.sa_flags = SA_RESTART;  // restart interrupted system calls
sigemptyset(&sa.sa_mask);
sigaddset(&sa.sa_mask, SIGCHLD);  // block SIGCHLD during handler

sigaction(SIGINT, &sa, NULL);
```

Advantages:
- **Handler stays installed** — no resetting, no race conditions
- **Can specify which signals to block** during handler execution
- **SA_RESTART flag** — automatically restarts interrupted system calls (like `read()`)
- **Consistent behavior** across all Unix systems

In Rust, `signal-hook` uses `sigaction` internally, so you get the correct behavior automatically. If you need raw `sigaction`, use the `nix` crate:

```rust
#[cfg(unix)]
fn setup_with_sigaction() {
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal};

    let handler = SigHandler::Handler(sigint_handler);
    let flags = SaFlags::SA_RESTART;
    let mask = SigSet::empty();
    let action = SigAction::new(handler, flags, mask);

    unsafe {
        sigaction(Signal::SIGINT, &action)
            .expect("Failed to set SIGINT handler");
    }
}

extern "C" fn sigint_handler(_sig: i32) {
    // Only async-signal-safe operations here!
    // Set a flag, write to a pipe, etc.
}
```

### SA_RESTART explained

When a signal interrupts a blocking system call (like `read()` waiting for keyboard input), the system call can either:

1. **Fail with EINTR** (Interrupted) — you must retry it manually
2. **Automatically restart** — as if the signal never happened

Without `SA_RESTART`, every `read_line()` call needs a retry loop:

```rust
// Without SA_RESTART — must handle EINTR
loop {
    match io::stdin().read_line(&mut input) {
        Ok(n) => break,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
        Err(e) => return Err(e),
    }
}
```

With `SA_RESTART`, the kernel retries `read()` automatically, and your code stays simple. `signal-hook` uses `SA_RESTART` by default.

---

## Key Rust concepts used

- **`std::sync::atomic::AtomicBool`** — lock-free boolean flag for cross-thread/signal communication
- **`Arc<AtomicBool>`** — shared ownership of atomic flag between main thread and signal handler
- **`Ordering::Relaxed`** — sufficient for simple flag-setting (no memory ordering guarantees needed)
- **`#[cfg(unix)]` / `#[cfg(windows)]`** — conditional compilation for platform-specific signal code
- **`unsafe` blocks** — required for `libc::signal`, `libc::kill`, `libc::sigprocmask`
- **`extern "C" fn`** — C-compatible function signature required for signal handlers
- **`std::os::unix::process::CommandExt::pre_exec`** — runs code in the child between fork and exec
- **`io::ErrorKind::Interrupted`** — Rust's representation of EINTR

---

## Milestone

### On Unix (Linux/Mac):

```
jsh> sleep 30
^C                                ← Ctrl-C kills sleep, not the shell
jsh> sleep 60 &
[1] 55210
jsh> sleep 60
^Z                                ← Ctrl-Z stops sleep
[1]  Stopped  sleep 60
jsh> jobs
[1]  Stopped  sleep 60
[2]  Running  sleep 60
jsh> bg 1
[1]  sleep 60 &
jsh> jobs
[1]  Running  sleep 60
[2]  Running  sleep 60
jsh>                              ← (wait a while, press Enter)
[1]  Done  sleep 60
[2]  Done  sleep 60
jsh> ^C                           ← Ctrl-C at prompt just gives a new line
jsh>
```

### On Windows:

```
jsh> sleep 30
^C                                ← Ctrl-C interrupts sleep (may also print ^C)
jsh> sleep 60 &
[1] 13720
jsh> jobs
[1]  Running  sleep 60
jsh> wait
[1]  Done  sleep 60
jsh> ^C                           ← Ctrl-C at prompt, new line
jsh>
```

Note: Ctrl-Z stop/resume is not available on Windows. The `fg` and `bg` builtins will print an informative message:

```
jsh> bg 1
bg: job suspension is not supported on Windows
```

---

## What's next?

Module 10 replaces our basic `read_line()` input with a proper **line editor** — arrow keys for cursor movement, up/down for command history, Tab for completion, and persistent history across sessions. This transforms the shell from functional to genuinely pleasant to use.
