# Module 8: Job Control

## What are we building?

Up to now, every command our shell runs blocks the prompt until it finishes. Type `sleep 60` and you stare at a frozen terminal for a full minute. Real shells let you run commands **in the background**, switch between running tasks, pause them, and resume them. After this module, your shell will support `&` for background execution, the `jobs` builtin to list running tasks, `fg` and `bg` to move jobs between foreground and background, and Ctrl-Z to suspend a foreground job.

---

## Concept 1: Foreground vs background execution

In every previous module, we ran commands using `.status()`:

```rust
let status = Command::new("sleep")
    .arg("10")
    .status()       // blocks until sleep exits
    .expect("failed to execute");
```

`.status()` **blocks** — the shell sits there waiting. This is **foreground execution**.

**Background execution** means starting the process but **not waiting**. The shell immediately shows a new prompt while the process runs alongside it. In traditional shells, you request this by appending `&`:

```
jsh> sleep 30 &
[1] 12345
jsh>                   ← prompt appears immediately
```

The `[1]` is the **job number** (shell-level ID) and `12345` is the OS **process ID (PID)**.

To implement this, we switch from `.status()` to `.spawn()`:

```rust
// Foreground — blocks until done
let status = Command::new("sleep").arg("10").status()?;

// Background — returns immediately with a Child handle
let child = Command::new("sleep").arg("10").spawn()?;
// child is still running! We hold a handle to it.
```

The `Child` handle is our lifeline to the background process. We can check if it is still running, wait for it, or kill it.

---

## Concept 2: The job table

A shell needs to track all background (and stopped) jobs. We need a data structure for this — the **job table**.

### What goes in a job entry?

| Field | Type | Purpose |
|-------|------|---------|
| `id` | `usize` | Shell-assigned job number (1, 2, 3, ...) |
| `pid` | `u32` | OS process ID |
| `command` | `String` | The command text (for display in `jobs`) |
| `status` | `JobStatus` | Running, Stopped, or Done |
| `child` | `Child` | The `std::process::Child` handle |

```rust
use std::process::Child;

#[derive(Debug, PartialEq)]
enum JobStatus {
    Running,
    Stopped,
    Done(i32),     // exit code
}

struct Job {
    id: usize,
    pid: u32,
    command: String,
    status: JobStatus,
    child: Child,
}
```

### Where to store jobs

You have two main options:

```rust
// Option A: Vec — simple, job IDs are indices + 1
struct Shell {
    jobs: Vec<Option<Job>>,   // None = slot is free
    next_job_id: usize,
}

// Option B: HashMap — sparse, easy removal
use std::collections::HashMap;

struct Shell {
    jobs: HashMap<usize, Job>,
    next_job_id: usize,
}
```

A `HashMap<usize, Job>` is the cleaner choice for a shell because jobs get removed when they finish, and a `HashMap` handles sparse IDs naturally. With a `Vec`, you either waste memory with `None` slots or renumber jobs (which confuses users).

```rust
use std::collections::HashMap;

struct Shell {
    jobs: HashMap<usize, Job>,
    next_job_id: usize,
    // ... other fields from previous modules
}

impl Shell {
    fn new() -> Self {
        Shell {
            jobs: HashMap::new(),
            next_job_id: 1,
        }
    }

    fn add_job(&mut self, child: Child, command: String) -> usize {
        let id = self.next_job_id;
        let pid = child.id();
        self.jobs.insert(id, Job {
            id,
            pid,
            command,
            status: JobStatus::Running,
            child,
        });
        self.next_job_id += 1;
        id
    }
}
```

---

## Concept 3: Background execution with `&`

### Parser changes

First, the parser needs to detect `&` at the end of a command. You likely already have a `Command` struct from Module 2 — add a `background` flag:

```rust
struct ParsedCommand {
    program: String,
    args: Vec<String>,
    background: bool,   // NEW: true if the line ended with &
    // ... redirections from Module 6, etc.
}
```

In the parser, strip a trailing `&` token and set the flag:

```rust
fn parse(&self, input: &str) -> ParsedCommand {
    let mut tokens = self.tokenize(input);
    let mut background = false;

    // Check if last token is "&"
    if tokens.last().map(|t| t.as_str()) == Some("&") {
        background = true;
        tokens.pop();   // remove the & from the token list
    }

    ParsedCommand {
        program: tokens[0].clone(),
        args: tokens[1..].to_vec(),
        background,
    }
}
```

### Executor changes

The executor now branches on the `background` flag:

```rust
fn execute(&mut self, cmd: &ParsedCommand) {
    // ... check builtins first (Module 4) ...

    if cmd.background {
        self.run_background(cmd);
    } else {
        self.run_foreground(cmd);
    }
}

fn run_foreground(&mut self, cmd: &ParsedCommand) {
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

fn run_background(&mut self, cmd: &ParsedCommand) {
    match Command::new(&cmd.program)
        .args(&cmd.args)
        .spawn()
    {
        Ok(child) => {
            let job_id = self.add_job(child, cmd.to_string());
            let pid = self.jobs[&job_id].pid;
            println!("[{}] {}", job_id, pid);
        }
        Err(e) => {
            eprintln!("jsh: {}: {}", cmd.program, e);
        }
    }
}
```

The key difference: `.spawn()` returns a `Child` immediately. We store it in the job table and print the job ID and PID, just like bash does.

---

## Concept 4: The `jobs` builtin

The `jobs` builtin lists all tracked jobs and their current status:

```rust
fn builtin_jobs(&mut self) {
    // First, reap any completed jobs (update their status)
    self.reap_jobs();

    for (_, job) in self.jobs.iter().sorted_by_key(|(id, _)| **id) {
        let status_str = match &job.status {
            JobStatus::Running => "Running",
            JobStatus::Stopped => "Stopped",
            JobStatus::Done(code) => "Done",
        };
        println!("[{}]  {:10} {}", job.id, status_str, job.command);
    }
}
```

If you do not want to pull in an external crate for `.sorted_by_key()`, collect into a `Vec` and sort:

```rust
fn builtin_jobs(&mut self) {
    self.reap_jobs();

    let mut job_list: Vec<&Job> = self.jobs.values().collect();
    job_list.sort_by_key(|j| j.id);

    for job in job_list {
        let status_str = match &job.status {
            JobStatus::Running  => "Running   ",
            JobStatus::Stopped  => "Stopped   ",
            JobStatus::Done(_)  => "Done      ",
        };
        println!("[{}]  {} {}", job.id, status_str, job.command);
    }
}
```

---

## Concept 5: Reaping completed background jobs

When a background process finishes, it does not disappear automatically. On Unix, it becomes a **zombie** — a dead process whose exit status is waiting to be collected by its parent. On Windows, the process handle remains valid until you close it.

We need to **reap** (check and clean up) completed jobs. The right time to do this is:
1. Before printing a new prompt (so the user sees "[1] Done" messages)
2. When `jobs` is called
3. When `wait` is called

```rust
fn reap_jobs(&mut self) {
    let mut done_ids = Vec::new();

    for (id, job) in self.jobs.iter_mut() {
        if job.status == JobStatus::Running {
            // try_wait() checks without blocking
            match job.child.try_wait() {
                Ok(Some(status)) => {
                    // Process has exited
                    let code = status.code().unwrap_or(1);
                    job.status = JobStatus::Done(code);
                    done_ids.push(*id);
                    println!("[{}]  Done  {}", job.id, job.command);
                }
                Ok(None) => {
                    // Still running — do nothing
                }
                Err(e) => {
                    eprintln!("jsh: error checking job {}: {}", id, e);
                }
            }
        }
    }

    // Remove completed jobs from the table
    for id in done_ids {
        self.jobs.remove(&id);
    }
}
```

The critical method here is `child.try_wait()`:

| Return value | Meaning |
|-------------|---------|
| `Ok(Some(status))` | Process has exited, `status` contains exit code |
| `Ok(None)` | Process is still running |
| `Err(e)` | Something went wrong checking the process |

This is **non-blocking** — it returns immediately, unlike `.wait()` which blocks. We call it in a loop over all jobs to check who is done.

### Integrating with the REPL loop

```rust
loop {
    // Reap completed background jobs BEFORE showing the prompt
    self.reap_jobs();

    print!("jsh> ");
    io::stdout().flush().unwrap();

    // ... read input, parse, execute ...
}
```

---

## Concept 6: The `fg` builtin — bringing jobs to the foreground

`fg` takes a stopped or background job and makes it the foreground job. The shell then waits for it to finish.

```rust
fn builtin_fg(&mut self, args: &[String]) {
    let job_id = match args.first() {
        Some(s) => match s.parse::<usize>() {
            Ok(id) => id,
            Err(_) => {
                eprintln!("fg: invalid job id: {}", s);
                return;
            }
        },
        None => {
            // No argument: use the most recent job
            match self.jobs.keys().max().copied() {
                Some(id) => id,
                None => {
                    eprintln!("fg: no current job");
                    return;
                }
            }
        }
    };

    // Remove the job from the table — it's now foreground
    let mut job = match self.jobs.remove(&job_id) {
        Some(j) => j,
        None => {
            eprintln!("fg: {}: no such job", job_id);
            return;
        }
    };

    println!("{}", job.command);

    // If the job was stopped (Unix), send SIGCONT to resume it
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            libc::kill(job.pid as i32, libc::SIGCONT);
        }
    }

    // Now wait for it (blocking — it's foreground now)
    match job.child.wait() {
        Ok(status) => {
            self.last_exit_code = status.code().unwrap_or(1);
        }
        Err(e) => {
            eprintln!("fg: error waiting for job: {}", e);
        }
    }
}
```

### The Unix-specific part: SIGCONT

On Unix, when a process is **stopped** (via Ctrl-Z / SIGTSTP), it is frozen. You must send it **SIGCONT** to resume execution before you can wait for it. On Windows, the concept of "stopped" processes does not exist in the same way (more on this in Concept 9).

---

## Concept 7: The `bg` builtin — resuming jobs in the background

`bg` is simpler than `fg` — it resumes a stopped job but does **not** wait for it:

```rust
fn builtin_bg(&mut self, args: &[String]) {
    let job_id = match args.first() {
        Some(s) => match s.parse::<usize>() {
            Ok(id) => id,
            Err(_) => {
                eprintln!("bg: invalid job id: {}", s);
                return;
            }
        },
        None => {
            match self.most_recent_stopped_job() {
                Some(id) => id,
                None => {
                    eprintln!("bg: no current job");
                    return;
                }
            }
        }
    };

    match self.jobs.get_mut(&job_id) {
        Some(job) => {
            if job.status != JobStatus::Stopped {
                eprintln!("bg: job {} is not stopped", job_id);
                return;
            }

            // Resume the process (Unix only)
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(job.pid as i32, libc::SIGCONT);
                }
            }

            job.status = JobStatus::Running;
            println!("[{}]  {} &", job.id, job.command);
        }
        None => {
            eprintln!("bg: {}: no such job", job_id);
        }
    }
}

fn most_recent_stopped_job(&self) -> Option<usize> {
    self.jobs
        .iter()
        .filter(|(_, j)| j.status == JobStatus::Stopped)
        .max_by_key(|(id, _)| *id)
        .map(|(id, _)| *id)
}
```

Notice that `bg` keeps the job in the table (unlike `fg`, which removes it). The job continues running in the background, and `reap_jobs()` will eventually clean it up.

---

## Concept 8: The `wait` builtin

`wait` blocks until one or all background jobs finish:

```rust
fn builtin_wait(&mut self, args: &[String]) {
    if args.is_empty() {
        // Wait for ALL background jobs
        self.wait_all_jobs();
    } else {
        // Wait for a specific job
        for arg in args {
            match arg.parse::<usize>() {
                Ok(job_id) => self.wait_for_job(job_id),
                Err(_) => eprintln!("wait: invalid job id: {}", arg),
            }
        }
    }
}

fn wait_all_jobs(&mut self) {
    // Collect all running job IDs
    let running_ids: Vec<usize> = self.jobs
        .iter()
        .filter(|(_, j)| j.status == JobStatus::Running)
        .map(|(id, _)| *id)
        .collect();

    for id in running_ids {
        self.wait_for_job(id);
    }
}

fn wait_for_job(&mut self, job_id: usize) {
    let job = match self.jobs.get_mut(&job_id) {
        Some(j) => j,
        None => {
            eprintln!("wait: {}: no such job", job_id);
            return;
        }
    };

    if job.status != JobStatus::Running {
        return; // Already done or stopped
    }

    // This blocks until the job finishes
    match job.child.wait() {
        Ok(status) => {
            let code = status.code().unwrap_or(1);
            job.status = JobStatus::Done(code);
            self.last_exit_code = code;
            println!("[{}]  Done  {}", job.id, job.command);
        }
        Err(e) => {
            eprintln!("wait: error: {}", e);
        }
    }

    self.jobs.remove(&job_id);
}
```

---

## Concept 9: Ctrl-Z — stopping the foreground job

When the user presses Ctrl-Z, the OS sends **SIGTSTP** to the foreground process group. The process stops (freezes) and the shell regains control. This is implemented primarily on Unix using signals.

### The Unix approach: process groups

On Unix, the foreground job runs in its own **process group**. The terminal sends signals (SIGINT, SIGTSTP) to the entire foreground process group. The shell itself is in a different process group, so it is not affected.

```rust
#[cfg(unix)]
fn run_foreground_unix(&mut self, cmd: &ParsedCommand) {
    use std::os::unix::process::CommandExt;

    // spawn() instead of status() so we can handle SIGTSTP
    let result = unsafe {
        Command::new(&cmd.program)
            .args(&cmd.args)
            .pre_exec(|| {
                // Put the child in its own process group
                libc::setpgid(0, 0);
                Ok(())
            })
            .spawn()
    };

    match result {
        Ok(mut child) => {
            let pid = child.id();

            // Give the terminal to the child's process group
            unsafe {
                libc::tcsetpgrp(libc::STDIN_FILENO, pid as i32);
            }

            // Wait for the child
            match child.wait() {
                Ok(status) => {
                    // Take the terminal back
                    unsafe {
                        libc::tcsetpgrp(libc::STDIN_FILENO, libc::getpgrp());
                    }

                    if status.code().is_some() {
                        // Normal exit
                        self.last_exit_code = status.code().unwrap_or(1);
                    }
                }
                Err(e) => {
                    eprintln!("jsh: {}: {}", cmd.program, e);
                }
            }

            // Take the terminal back (in case we exited the match early)
            unsafe {
                libc::tcsetpgrp(libc::STDIN_FILENO, libc::getpgrp());
            }
        }
        Err(e) => {
            eprintln!("jsh: {}: {}", cmd.program, e);
        }
    }
}
```

### Detecting that the child was stopped (not exited)

On Unix, `child.wait()` can return with a status indicating the child was **stopped** by a signal, not that it exited. Rust's `ExitStatus` does not directly expose this, so you need platform-specific code:

```rust
#[cfg(unix)]
fn was_stopped(status: &std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    // If the child was stopped by a signal, signal() returns the signal number
    // and code() returns None
    status.stopped_signal().is_some()
}

#[cfg(unix)]
fn stopped_signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.stopped_signal()
}
```

When you detect that the child was stopped, move it into the job table as `Stopped`:

```rust
#[cfg(unix)]
{
    use std::os::unix::process::ExitStatusExt;
    if let Some(_sig) = status.stopped_signal() {
        // Child was stopped (Ctrl-Z)
        let job_id = self.add_job_stopped(child, cmd.to_string());
        println!("\n[{}]  Stopped  {}", job_id, cmd.to_string());
        return;
    }
}
```

However, there is a subtlety here. When a child is stopped by SIGTSTP, `child.wait()` on Unix actually uses `waitpid()` under the hood. By default, Rust's `Child::wait()` does **not** pass the `WUNTRACED` flag, which means it will not return when the child is stopped — it will only return when the child exits. To properly handle Ctrl-Z, you need to use `waitpid` directly with the `WUNTRACED` flag:

```rust
#[cfg(unix)]
fn wait_with_stop_detection(pid: u32) -> WaitResult {
    use libc::{waitpid, WUNTRACED, WIFSTOPPED, WIFEXITED, WEXITSTATUS, WSTOPSIG};

    let mut status: i32 = 0;
    let result = unsafe {
        waitpid(pid as i32, &mut status, WUNTRACED)
    };

    if result < 0 {
        return WaitResult::Error;
    }

    if WIFEXITED(status) {
        WaitResult::Exited(WEXITSTATUS(status))
    } else if WIFSTOPPED(status) {
        WaitResult::Stopped(WSTOPSIG(status))
    } else {
        WaitResult::Error
    }
}

enum WaitResult {
    Exited(i32),
    Stopped(i32),
    Error,
}
```

---

## Concept 10: Cross-platform challenges

Job control is one of the areas where Unix and Windows diverge most sharply.

### Comparison table

| Feature | Unix | Windows |
|---------|------|---------|
| Background processes | Fork + exec in new process group | `CreateProcess` (works fine) |
| Stopping a process (Ctrl-Z) | SIGTSTP signal, process freezes | No equivalent signal |
| Resuming a stopped process | SIGCONT signal | N/A |
| Process groups | `setpgid()`, `tcsetpgrp()` | Job objects (different model) |
| Reaping zombies | `waitpid()` with flags | `WaitForSingleObject` |
| Terminal ownership | `tcsetpgrp()` | Console attached to process |

### Strategy for james-shell

```rust
// Full job control on Unix, basic background execution on Windows

#[cfg(unix)]
fn run_foreground(&mut self, cmd: &ParsedCommand) {
    // Full implementation with process groups, SIGTSTP detection, etc.
    self.run_foreground_unix(cmd);
}

#[cfg(windows)]
fn run_foreground(&mut self, cmd: &ParsedCommand) {
    // Simple .status() — no Ctrl-Z support
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

Background execution (`&`) and the `jobs`, `wait` builtins work on **both** platforms because `.spawn()` and `.try_wait()` are cross-platform. The features that are Unix-only are:
- Ctrl-Z (stop/suspend)
- `fg` resuming a stopped process (SIGCONT)
- `bg` resuming a stopped process (SIGCONT)
- Process group management (`setpgid`, `tcsetpgrp`)

On Windows, `fg` can bring a running background job to the foreground (wait for it), but it cannot resume a stopped job because Windows does not have stopped jobs.

### Windows job objects (advanced)

Windows has a concept called **Job Objects** that group processes together. You can use them to manage a tree of processes (e.g., kill an entire pipeline). This is more advanced and not required for basic job control, but worth knowing about:

```rust
#[cfg(windows)]
fn create_job_object() {
    use windows_sys::Win32::System::JobObjects::*;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        // Assign a process to the job
        // AssignProcessToJobObject(job, process_handle);
        // Terminate all processes in the job
        // TerminateJobObject(job, exit_code);
    }
}
```

For james-shell, the practical approach is: implement full job control on Unix, and provide graceful degradation on Windows (background jobs work, but stop/resume does not).

---

## Concept 11: Putting it all together — the updated Shell struct

Here is the full picture of what the shell looks like after adding job control:

```rust
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Child, Command};

#[derive(Debug, PartialEq)]
enum JobStatus {
    Running,
    Stopped,
    Done(i32),
}

struct Job {
    id: usize,
    pid: u32,
    command: String,
    status: JobStatus,
    child: Child,
}

struct Shell {
    jobs: HashMap<usize, Job>,
    next_job_id: usize,
    last_exit_code: i32,
}

impl Shell {
    fn new() -> Self {
        Shell {
            jobs: HashMap::new(),
            next_job_id: 1,
            last_exit_code: 0,
        }
    }

    fn run(&mut self) {
        loop {
            // Reap finished background jobs
            self.reap_jobs();

            // Print prompt and read input
            print!("jsh> ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(0) => {
                    println!("\nGoodbye!");
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("jsh: read error: {}", e);
                    continue;
                }
            }

            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            let cmd = self.parse(input);
            self.execute(&cmd);
        }
    }

    fn execute(&mut self, cmd: &ParsedCommand) {
        match cmd.program.as_str() {
            "jobs"  => self.builtin_jobs(),
            "fg"    => self.builtin_fg(&cmd.args),
            "bg"    => self.builtin_bg(&cmd.args),
            "wait"  => self.builtin_wait(&cmd.args),
            // ... other builtins from Module 4 ...
            _ => {
                if cmd.background {
                    self.run_background(cmd);
                } else {
                    self.run_foreground(cmd);
                }
            }
        }
    }

    // ... all the methods from this module ...
}
```

### Ownership note

Notice that `Job` **owns** the `Child` handle. This is important — when a `Job` is dropped (removed from the HashMap), the `Child` handle is dropped too. On Unix, dropping a `Child` without calling `.wait()` can leave a zombie process. Always `.wait()` or `.try_wait()` before removing a job from the table.

```rust
// WRONG: removing a job without waiting leaves a zombie
self.jobs.remove(&id);

// RIGHT: wait first, then remove
if let Some(mut job) = self.jobs.remove(&id) {
    let _ = job.child.try_wait(); // clean up the process
}
```

Actually, `try_wait()` returning `Ok(Some(_))` means the process has already exited, so it is safe to drop. The issue is if the process is **still running** when you drop the `Child`. In recent Rust versions (1.70+), dropping a `Child` without waiting will **not** wait for the child — it will just leak the handle. On Unix this creates a zombie; on Windows the handle is closed but the process keeps running. The safest pattern is to always check `try_wait()` before removing.

---

## Key Rust concepts used

- **`std::process::Child`** — handle to a spawned process, with `.wait()`, `.try_wait()`, `.id()`, `.kill()`
- **`HashMap<usize, Job>`** — associative container for the job table
- **`#[cfg(unix)]` / `#[cfg(windows)]`** — conditional compilation for platform-specific code
- **`std::os::unix::process::CommandExt`** — Unix-specific extensions to `Command` (e.g., `pre_exec`)
- **`unsafe` blocks** — required for `libc` calls like `setpgid`, `tcsetpgrp`, `kill`, `waitpid`
- **Ownership transfer** — `Job` owns `Child`; removing a job from the map transfers ownership
- **`try_wait()` vs `wait()`** — non-blocking vs blocking process status check

---

## Milestone

```
jsh> sleep 5 &
[1] 48210
jsh> sleep 10 &
[2] 48211
jsh> jobs
[1]  Running    sleep 5
[2]  Running    sleep 10
jsh> echo hello
hello
jsh>                              ← (5 seconds pass, then press Enter)
[1]  Done  sleep 5
jsh> fg 2
sleep 10                          ← shell waits for sleep 10 to finish
jsh> jobs
jsh>                              ← no jobs remaining

# On Unix, Ctrl-Z works too:
jsh> sleep 60
^Z
[1]  Stopped  sleep 60
jsh> bg 1
[1]  sleep 60 &
jsh> jobs
[1]  Running    sleep 60
jsh> wait
[1]  Done  sleep 60
jsh>
```

On Windows, the same session works except for the Ctrl-Z / stop / resume portion:

```
jsh> sleep 5 &
[1] 13720
jsh> jobs
[1]  Running    sleep 5
jsh> wait
[1]  Done  sleep 5
jsh>
```

---

## What's next?

Module 9 dives deep into **signal handling** — making Ctrl-C kill only the foreground job (not the shell), handling SIGCHLD for automatic background job notification, and dealing with the cross-platform differences between Unix signals and Windows console control events.
