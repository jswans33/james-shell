/// Convert an OS process status into shell-style exit code semantics.
///
/// On Unix, processes terminated by signal map to `128 + signal`.
pub fn exit_code(status: std::process::ExitStatus) -> i32 {
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

#[cfg(unix)]
pub fn exit_code_from_wait_status(raw_status: libc::c_int) -> Option<i32> {
    if unsafe { libc::WIFEXITED(raw_status) } {
        return Some(unsafe { libc::WEXITSTATUS(raw_status) });
    }

    if unsafe { libc::WIFSIGNALED(raw_status) } {
        let signal = unsafe { libc::WTERMSIG(raw_status) };
        return Some(128 + signal);
    }

    None
}
