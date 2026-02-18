use std::io::Write;
use std::process::{Command, Stdio};

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
fn failing_background_command() -> &'static str {
    "sh -c 'sleep 1; exit 7' &"
}

#[cfg(windows)]
fn failing_background_command() -> &'static str {
    "powershell -NoProfile -Command \"Start-Sleep -Seconds 1; exit 7\" &"
}

#[test]
fn wait_returns_background_job_exit_status() {
    let output = run_shell(&[failing_background_command(), "wait", "echo WAIT:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WAIT:7"), "stdout was: {stdout}");
}

#[test]
fn wait_invalid_job_id_sets_nonzero_status() {
    let output = run_shell(&["wait %99999", "echo WAIT:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WAIT:1"), "stdout was: {stdout}");
}

#[cfg(unix)]
#[test]
fn fg_preserves_signal_exit_code() {
    let output = run_shell(&["sh -c 'sleep 1; kill -INT $$' &", "fg", "echo FG:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FG:130"), "stdout was: {stdout}");
}
