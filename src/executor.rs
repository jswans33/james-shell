use os_pipe::{pipe, PipeReader, PipeWriter};
use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::process::{Command, Stdio};

use crate::builtins;
use crate::jobs::JobTable;
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

#[derive(Debug)]
pub struct PipelineCommand {
    pub command: parser::Command,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug)]
pub enum ExecutionAction {
    Continue(i32),
    Exit(i32),
}

/// Execute a parsed command with optional redirections.
/// Builtins are checked first, then external programs.
pub fn execute(
    cmd: &parser::Command,
    redirections: &[Redirection],
    background: bool,
    job_table: &mut JobTable,
    command_text: &str,
) -> ExecutionAction {
    if builtins::is_builtin(&cmd.program) {
        // Builtins always run in the foreground — background flag is ignored.
        return run_builtin(cmd, redirections, job_table);
    }

    ExecutionAction::Continue(run_external(cmd, redirections, background, job_table, command_text))
}

pub fn execute_pipeline(
    commands: Vec<PipelineCommand>,
    background: bool,
    job_table: &mut JobTable,
    command_text: &str,
) -> ExecutionAction {
    if commands.is_empty() {
        return ExecutionAction::Continue(0);
    }

    if commands.len() == 1 {
        let cmd = &commands[0];
        return execute(&cmd.command, &cmd.redirections, background, job_table, command_text);
    }

    if commands.iter().any(|cmd| cmd.command.program == "exit") {
        eprintln!("jsh: 'exit' is not supported in pipelines");
        return ExecutionAction::Continue(1);
    }

    let mut children: Vec<std::process::Child> = Vec::new();
    let mut prev_pipe: Option<PipeReader> = None;
    let mut last_status = 0;
    let last_is_external = !builtins::is_builtin(
        &commands
            .last()
            .map(|cmd| cmd.command.program.as_str())
            .unwrap_or(""),
    );
    let mut last_external_index: Option<usize> = None;

    for (idx, segment) in commands.iter().enumerate() {
        let is_last = idx + 1 == commands.len();
        let is_builtin = builtins::is_builtin(&segment.command.program);

        let stdin_default = prev_pipe
            .take()
            .map(InputHandle::Pipe)
            .unwrap_or(InputHandle::Inherit);

        let (stdout_default, next_pipe_reader) = if !is_last {
            match pipe() {
                Ok((reader, writer)) => (OutputHandle::Pipe(writer), Some(reader)),
                Err(e) => {
                    eprintln!("jsh: failed to create pipe: {e}");
                    wait_children(&mut children);
                    return ExecutionAction::Continue(1);
                }
            }
        } else {
            (OutputHandle::Inherit, None)
        };

        let defaults = RedirectionDefaults {
            stdin: stdin_default,
            stdout: stdout_default,
            stderr: OutputHandle::Inherit,
        };

        let resolved = match resolve_redirections(&segment.redirections, defaults) {
            Ok(resolved) => resolved,
            Err(msg) => {
                eprintln!("{msg}");
                wait_children(&mut children);
                return ExecutionAction::Continue(1);
            }
        };

        let ResolvedRedirections {
            stdin,
            stdout,
            stderr,
            stdout_redirected,
        } = resolved;

        if !is_last && stdout_redirected {
            eprintln!(
                "jsh: cannot redirect stdout of non-terminal pipeline command '{}'",
                segment.command.program
            );
            wait_children(&mut children);
            return ExecutionAction::Continue(1);
        }

        if is_builtin {
            let mut stdin_reader = match stdin.into_reader() {
                Ok(reader) => reader,
                Err(msg) => {
                    eprintln!("{msg}");
                    wait_children(&mut children);
                    return ExecutionAction::Continue(1);
                }
            };
            let mut stdout_writer = match stdout.into_writer("stdout") {
                Ok(writer) => writer,
                Err(msg) => {
                    eprintln!("{msg}");
                    wait_children(&mut children);
                    return ExecutionAction::Continue(1);
                }
            };
            let mut stderr_writer = match stderr.into_writer("stderr") {
                Ok(writer) => writer,
                Err(msg) => {
                    eprintln!("{msg}");
                    wait_children(&mut children);
                    return ExecutionAction::Continue(1);
                }
            };

            let status = match builtins::execute(
                &segment.command.program,
                &segment.command.args,
                stdin_reader.as_mut(),
                stdout_writer.as_mut(),
                stderr_writer.as_mut(),
                job_table,
            ) {
                builtins::BuiltinAction::Continue(code)
                | builtins::BuiltinAction::Exit(code) => code,
            };

            let _ = stdout_writer.flush();
            let _ = stderr_writer.flush();

            if is_last {
                last_status = status;
            }
        } else {
            let mut process = Command::new(&segment.command.program);
            process.args(&segment.command.args);

            let (stdin_stdio, here_string) = match stdin.into_stdio() {
                Ok(result) => result,
                Err(msg) => {
                    eprintln!("{msg}");
                    wait_children(&mut children);
                    return ExecutionAction::Continue(1);
                }
            };
            let stdout_stdio = match stdout.into_stdio() {
                Ok(stdio) => stdio,
                Err(msg) => {
                    eprintln!("{msg}");
                    wait_children(&mut children);
                    return ExecutionAction::Continue(1);
                }
            };
            let stderr_stdio = match stderr.into_stdio() {
                Ok(stdio) => stdio,
                Err(msg) => {
                    eprintln!("{msg}");
                    wait_children(&mut children);
                    return ExecutionAction::Continue(1);
                }
            };

            process.stdin(stdin_stdio).stdout(stdout_stdio).stderr(stderr_stdio);

            let mut child = match process.spawn() {
                Ok(child) => child,
                Err(e) => {
                    let code = command_error(&segment.command.program, &e);
                    wait_children(&mut children);
                    return ExecutionAction::Continue(code);
                }
            };

            if let Some(text) = here_string {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = writeln!(stdin, "{text}");
                }
            }

            children.push(child);
            last_external_index = Some(children.len() - 1);
        }

        prev_pipe = next_pipe_reader;
    }

    // ── Background pipeline: hand off the last external child, drop the rest ──
    if background {
        if let Some(last_idx) = last_external_index {
            // `swap_remove` moves the element at `last_idx` out.  Because we're
            // about to drop everything else anyway, reordering is irrelevant.
            let last_child = children.swap_remove(last_idx);
            drop(children); // drop remaining (may produce short-lived zombies until SIGCHLD in module 9)
            let (id, pid) = job_table.add(last_child, command_text.to_string());
            println!("[{}] {}", id, pid);
        }
        // No external children (all builtins) — nothing to track.
        return ExecutionAction::Continue(0);
    }

    // ── Foreground pipeline: wait for all children ──
    for (idx, mut child) in children.into_iter().enumerate() {
        match child.wait() {
            Ok(status) => {
                if last_is_external && Some(idx) == last_external_index {
                    last_status = exit_code(status);
                }
            }
            Err(_) => {
                return ExecutionAction::Continue(1);
            }
        }
    }

    ExecutionAction::Continue(last_status)
}

// ── Redirection resolution ──

#[derive(Debug)]
enum InputHandle {
    Inherit,
    Pipe(PipeReader),
    File(File),
    HereString(String),
}

#[derive(Debug)]
enum OutputHandle {
    Inherit,
    Null,
    File(File),
    Pipe(PipeWriter),
}

struct ResolvedRedirections {
    stdin: InputHandle,
    stdout: OutputHandle,
    stderr: OutputHandle,
    stdout_redirected: bool,
}

struct RedirectionDefaults {
    stdin: InputHandle,
    stdout: OutputHandle,
    stderr: OutputHandle,
}

impl OutputHandle {
    fn try_clone(&self) -> Result<OutputHandle, String> {
        match self {
            OutputHandle::Inherit => Ok(OutputHandle::Inherit),
            OutputHandle::Null => Ok(OutputHandle::Null),
            OutputHandle::File(file) => file
                .try_clone()
                .map(OutputHandle::File)
                .map_err(|e| format!("jsh: failed to duplicate file: {e}")),
            OutputHandle::Pipe(writer) => writer
                .try_clone()
                .map(OutputHandle::Pipe)
                .map_err(|e| format!("jsh: failed to duplicate pipe: {e}")),
        }
    }

    fn into_stdio(self) -> Result<Stdio, String> {
        Ok(match self {
            OutputHandle::Inherit => Stdio::inherit(),
            OutputHandle::Null => Stdio::null(),
            OutputHandle::File(file) => Stdio::from(file),
            OutputHandle::Pipe(writer) => Stdio::from(writer),
        })
    }

    fn into_writer(self, label: &str) -> Result<Box<dyn Write>, String> {
        match self {
            OutputHandle::Inherit => {
                if label == "stderr" {
                    Ok(Box::new(io::stderr()))
                } else {
                    Ok(Box::new(io::stdout()))
                }
            }
            OutputHandle::Null => Ok(Box::new(io::sink())),
            OutputHandle::File(file) => Ok(Box::new(file)),
            OutputHandle::Pipe(writer) => Ok(Box::new(writer)),
        }
    }
}

impl InputHandle {
    fn into_stdio(self) -> Result<(Stdio, Option<String>), String> {
        Ok(match self {
            InputHandle::Inherit => (Stdio::inherit(), None),
            InputHandle::Pipe(reader) => (Stdio::from(reader), None),
            InputHandle::File(file) => (Stdio::from(file), None),
            InputHandle::HereString(text) => (Stdio::piped(), Some(text)),
        })
    }

    fn into_reader(self) -> Result<Box<dyn Read>, String> {
        match self {
            InputHandle::Inherit => Ok(Box::new(io::stdin())),
            InputHandle::Pipe(reader) => Ok(Box::new(reader)),
            InputHandle::File(file) => Ok(Box::new(file)),
            InputHandle::HereString(text) => {
                Ok(Box::new(Cursor::new(format!("{text}\n"))))
            }
        }
    }
}

fn resolve_redirections(
    redirections: &[Redirection],
    defaults: RedirectionDefaults,
) -> Result<ResolvedRedirections, String> {
    let mut stdin = defaults.stdin;
    let mut stdout = defaults.stdout;
    let mut stderr = defaults.stderr;
    let mut stdout_redirected = false;

    for redir in redirections {
        match (&redir.target, redir.fd) {
            // ── fd duplicated to itself — no-op ──
            (RedirectTarget::Fd(target), fd) if *target == fd => {}

            // ── stdout > file (truncate) ──
            (RedirectTarget::File(path), 1) => {
                stdout = open_output_file(path, false)?;
                stdout_redirected = true;
            }

            // ── stdout >> file (append) ──
            (RedirectTarget::FileAppend(path), 1) => {
                stdout = open_output_file(path, true)?;
                stdout_redirected = true;
            }

            // ── stdin < file ──
            (RedirectTarget::FileRead(path), 0) => {
                stdin = open_input_file(path)?;
            }

            // ── stderr 2> file (truncate) ──
            (RedirectTarget::File(path), 2) => {
                stderr = open_output_file(path, false)?;
            }

            // ── stderr 2>> file (append) ──
            (RedirectTarget::FileAppend(path), 2) => {
                stderr = open_output_file(path, true)?;
            }

            // ── 2>&1: stderr → wherever stdout currently points ──
            (RedirectTarget::Fd(1), 2) => {
                stderr = stdout.try_clone()?;
            }

            // ── 1>&2: stdout → wherever stderr currently points ──
            (RedirectTarget::Fd(2), 1) => {
                stdout = stderr.try_clone()?;
                stdout_redirected = true;
            }

            // ── Here string: <<< text ──
            (RedirectTarget::HereString(text), 0) => {
                stdin = InputHandle::HereString(text.clone());
            }

            _ => {
                return Err(format!(
                    "jsh: unsupported redirection: fd {} -> {:?}",
                    redir.fd, redir.target
                ));
            }
        }
    }

    Ok(ResolvedRedirections {
        stdin,
        stdout,
        stderr,
        stdout_redirected,
    })
}

fn open_output_file(path: &str, append: bool) -> Result<OutputHandle, String> {
    if is_null_device(path) {
        return Ok(OutputHandle::Null);
    }

    let file = if append {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
    };

    file.map(OutputHandle::File)
        .map_err(|e| format!("jsh: {path}: {e}"))
}

fn open_input_file(path: &str) -> Result<InputHandle, String> {
    let file = File::open(path)
        .map_err(|e| format!("jsh: {path}: {e}"))?;
    Ok(InputHandle::File(file))
}

fn wait_children(children: &mut Vec<std::process::Child>) {
    for mut child in children.drain(..) {
        let _ = child.wait();
    }
}

// ── Builtin execution with redirections ──

/// Run a builtin command, routing its output through redirect targets.
fn run_builtin(
    cmd: &parser::Command,
    redirections: &[Redirection],
    job_table: &mut JobTable,
) -> ExecutionAction {
    let defaults = RedirectionDefaults {
        stdin: InputHandle::Inherit,
        stdout: OutputHandle::Inherit,
        stderr: OutputHandle::Inherit,
    };

    let resolved = match resolve_redirections(redirections, defaults) {
        Ok(resolved) => resolved,
        Err(msg) => {
            eprintln!("{msg}");
            return ExecutionAction::Continue(1);
        }
    };

    let ResolvedRedirections { stdin, stdout, stderr, .. } = resolved;

    let mut stdin_reader = match stdin.into_reader() {
        Ok(reader) => reader,
        Err(msg) => {
            eprintln!("{msg}");
            return ExecutionAction::Continue(1);
        }
    };

    let mut stdout_writer = match stdout.into_writer("stdout") {
        Ok(writer) => writer,
        Err(msg) => {
            eprintln!("{msg}");
            return ExecutionAction::Continue(1);
        }
    };

    let mut stderr_writer = match stderr.into_writer("stderr") {
        Ok(writer) => writer,
        Err(msg) => {
            eprintln!("{msg}");
            return ExecutionAction::Continue(1);
        }
    };

    let action = match builtins::execute(
        &cmd.program,
        &cmd.args,
        stdin_reader.as_mut(),
        stdout_writer.as_mut(),
        stderr_writer.as_mut(),
        job_table,
    ) {
        builtins::BuiltinAction::Continue(code) => ExecutionAction::Continue(code),
        builtins::BuiltinAction::Exit(code) => ExecutionAction::Exit(code),
    };

    let _ = stdout_writer.flush();
    let _ = stderr_writer.flush();

    action
}

// ── External command execution with redirections ──

/// Spawn an external program with I/O redirections applied.
/// If `background` is true, the child is handed off to the job table immediately.
fn run_external(
    cmd: &parser::Command,
    redirections: &[Redirection],
    background: bool,
    job_table: &mut JobTable,
    command_text: &str,
) -> i32 {
    let defaults = RedirectionDefaults {
        stdin: InputHandle::Inherit,
        stdout: OutputHandle::Inherit,
        stderr: OutputHandle::Inherit,
    };

    let resolved = match resolve_redirections(redirections, defaults) {
        Ok(resolved) => resolved,
        Err(msg) => {
            eprintln!("{msg}");
            return 1;
        }
    };

    let ResolvedRedirections { stdin, stdout, stderr, .. } = resolved;

    let mut process = Command::new(&cmd.program);
    process.args(&cmd.args);

    let (stdin_stdio, here_string) = match stdin.into_stdio() {
        Ok(result) => result,
        Err(msg) => {
            eprintln!("{msg}");
            return 1;
        }
    };

    let stdout_stdio = match stdout.into_stdio() {
        Ok(stdio) => stdio,
        Err(msg) => {
            eprintln!("{msg}");
            return 1;
        }
    };

    let stderr_stdio = match stderr.into_stdio() {
        Ok(stdio) => stdio,
        Err(msg) => {
            eprintln!("{msg}");
            return 1;
        }
    };

    process.stdin(stdin_stdio).stdout(stdout_stdio).stderr(stderr_stdio);

    // ── Background: spawn and hand off to job table ──
    if background {
        match process.spawn() {
            Ok(mut child) => {
                if let Some(text) = here_string {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = writeln!(stdin, "{text}");
                    }
                }
                let (id, pid) = job_table.add(child, command_text.to_string());
                println!("[{}] {}", id, pid);
                return 0;
            }
            Err(e) => return command_error(&cmd.program, &e),
        }
    }

    // ── Foreground with here-string: must spawn to feed stdin ──
    if let Some(text) = here_string {
        return match process.spawn() {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = writeln!(stdin, "{text}");
                }
                run_foreground(child, &cmd.program, job_table, command_text)
            }
            Err(e) => command_error(&cmd.program, &e),
        };
    }

    // ── Foreground without here-string ──
    match process.spawn() {
        Ok(child) => run_foreground(child, &cmd.program, job_table, command_text),
        Err(e) => command_error(&cmd.program, &e),
    }
}

/// Wait for a foreground child process to finish or be stopped (Unix: Ctrl-Z).
///
/// On Unix we call `waitpid` with `WUNTRACED` so that a SIGTSTP (Ctrl-Z) from
/// the user causes the wait to return instead of blocking forever. If the child
/// is stopped, we move it to the job table rather than discarding it.
///
/// On Windows (and other non-Unix targets) we simply call `child.wait()`.
fn run_foreground(
    mut child: std::process::Child,
    cmd_name: &str,
    // These are consumed only in the #[cfg(unix)] path; suppress the
    // "unused variable" warning that fires on non-Unix builds.
    #[allow(unused_variables)] job_table: &mut JobTable,
    #[allow(unused_variables)] command_text: &str,
) -> i32 {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        let mut raw_status: libc::c_int = 0;

        // WUNTRACED: return when the child stops (SIGTSTP), not only when it exits.
        let wait_result = unsafe { libc::waitpid(pid, &mut raw_status, libc::WUNTRACED) };

        if wait_result < 0 {
            eprintln!("jsh: {cmd_name}: waitpid failed");
            return 1;
        }

        if unsafe { libc::WIFSTOPPED(raw_status) } {
            // Child was stopped by Ctrl-Z (SIGTSTP). Move it to the job table.
            let (id, _) = job_table.add_stopped(child, command_text.to_string());
            println!("[{}]  Stopped  {}", id, command_text);
            return 0;
        }

        if unsafe { libc::WIFEXITED(raw_status) } {
            return unsafe { libc::WEXITSTATUS(raw_status) };
        }

        if unsafe { libc::WIFSIGNALED(raw_status) } {
            let sig = unsafe { libc::WTERMSIG(raw_status) };
            return 128 + sig;
        }

        1
    }

    #[cfg(not(unix))]
    {
        match child.wait() {
            Ok(status) => exit_code(status),
            Err(e) => {
                eprintln!("jsh: {cmd_name}: {e}");
                1
            }
        }
    }
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
