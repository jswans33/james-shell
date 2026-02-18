#[cfg(unix)]
use std::io;

#[cfg(unix)]
pub(crate) enum WaitOutcome {
    Exited(i32),
    Stopped,
}

#[cfg(unix)]
pub(crate) fn set_process_group(pid: libc::pid_t, pgid: libc::pid_t) -> io::Result<()> {
    loop {
        let rc = unsafe { libc::setpgid(pid, pgid) };
        if rc == 0 {
            return Ok(());
        }

        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code) if code == libc::EINTR => continue,
            // Already exec'd or gone; caller can proceed with best-effort behavior.
            Some(code) if code == libc::EACCES || code == libc::ESRCH => return Ok(()),
            _ => return Err(err),
        }
    }
}

#[cfg(unix)]
pub(crate) fn process_group_id(pid: libc::pid_t) -> io::Result<libc::pid_t> {
    loop {
        let rc = unsafe { libc::getpgid(pid) };
        if rc >= 0 {
            return Ok(rc);
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return Err(err);
    }
}

#[cfg(unix)]
pub(crate) fn send_continue_to_group(pgid: libc::pid_t) -> io::Result<()> {
    if pgid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid process group id",
        ));
    }

    loop {
        let rc = unsafe { libc::kill(-pgid, libc::SIGCONT) };
        if rc == 0 {
            return Ok(());
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return Err(err);
    }
}

#[cfg(unix)]
pub(crate) struct ForegroundTerminalGuard {
    tty_fd: Option<libc::c_int>,
    shell_pgid: libc::pid_t,
}

#[cfg(unix)]
impl ForegroundTerminalGuard {
    pub(crate) fn new(target_pgid: libc::pid_t) -> io::Result<Self> {
        let tty_fd = if unsafe { libc::isatty(libc::STDIN_FILENO) } == 1 {
            Some(libc::STDIN_FILENO)
        } else {
            None
        };

        let shell_pgid = unsafe { libc::getpgrp() };
        let guard = Self { tty_fd, shell_pgid };

        if let Some(fd) = guard.tty_fd {
            set_terminal_foreground(fd, target_pgid)?;
        }

        Ok(guard)
    }
}

#[cfg(unix)]
impl Drop for ForegroundTerminalGuard {
    fn drop(&mut self) {
        if let Some(fd) = self.tty_fd {
            let _ = set_terminal_foreground(fd, self.shell_pgid);
        }
    }
}

#[cfg(unix)]
pub(crate) fn wait_for_pid(pid: libc::pid_t) -> io::Result<WaitOutcome> {
    let mut raw_status: libc::c_int = 0;

    loop {
        let rc = unsafe { libc::waitpid(pid, &mut raw_status, libc::WUNTRACED) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }

        if unsafe { libc::WIFSTOPPED(raw_status) } {
            return Ok(WaitOutcome::Stopped);
        }

        if let Some(code) = crate::status::exit_code_from_wait_status(raw_status) {
            return Ok(WaitOutcome::Exited(code));
        }
    }
}

#[cfg(unix)]
struct SignalIgnoreGuard {
    signal: libc::c_int,
    previous: libc::sighandler_t,
}

#[cfg(unix)]
impl SignalIgnoreGuard {
    fn ignore(signal: libc::c_int) -> io::Result<Self> {
        let previous = unsafe { libc::signal(signal, libc::SIG_IGN) };
        if previous == libc::SIG_ERR {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { signal, previous })
    }
}

#[cfg(unix)]
impl Drop for SignalIgnoreGuard {
    fn drop(&mut self) {
        unsafe {
            libc::signal(self.signal, self.previous);
        }
    }
}

#[cfg(unix)]
fn set_terminal_foreground(fd: libc::c_int, pgid: libc::pid_t) -> io::Result<()> {
    if pgid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid process group id",
        ));
    }

    let _sigttou = SignalIgnoreGuard::ignore(libc::SIGTTOU)?;
    loop {
        let rc = unsafe { libc::tcsetpgrp(fd, pgid) };
        if rc == 0 {
            return Ok(());
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return Err(err);
    }
}
