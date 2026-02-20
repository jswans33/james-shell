use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use crate::job_control;
use crate::jobs::{JobStatus, JobTable};
use crate::status;

/// The list of all builtin command names.
const BUILTINS: &[&str] = &[
    "cd", "pwd", "exit", "echo", "export", "unset", "type", "jobs", "fg", "bg", "wait", "help",
];

#[derive(Debug)]
pub enum BuiltinAction {
    Continue(i32),
    Exit(i32),
}

/// Returns true if the command name is a shell builtin.
pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// Execute a builtin command, writing output to the provided streams.
/// Returns the exit code.
pub fn execute(
    program: &str,
    args: &[String],
    _stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    job_table: &mut JobTable,
) -> BuiltinAction {
    match program {
        "cd" => BuiltinAction::Continue(builtin_cd(args, stderr)),
        "pwd" => BuiltinAction::Continue(builtin_pwd(stdout, stderr)),
        "exit" => builtin_exit(args, stderr),
        "echo" => BuiltinAction::Continue(builtin_echo(args, stdout)),
        "export" => BuiltinAction::Continue(builtin_export(args, stderr)),
        "unset" => BuiltinAction::Continue(builtin_unset(args)),
        "type" => BuiltinAction::Continue(builtin_type(args, stdout, stderr)),
        "jobs" => BuiltinAction::Continue(builtin_jobs(job_table, stdout)),
        "fg" => BuiltinAction::Continue(builtin_fg(args, job_table, stdout, stderr)),
        "bg" => BuiltinAction::Continue(builtin_bg(args, job_table, stdout, stderr)),
        "wait" => BuiltinAction::Continue(builtin_wait(args, job_table, stdout, stderr)),
        "help" => BuiltinAction::Continue(builtin_help(args, stdout, stderr)),
        _ => {
            let _ = writeln!(stderr, "jsh: unknown builtin: {program}");
            BuiltinAction::Continue(1)
        }
    }
}

fn builtin_cd(args: &[String], stderr: &mut dyn Write) -> i32 {
    let target = match args.first() {
        Some(dir) if dir == "-" => {
            // cd - : go to previous directory
            match std::env::var("OLDPWD") {
                Ok(prev) => prev,
                Err(_) => {
                    let _ = writeln!(stderr, "cd: OLDPWD not set");
                    return 1;
                }
            }
        }
        Some(dir) => dir.clone(),
        None => {
            // cd with no args → go home
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string())
        }
    };

    let old_dir = std::env::current_dir().ok();

    // On success, update OLDPWD to the directory we left.
    // SAFETY: We only mutate env vars on the main thread. The ctrlc handler
    // thread does not read or write environment variables.
    if let Err(e) = std::env::set_current_dir(&target) {
        let _ = writeln!(stderr, "cd: {target}: {e}");
        return 1;
    }

    if let Some(cwd) = old_dir {
        // SAFETY: We only mutate env vars on the main thread. The ctrlc handler
        // thread does not read or write environment variables.
        unsafe { std::env::set_var("OLDPWD", cwd) };
    }

    0
}

fn builtin_pwd(stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    match std::env::current_dir() {
        Ok(path) => {
            let _ = writeln!(stdout, "{}", path.display());
            0
        }
        Err(e) => {
            let _ = writeln!(stderr, "pwd: {e}");
            1
        }
    }
}

fn builtin_exit(args: &[String], stderr: &mut dyn Write) -> BuiltinAction {
    match args.first() {
        None => BuiltinAction::Exit(0),
        Some(s) => match s.parse::<i32>() {
            Ok(code) => BuiltinAction::Exit(code),
            Err(_) => {
                let _ = writeln!(stderr, "exit: {s}: numeric argument required");
                BuiltinAction::Exit(2)
            }
        },
    }
}

fn builtin_echo(args: &[String], stdout: &mut dyn Write) -> i32 {
    let _ = writeln!(stdout, "{}", args.join(" "));
    0
}

fn builtin_export(args: &[String], stderr: &mut dyn Write) -> i32 {
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            // SAFETY: Env var mutation only happens on the main thread.
            unsafe { std::env::set_var(key, value) };
        } else {
            // export VAR with no value — just mark for export (no-op for now)
            let _ = writeln!(stderr, "export: usage: export VAR=value");
        }
    }
    0
}

fn builtin_unset(args: &[String]) -> i32 {
    for arg in args {
        // SAFETY: Env var mutation only happens on the main thread.
        unsafe { std::env::remove_var(arg) };
    }
    0
}

fn builtin_type(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let mut exit_code = 0;
    for arg in args {
        if is_builtin(arg) {
            let _ = writeln!(stdout, "{arg} is a shell builtin");
        } else {
            match find_in_path(arg) {
                Some(path) => {
                    let _ = writeln!(stdout, "{arg} is {}", path.display());
                }
                None => {
                    let _ = writeln!(stderr, "{arg}: not found");
                    exit_code = 1;
                }
            }
        }
    }
    exit_code
}

fn builtin_help(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        // ── no args: overview ────────────────────────────────────────────────
        None => {
            let _ = writeln!(stdout, "jsh — James Shell  (type 'help <topic>' for details)");
            let _ = writeln!(stdout, "");
            let _ = writeln!(stdout, "Builtins:");
            let _ = writeln!(stdout, "  cd [dir|-]          Change directory (- goes to previous)");
            let _ = writeln!(stdout, "  pwd                 Print working directory");
            let _ = writeln!(stdout, "  echo [args...]      Print arguments");
            let _ = writeln!(stdout, "  export VAR=value    Set and export environment variable");
            let _ = writeln!(stdout, "  unset VAR           Remove environment variable");
            let _ = writeln!(stdout, "  type name...        Show whether name is builtin or external");
            let _ = writeln!(stdout, "  exit [code]         Exit the shell");
            let _ = writeln!(stdout, "  jobs                List background jobs");
            let _ = writeln!(stdout, "  fg [%N]             Bring job to foreground");
            let _ = writeln!(stdout, "  bg [%N]             Resume stopped job in background");
            let _ = writeln!(stdout, "  wait [%N]           Wait for background job(s)");
            let _ = writeln!(stdout, "  Stateful builtins (cd/export/unset/fg/bg)");
            let _ = writeln!(stdout, "    are not supported in non-terminal pipeline steps");
            let _ = writeln!(stdout, "  help [topic]        Show this help or a topic reference");
            let _ = writeln!(stdout, "");
            let _ = writeln!(stdout, "Topics: variables  redirection  jobs  expansion  quotes  exit-codes");
            0
        }

        // ── builtin-specific usage ────────────────────────────────────────────
        Some("cd") => {
            let _ = writeln!(stdout, "cd [dir|-]");
            let _ = writeln!(stdout, "  Change the current directory.");
            let _ = writeln!(stdout, "  No argument: go to $HOME.");
            let _ = writeln!(stdout, "  '-': go to the previous directory ($OLDPWD).");
            let _ = writeln!(stdout, "  Sets $OLDPWD to the directory you came from.");
            0
        }
        Some("pwd") => {
            let _ = writeln!(stdout, "pwd");
            let _ = writeln!(stdout, "  Print the absolute path of the current directory.");
            let _ = writeln!(stdout, "  This command does not run as a background job.");
            0
        }
        Some("echo") => {
            let _ = writeln!(stdout, "echo [args...]");
            let _ = writeln!(stdout, "  Print arguments separated by spaces, followed by a newline.");
            0
        }
        Some("export") => {
            let _ = writeln!(stdout, "export VAR=value");
            let _ = writeln!(stdout, "  Set VAR to value and export it to child processes.");
            0
        }
        Some("unset") => {
            let _ = writeln!(stdout, "unset VAR...");
            let _ = writeln!(stdout, "  Remove one or more environment variables.");
            0
        }
        Some("type") => {
            let _ = writeln!(stdout, "type name...");
            let _ = writeln!(stdout, "  For each name, report whether it is a shell builtin");
            let _ = writeln!(stdout, "  or the full path of the external executable.");
            let _ = writeln!(stdout, "  Exit code 1 if any name is not found.");
            0
        }
        Some("exit") => {
            let _ = writeln!(stdout, "exit [code]");
            let _ = writeln!(stdout, "  Exit the shell with the given numeric exit code.");
            let _ = writeln!(stdout, "  No argument: exit 0.  Non-numeric argument: exit 2.");
            0
        }
        Some("help") => {
            let _ = writeln!(stdout, "help [topic|builtin]");
            let _ = writeln!(stdout, "  No argument: list all builtins and topics.");
            let _ = writeln!(stdout, "  Builtin name: show usage for that builtin.");
            let _ = writeln!(stdout, "  Topic name: show a reference section.");
            let _ = writeln!(stdout, "  Topics: variables  redirection  jobs  expansion  quotes  exit-codes");
            0
        }

        // ── topics ────────────────────────────────────────────────────────────
        // Note: "jobs" covers both the builtin and the job-control topic in one arm
        // to avoid an unreachable-pattern compiler error.
        Some("jobs") => {
            let _ = writeln!(stdout, "jobs");
            let _ = writeln!(stdout, "  List background and stopped jobs with their IDs.");
            let _ = writeln!(stdout, "  Status column: Running | Stopped | Done");
            let _ = writeln!(stdout, "");
            let _ = writeln!(stdout, "Job control summary:");
            let _ = writeln!(stdout, "  cmd &           Run command in background");
            let _ = writeln!(stdout, "  fg [%N]         Bring job to foreground");
            let _ = writeln!(stdout, "  bg [%N]         Resume stopped job in background");
            let _ = writeln!(stdout, "  wait [%N]       Wait for job(s) to finish");
            let _ = writeln!(stdout, "  Ctrl-Z          Suspend foreground job (Unix only)");
            0
        }
        Some("fg") => {
            let _ = writeln!(stdout, "fg [%N]");
            let _ = writeln!(stdout, "  Bring job %N to the foreground and wait for it.");
            let _ = writeln!(stdout, "  No argument: use the most recently backgrounded job.");
            0
        }
        Some("bg") => {
            let _ = writeln!(stdout, "bg [%N]");
            let _ = writeln!(stdout, "  Resume stopped job %N in the background.");
            let _ = writeln!(stdout, "  No argument: use the most recently stopped job.");
            0
        }
        Some("wait") => {
            let _ = writeln!(stdout, "wait [%N]");
            let _ = writeln!(stdout, "  Wait for background job %N to finish.");
            let _ = writeln!(stdout, "  No argument: wait for all background jobs.");
            let _ = writeln!(stdout, "  Sets $? to the exit code of the waited job.");
            0
        }
        Some("variables") => {
            let _ = writeln!(stdout, "Special variables:");
            let _ = writeln!(stdout, "  $?        Exit code of the last command");
            let _ = writeln!(stdout, "  $$        PID of the shell process");
            let _ = writeln!(stdout, "  $0        Shell name (always 'jsh')");
            let _ = writeln!(stdout, "  $HOME     Home directory");
            let _ = writeln!(stdout, "  $PATH     Command search path");
            let _ = writeln!(stdout, "  $PWD      Current directory");
            let _ = writeln!(stdout, "  $OLDPWD   Previous directory (set by cd)");
            let _ = writeln!(stdout, "  $USER     Current user name");
            let _ = writeln!(stdout, "  $VAR      Value of any exported environment variable");
            let _ = writeln!(stdout, "  ${{VAR}}    Same as $VAR (brace form)");
            0
        }
        Some("redirection") => {
            let _ = writeln!(stdout, "Redirection operators:");
            let _ = writeln!(stdout, "  cmd > file      Write stdout to file (truncate)");
            let _ = writeln!(stdout, "  cmd >> file     Append stdout to file");
            let _ = writeln!(stdout, "  cmd < file      Read stdin from file");
            let _ = writeln!(stdout, "  cmd 2> file     Write stderr to file");
            let _ = writeln!(stdout, "  cmd 2>> file    Append stderr to file");
            let _ = writeln!(stdout, "  cmd 2>&1        Merge stderr into stdout");
            let _ = writeln!(stdout, "  cmd 1>&2        Merge stdout into stderr");
            let _ = writeln!(stdout, "  cmd <<< word    Feed word as stdin (here-string)");
            0
        }
        Some("expansion") => {
            let _ = writeln!(stdout, "Word expansion (applied in order):");
            let _ = writeln!(stdout, "  ~               Expands to $HOME");
            let _ = writeln!(stdout, "  ~/path          Expands to $HOME/path");
            let _ = writeln!(stdout, "  $VAR            Variable substitution");
            let _ = writeln!(stdout, "  ${{VAR}}          Braced variable substitution");
            let _ = writeln!(stdout, "  *               Matches any string of characters");
            let _ = writeln!(stdout, "  ?               Matches any single character");
            let _ = writeln!(stdout, "  [abc]           Matches any character in the set");
            let _ = writeln!(stdout, "  Globs that match nothing are kept as literals.");
            let _ = writeln!(stdout, "  Globs inside quotes are not expanded.");
            0
        }
        Some("quotes") => {
            let _ = writeln!(stdout, "Quoting:");
            let _ = writeln!(stdout, "  'text'    Single quotes: no expansion of any kind");
            let _ = writeln!(stdout, "  \"text\"    Double quotes: $VAR expanded, globs suppressed");
            let _ = writeln!(stdout, "  \\c        Backslash: treat next character literally");
            let _ = writeln!(stdout, "  Mixing quote styles in one word is allowed.");
            0
        }
        Some("exit-codes") => {
            let _ = writeln!(stdout, "Exit codes:");
            let _ = writeln!(stdout, "  0          Success");
            let _ = writeln!(stdout, "  1          General error");
            let _ = writeln!(stdout, "  2          Bad usage (wrong arguments)");
            let _ = writeln!(stdout, "  126        Command found but not executable");
            let _ = writeln!(stdout, "  127        Command not found");
            let _ = writeln!(stdout, "  128+N      Command killed by signal N");
            let _ = writeln!(stdout, "  $?         Holds the exit code of the last command");
            0
        }

        // ── unknown ───────────────────────────────────────────────────────────
        Some(unknown) => {
            let _ = writeln!(stderr, "help: no help for '{unknown}'");
            1
        }
    }
}

// ── Job control builtins ──

/// List all tracked jobs.
fn builtin_jobs(job_table: &mut JobTable, stdout: &mut dyn Write) -> i32 {
    // Reap first so any jobs that just finished show as "Done" if still tracked,
    // but in practice reap() removes them — so jobs shows only live jobs.
    job_table.reap();

    for job in job_table.jobs_sorted() {
        let status_str = match &job.status {
            JobStatus::Running => "Running   ",
            JobStatus::Stopped => "Stopped   ",
            JobStatus::Done(_) => "Done      ",
        };
        let _ = writeln!(stdout, "[{}]  {} {}", job.id, status_str, job.command);
    }
    0
}

/// Bring a background or stopped job to the foreground and wait for it.
fn builtin_fg(
    args: &[String],
    job_table: &mut JobTable,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let job_id = match resolve_job_id(args.first(), job_table.most_recent_id(), stderr) {
        Some(id) => id,
        None => return 1,
    };

    #[allow(unused_variables)]
    let (pid, pgid, command) = match job_table.get_mut(job_id) {
        Some(job) => {
            // Mark as running while we foreground it.
            job.status = JobStatus::Running;
            (job.pid, job.pgid, job.command.clone())
        }
        None => {
            let _ = writeln!(stderr, "fg: {}: no such job", job_id);
            return 1;
        }
    };

    let _ = writeln!(stdout, "{command}");

    #[cfg(unix)]
    {
        let terminal_guard = match job_control::ForegroundTerminalGuard::new(pgid as libc::pid_t) {
            Ok(guard) => Some(guard),
            Err(e) => {
                let _ = writeln!(
                    stderr,
                    "fg: failed to move terminal to job {}: {}",
                    job_id, e
                );
                None
            }
        };

        if let Err(e) = job_control::send_continue_to_group(pgid as libc::pid_t) {
            // If the process has just exited, waitpid below will observe it.
            if e.raw_os_error() != Some(libc::ESRCH) {
                let _ = writeln!(stderr, "fg: failed to resume job {}: {}", job_id, e);
            }
        }

        let outcome = match job_control::wait_for_pid(pid as libc::pid_t) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = writeln!(stderr, "fg: error waiting for job {}: {}", job_id, e);
                return 1;
            }
        };

        drop(terminal_guard);

        match outcome {
            job_control::WaitOutcome::Stopped => {
                if let Some(job) = job_table.get_mut(job_id) {
                    job.status = JobStatus::Stopped;
                }
                let _ = writeln!(stdout, "[{}]  Stopped  {}", job_id, command);
                0
            }
            job_control::WaitOutcome::Exited(code) => {
                job_table.remove(job_id);
                code
            }
        }
    }

    #[cfg(not(unix))]
    {
        let wait_result = match job_table.get_mut(job_id) {
            Some(job) => job.child.wait(),
            None => {
                let _ = writeln!(stderr, "fg: {}: no such job", job_id);
                return 1;
            }
        };

        match wait_result {
            Ok(status) => {
                job_table.remove(job_id);
                status::exit_code(status)
            }
            Err(e) => {
                let _ = writeln!(stderr, "fg: error waiting for job {}: {}", job_id, e);
                1
            }
        }
    }
}

/// Resume a stopped job in the background (Unix only).
fn builtin_bg(
    args: &[String],
    job_table: &mut JobTable,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let job_id = match resolve_job_id(args.first(), job_table.most_recent_stopped_id(), stderr) {
        Some(id) => id,
        None => return 1,
    };

    match job_table.get_mut(job_id) {
        Some(job) => {
            if job.status != JobStatus::Stopped {
                let _ = writeln!(stderr, "bg: job {} is not stopped", job_id);
                return 1;
            }

            #[cfg(unix)]
            if let Err(e) = job_control::send_continue_to_group(job.pgid as libc::pid_t) {
                let _ = writeln!(stderr, "bg: failed to resume job {}: {}", job_id, e);
                return 1;
            }

            job.status = JobStatus::Running;
            let _ = writeln!(stdout, "[{}]  {} &", job.id, job.command);
            0
        }
        None => {
            let _ = writeln!(stderr, "bg: {}: no such job", job_id);
            1
        }
    }
}

/// Block until one or all background jobs finish.
fn builtin_wait(
    args: &[String],
    job_table: &mut JobTable,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let mut last_status = 0;
    let mut had_error = false;

    if args.is_empty() {
        let ids = job_table.running_ids();
        for id in ids {
            match wait_for_job(id, job_table, stdout, stderr) {
                Ok(status) => last_status = status,
                Err(()) => had_error = true,
            }
        }
    } else {
        for arg in args {
            match arg.trim_start_matches('%').parse::<usize>() {
                Ok(id) => match wait_for_job(id, job_table, stdout, stderr) {
                    Ok(status) => last_status = status,
                    Err(()) => had_error = true,
                },
                Err(_) => {
                    let _ = writeln!(stderr, "wait: invalid job id: {}", arg);
                    had_error = true;
                }
            }
        }
    }

    if had_error { 1 } else { last_status }
}

/// Blocking wait for a single job; removes it from the table when done.
fn wait_for_job(
    job_id: usize,
    job_table: &mut JobTable,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<i32, ()> {
    let job = match job_table.get_mut(job_id) {
        Some(j) => j,
        None => {
            let _ = writeln!(stderr, "wait: {}: no such job", job_id);
            return Err(());
        }
    };

    if job.status != JobStatus::Running {
        return Ok(0);
    }

    let id = job.id;
    let cmd = job.command.clone();

    let wait_result = job.child.wait();

    match wait_result {
        Ok(status) => {
            let code = status::exit_code(status);
            let _ = writeln!(stdout, "[{}]  Done  {}", id, cmd);
            job_table.remove(job_id);
            Ok(code)
        }
        Err(e) => {
            let _ = writeln!(stderr, "wait: error: {}", e);
            Err(())
        }
    }
}

// ── Helpers ──

/// Parse a job ID from an argument (accepts `%N` or `N`), falling back to
/// `default` when no argument is given.
fn resolve_job_id(
    arg: Option<&String>,
    default: Option<usize>,
    stderr: &mut dyn Write,
) -> Option<usize> {
    match arg {
        Some(s) => match s.trim_start_matches('%').parse::<usize>() {
            Ok(id) => Some(id),
            Err(_) => {
                let _ = writeln!(stderr, "invalid job id: {s}");
                None
            }
        },
        None => {
            if default.is_none() {
                let _ = writeln!(stderr, "no current job");
            }
            default
        }
    }
}

/// Check if a path points to an executable file.
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }

    // On Unix, check the executable permission bits
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return meta.permissions().mode() & 0o111 != 0;
    }

    // On Windows, being a file with the right extension is sufficient
    #[cfg(not(unix))]
    {
        let extension = match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) => ext.to_ascii_lowercase(),
            None => return false,
        };
        is_windows_executable_extension(&extension)
    }
}

#[cfg(not(unix))]
fn is_windows_executable_extension(extension: &str) -> bool {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    pathext
        .split(';')
        .any(|ext| extension == ext.trim_start_matches('.').to_ascii_lowercase())
}

/// Search PATH for an executable with the given name.
fn find_in_path(cmd: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    let separator = if cfg!(windows) { ';' } else { ':' };

    for dir in path_var.split(separator) {
        let full_path = Path::new(dir).join(cmd);
        if is_executable(&full_path) {
            return Some(full_path);
        }
        // On Windows, also try PATHEXT-configured executable extensions.
        if cfg!(windows) {
            let exts =
                std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
            let exts = exts
                .split(';')
                .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase());
            for ext in exts {
                let with_ext = full_path.with_extension(ext);
                if is_executable(&with_ext) {
                    return Some(with_ext);
                }
            }
        }
    }
    None
}
