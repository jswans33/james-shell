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

// ── command helpers ───────────────────────────────────────────────────────────

#[cfg(unix)]
fn failing_background_command() -> &'static str {
    "sh -c 'sleep 1; exit 7' &"
}

#[cfg(windows)]
fn failing_background_command() -> &'static str {
    "powershell -NoProfile -Command \"Start-Sleep -Seconds 1; exit 7\" &"
}

/// A background command that stays alive long enough for `jobs` to observe it.
#[cfg(unix)]
fn long_background_command() -> &'static str {
    "sh -c 'sleep 3' &"
}

#[cfg(windows)]
fn long_background_command() -> &'static str {
    "powershell -NoProfile -Command \"Start-Sleep -Seconds 3\" &"
}

/// A background command that exits 0 as quickly as possible.
#[cfg(unix)]
fn quick_exit_background_command() -> &'static str {
    "sh -c 'exit 0' &"
}

#[cfg(windows)]
fn quick_exit_background_command() -> &'static str {
    "cmd /c exit 0 &"
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

// ── Module 8 coverage ─────────────────────────────────────────────────────────

#[test]
fn jobs_lists_a_running_background_job() {
    let output = run_shell(&[long_background_command(), "jobs", "wait"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[1]"), "stdout was: {stdout}");
    assert!(stdout.contains("Running"), "stdout was: {stdout}");
    assert!(output.status.success(), "exit code was not 0");
}

#[test]
fn fg_exits_zero_for_clean_background_job() {
    // Tests `fg` with no argument — uses most-recently-added job.
    let output = run_shell(&[quick_exit_background_command(), "fg", "echo FG:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FG:0"), "stdout was: {stdout}");
}

#[test]
fn fg_with_explicit_job_id_returns_exit_code() {
    // Tests `fg %1` — explicit %N job-id syntax through resolve_job_id.
    let output = run_shell(&[quick_exit_background_command(), "fg %1", "echo FG:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FG:0"), "stdout was: {stdout}");
}

#[test]
fn wait_accepts_percent_job_id() {
    // Tests `wait %1` — explicit %N job-id argument vs bare `wait`.
    let output = run_shell(&[failing_background_command(), "wait %1", "echo WAIT:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WAIT:7"), "stdout was: {stdout}");
}

#[test]
fn multiple_background_jobs_all_appear_in_jobs_output() {
    let output = run_shell(&[
        long_background_command(),
        long_background_command(),
        "jobs",
        "wait",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[1]"), "stdout was: {stdout}");
    assert!(stdout.contains("[2]"), "stdout was: {stdout}");
}
