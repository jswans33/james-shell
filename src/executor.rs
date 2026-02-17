use std::process::Command;

use crate::parser;

/// Derive an exit code from a process status.
/// On Unix, if a process is killed by a signal, `status.code()` is None
/// but we can recover the signal number. The shell convention is 128+signal.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    // On Unix, a signal-terminated process has no exit code but has a signal number.
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }

    1
}

/// Execute a parsed command by spawning an OS process.
/// Returns the exit code (0 = success, 127 = not found, 128+N = killed by signal N).
pub fn execute(cmd: &parser::Command) -> i32 {
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
