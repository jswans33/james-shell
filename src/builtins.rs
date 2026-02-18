use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::jobs::{JobStatus, JobTable};

/// The list of all builtin command names.
const BUILTINS: &[&str] = &[
    "cd", "pwd", "exit", "echo", "export", "unset", "type", "jobs", "fg", "bg", "wait",
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

    // Save current directory as OLDPWD before changing.
    // SAFETY: We only mutate env vars on the main thread. The ctrlc handler
    // thread does not read or write environment variables.
    if let Ok(cwd) = std::env::current_dir() {
        unsafe { std::env::set_var("OLDPWD", cwd) };
    }

    if let Err(e) = std::env::set_current_dir(&target) {
        let _ = writeln!(stderr, "cd: {target}: {e}");
        return 1;
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

    // Remove the job from the table — it's transitioning to foreground.
    let mut job = match job_table.remove(job_id) {
        Some(j) => j,
        None => {
            let _ = writeln!(stderr, "fg: {}: no such job", job_id);
            return 1;
        }
    };

    let _ = writeln!(stdout, "{}", job.command);

    // On Unix, a stopped job needs SIGCONT before we can wait for it.
    #[cfg(unix)]
    if job.status == JobStatus::Stopped {
        unsafe {
            libc::kill(job.pid as libc::pid_t, libc::SIGCONT);
        }
    }

    match job.child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            let _ = writeln!(stderr, "fg: error waiting for job: {}", e);
            1
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
    let job_id =
        match resolve_job_id(args.first(), job_table.most_recent_stopped_id(), stderr) {
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
            unsafe {
                libc::kill(job.pid as libc::pid_t, libc::SIGCONT);
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
    if args.is_empty() {
        let ids = job_table.running_ids();
        for id in ids {
            wait_for_job(id, job_table, stdout, stderr);
        }
    } else {
        for arg in args {
            match arg.trim_start_matches('%').parse::<usize>() {
                Ok(id) => wait_for_job(id, job_table, stdout, stderr),
                Err(_) => {
                    let _ = writeln!(stderr, "wait: invalid job id: {}", arg);
                }
            }
        }
    }
    0
}

/// Blocking wait for a single job; removes it from the table when done.
fn wait_for_job(
    job_id: usize,
    job_table: &mut JobTable,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) {
    let job = match job_table.get_mut(job_id) {
        Some(j) => j,
        None => {
            let _ = writeln!(stderr, "wait: {}: no such job", job_id);
            return;
        }
    };

    if job.status != JobStatus::Running {
        return;
    }

    let id = job.id;
    let cmd = job.command.clone();

    match job.child.wait() {
        Ok(_status) => {
            let _ = writeln!(stdout, "[{}]  Done  {}", id, cmd);
        }
        Err(e) => {
            let _ = writeln!(stderr, "wait: error: {}", e);
        }
    }

    job_table.remove(job_id);
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
    let pathext = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
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
            let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
            let exts = exts.split(';').map(|ext| ext.trim_start_matches('.').to_ascii_lowercase());
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
