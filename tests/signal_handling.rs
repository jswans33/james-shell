#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::process::{Command, Stdio};

#[cfg(unix)]
fn run_shell(lines: &[&str]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_james-shell"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn james-shell");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        for line in lines {
            writeln!(stdin, "{line}").expect("write line");
        }
        writeln!(stdin, "exit").expect("write exit");
    }

    child.wait_with_output().expect("wait output")
}

#[cfg(unix)]
#[test]
fn pipeline_sigpipe_does_not_abort_shell() {
    // yes writes indefinitely; head -1 exits after one line, closing the read end.
    // yes receives SIGPIPE (SIG_DFL in child via pre_exec) and terminates.
    // The shell has SIGPIPE = SIG_IGN, so it survives and runs the next command.
    // We also check $? to verify the shell is still responsive after the event
    // (guards against subtle state corruption where the shell appears alive but
    // stops processing commands normally).
    let output = run_shell(&["yes | head -1", "echo ALIVE", "echo STATUS:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ALIVE"), "stdout was: {stdout}");
    assert!(stdout.contains("STATUS:0"), "stdout was: {stdout}");
    assert!(output.status.success(), "shell did not exit cleanly");
}

#[cfg(unix)]
#[test]
fn shell_ignores_sigtstp_at_prompt() {
    // Send SIGTSTP to the shell's own process group via $$.
    // With SIG_IGN, the shell should not stop; it continues and prints ALIVE.
    let output = run_shell(&["kill -TSTP $$", "echo ALIVE"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ALIVE"), "stdout was: {stdout}");
}
