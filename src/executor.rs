use std::process::Command;

use crate::builtins;
use crate::parser;

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

/// Execute a parsed command — builtins first, then external programs.
/// Returns the exit code.
pub fn execute(cmd: &parser::Command) -> i32 {
    if builtins::is_builtin(&cmd.program) {
        return builtins::execute(&cmd.program, &cmd.args);
    }

    run_external(cmd)
}

/// Spawn an external program as a child process.
fn run_external(cmd: &parser::Command) -> i32 {
    match Command::new(&cmd.program).args(&cmd.args).status() {
        Ok(status) => exit_code(status),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("jsh: command not found: {}", cmd.program);
                127
            } else {
                eprintln!("jsh: {}: {e}", cmd.program);
                126
            }
        }
    }
}
