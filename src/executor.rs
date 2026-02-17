use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::builtins;
use crate::parser;
use crate::redirect::{is_null_device, Redirection, RedirectTarget};

/// Derive an exit code from a process status.
/// On Unix, if a process is killed by a signal, `status.code()` is None
/// but we can recover the signal number. The shell convention is 128+signal.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }

    1
}

/// Track what a file descriptor has been set to, so we can duplicate it
/// when processing 2>&1 or 1>&2 redirections.
#[derive(Clone)]
enum StdioTarget {
    /// Inherited from the shell (default)
    Inherit,
    /// Discarded via /dev/null or NUL
    Null,
    /// Redirected to a file (path, append)
    FilePath(String, bool),
}

/// A pair of writers for stdout and stderr.
type WriterPair = (Box<dyn Write>, Box<dyn Write>);

/// Execute a parsed command with optional redirections.
/// Builtins are checked first, then external programs.
pub fn execute(cmd: &parser::Command, redirections: &[Redirection]) -> i32 {
    if builtins::is_builtin(&cmd.program) {
        return run_builtin(cmd, redirections);
    }

    run_external(cmd, redirections)
}

// ── Builtin execution with redirections ──

/// Run a builtin command, routing its output through redirect targets.
fn run_builtin(cmd: &parser::Command, redirections: &[Redirection]) -> i32 {
    // Resolve redirections into writers for stdout and stderr
    let (mut stdout_writer, mut stderr_writer) = match open_builtin_writers(redirections) {
        Ok(pair) => pair,
        Err(msg) => {
            eprintln!("{msg}");
            return 1;
        }
    };

    builtins::execute(
        &cmd.program,
        &cmd.args,
        stdout_writer.as_mut(),
        stderr_writer.as_mut(),
    )
}

/// Resolve redirections into boxed writers for builtins.
/// Processes redirections left-to-right, tracking targets for fd duplication.
fn open_builtin_writers(
    redirections: &[Redirection],
) -> Result<WriterPair, String> {
    let mut stdout_target = StdioTarget::Inherit;
    let mut stderr_target = StdioTarget::Inherit;

    for redir in redirections {
        match (&redir.target, redir.fd) {
            // fd duplicated to itself — no-op (e.g. >&1, 2>&2)
            (RedirectTarget::Fd(target), fd) if *target == fd => {}

            (RedirectTarget::File(path), 1) => {
                stdout_target = if is_null_device(path) {
                    StdioTarget::Null
                } else {
                    StdioTarget::FilePath(path.clone(), false)
                };
            }
            (RedirectTarget::FileAppend(path), 1) => {
                stdout_target = StdioTarget::FilePath(path.clone(), true);
            }
            (RedirectTarget::File(path), 2) => {
                stderr_target = if is_null_device(path) {
                    StdioTarget::Null
                } else {
                    StdioTarget::FilePath(path.clone(), false)
                };
            }
            (RedirectTarget::FileAppend(path), 2) => {
                stderr_target = StdioTarget::FilePath(path.clone(), true);
            }
            (RedirectTarget::Fd(1), 2) => {
                stderr_target = stdout_target.clone();
            }
            (RedirectTarget::Fd(2), 1) => {
                stdout_target = stderr_target.clone();
            }
            // Builtins don't support stdin redirection (< and <<<)
            (RedirectTarget::FileRead(_) | RedirectTarget::HereString(_), 0) => {}
            _ => {
                return Err(format!(
                    "jsh: unsupported redirection: fd {} -> {:?}",
                    redir.fd, redir.target
                ));
            }
        }
    }

    let stdout_writer: Box<dyn Write> = open_writer(&stdout_target, "stdout")?;
    let stderr_writer: Box<dyn Write> = open_writer(&stderr_target, "stderr")?;

    Ok((stdout_writer, stderr_writer))
}

/// Open a writer for a given StdioTarget.
fn open_writer(target: &StdioTarget, label: &str) -> Result<Box<dyn Write>, String> {
    match target {
        StdioTarget::Inherit => {
            if label == "stderr" {
                Ok(Box::new(io::stderr()))
            } else {
                Ok(Box::new(io::stdout()))
            }
        }
        StdioTarget::Null => Ok(Box::new(io::sink())),
        StdioTarget::FilePath(path, append) => {
            let file = if *append {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
            } else {
                File::create(path)
            };
            Ok(Box::new(
                file.map_err(|e| format!("jsh: {path}: {e}"))?,
            ))
        }
    }
}

// ── External command execution with redirections ──

/// Spawn an external program with I/O redirections applied.
fn run_external(cmd: &parser::Command, redirections: &[Redirection]) -> i32 {
    let mut process = Command::new(&cmd.program);
    process.args(&cmd.args);

    // Apply all redirections, collecting any here-string text
    let here_string = match apply_redirections(&mut process, redirections) {
        Ok(hs) => hs,
        Err(msg) => {
            eprintln!("{msg}");
            return 1;
        }
    };

    if let Some(text) = here_string {
        // Here-string: spawn, write to stdin pipe, then wait
        match process.spawn() {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    // Write text + newline (bash convention), then drop to send EOF
                    let _ = writeln!(stdin, "{text}");
                }
                match child.wait() {
                    Ok(status) => exit_code(status),
                    Err(e) => {
                        eprintln!("jsh: {}: {e}", cmd.program);
                        1
                    }
                }
            }
            Err(e) => command_error(&cmd.program, &e),
        }
    } else {
        // Normal execution: status() blocks until the child exits
        match process.status() {
            Ok(status) => exit_code(status),
            Err(e) => command_error(&cmd.program, &e),
        }
    }
}

/// Apply all redirections to a Command, returning any here-string text.
/// Redirections are processed left-to-right (order matters for 2>&1).
fn apply_redirections(
    cmd: &mut Command,
    redirections: &[Redirection],
) -> Result<Option<String>, String> {
    let mut stdout_target = StdioTarget::Inherit;
    let mut stderr_target = StdioTarget::Inherit;
    let mut here_string: Option<String> = None;

    for redir in redirections {
        match (&redir.target, redir.fd) {
            // ── fd duplicated to itself — no-op ──
            (RedirectTarget::Fd(target), fd) if *target == fd => {}

            // ── stdout > file (truncate) ──
            (RedirectTarget::File(path), 1) => {
                if is_null_device(path) {
                    cmd.stdout(Stdio::null());
                    stdout_target = StdioTarget::Null;
                } else {
                    let file = File::create(path)
                        .map_err(|e| format!("jsh: {path}: {e}"))?;
                    cmd.stdout(Stdio::from(file));
                    stdout_target = StdioTarget::FilePath(path.clone(), false);
                }
            }

            // ── stdout >> file (append) ──
            (RedirectTarget::FileAppend(path), 1) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| format!("jsh: {path}: {e}"))?;
                cmd.stdout(Stdio::from(file));
                stdout_target = StdioTarget::FilePath(path.clone(), true);
            }

            // ── stdin < file ──
            (RedirectTarget::FileRead(path), 0) => {
                let file = File::open(path)
                    .map_err(|e| format!("jsh: {path}: {e}"))?;
                cmd.stdin(Stdio::from(file));
            }

            // ── stderr 2> file (truncate) ──
            (RedirectTarget::File(path), 2) => {
                if is_null_device(path) {
                    cmd.stderr(Stdio::null());
                    stderr_target = StdioTarget::Null;
                } else {
                    let file = File::create(path)
                        .map_err(|e| format!("jsh: {path}: {e}"))?;
                    cmd.stderr(Stdio::from(file));
                    stderr_target = StdioTarget::FilePath(path.clone(), false);
                }
            }

            // ── stderr 2>> file (append) ──
            (RedirectTarget::FileAppend(path), 2) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| format!("jsh: {path}: {e}"))?;
                cmd.stderr(Stdio::from(file));
                stderr_target = StdioTarget::FilePath(path.clone(), true);
            }

            // ── 2>&1: stderr → wherever stdout currently points ──
            (RedirectTarget::Fd(1), 2) => {
                apply_dup(cmd, &stdout_target, 2)?;
                stderr_target = stdout_target.clone();
            }

            // ── 1>&2: stdout → wherever stderr currently points ──
            (RedirectTarget::Fd(2), 1) => {
                apply_dup(cmd, &stderr_target, 1)?;
                stdout_target = stderr_target.clone();
            }

            // ── Here string: <<< text ──
            (RedirectTarget::HereString(text), 0) => {
                here_string = Some(text.clone());
                cmd.stdin(Stdio::piped());
            }

            _ => {
                return Err(format!(
                    "jsh: unsupported redirection: fd {} -> {:?}",
                    redir.fd, redir.target
                ));
            }
        }
    }

    Ok(here_string)
}

/// Duplicate an fd's target onto another fd.
/// Opens the same file again (two cursors) for the cross-platform approach.
fn apply_dup(cmd: &mut Command, source: &StdioTarget, dest_fd: i32) -> Result<(), String> {
    let stdio = match source {
        StdioTarget::Inherit => Stdio::inherit(),
        StdioTarget::Null => Stdio::null(),
        StdioTarget::FilePath(path, append) => {
            let file = if *append {
                OpenOptions::new().create(true).append(true).open(path)
            } else {
                OpenOptions::new().create(true).write(true).truncate(false).open(path)
            };
            Stdio::from(file.map_err(|e| format!("jsh: {path}: {e}"))?)
        }
    };

    match dest_fd {
        0 => cmd.stdin(stdio),
        1 => cmd.stdout(stdio),
        2 => cmd.stderr(stdio),
        _ => return Err(format!("jsh: unsupported fd: {dest_fd}")),
    };

    Ok(())
}

/// Map a spawn/exec error to the appropriate exit code.
fn command_error(program: &str, e: &std::io::Error) -> i32 {
    if e.kind() == std::io::ErrorKind::NotFound {
        eprintln!("jsh: command not found: {program}");
        127
    } else {
        eprintln!("jsh: {program}: {e}");
        126
    }
}
