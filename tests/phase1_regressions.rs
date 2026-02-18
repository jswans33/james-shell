use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

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

fn assert_builtin_large_payload_roundtrip(pipeline_cmd: &str) {
    let payload_len = 200_000;
    let payload = "x".repeat(payload_len);
    let export = format!("export BIG={payload}");
    let output = run_shell(&[export.as_str(), pipeline_cmd]);

    assert!(
        output.status.success(),
        "shell command failed: status={:?}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let streamed_x = output.stdout.iter().filter(|&&byte| byte == b'x').count();

    assert!(
        streamed_x >= payload_len,
        "streamed x count was {streamed_x}, expected at least {payload_len}"
    );
}

fn assert_large_payload_roundtrip_returns_quickly(pipeline_cmd: &str) {
    // Regression guard for builtin-before-external pipeline deadlock.
    // If builtin output is written synchronously before downstream reader starts,
    // large payloads can block forever on pipe backpressure.
    let start = std::time::Instant::now();
    assert_builtin_large_payload_roundtrip(pipeline_cmd);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "pipeline appeared to hang; took {:.2?}",
        elapsed
    );
}

#[test]
fn builtin_to_external_pipeline_outputs() {
    let output = run_shell(&["echo hello | sort"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello"), "stdout was: {stdout}");
}

#[test]
fn builtin_stdin_redirection_is_accepted() {
    let temp_dir = std::env::temp_dir().join(format!("jsh_builtin_stdin_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("input.txt");
    std::fs::write(&input_path, "ignored").unwrap();

    let input_file = input_path.to_string_lossy().replace('\\', "/");
    let cmd = format!("pwd < \"{input_file}\"");
    let output = run_shell(&[cmd.as_str(), "echo DONE:$?"]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("DONE:0"), "stdout was: {stdout}");
    assert!(
        !stderr.contains("unsupported redirection"),
        "stderr was: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[cfg(unix)]
#[test]
fn builtin_to_external_large_output_does_not_hang_unix() {
    assert_large_payload_roundtrip_returns_quickly("echo $BIG | sort");
}

#[test]
#[cfg(unix)]
fn external_stderr_pipes_into_next_command_unix() {
    let output = run_shell(&["sh -c 'echo err 1>&2' 2>&1 | sort"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("err"), "stdout was: {stdout}");
    assert!(!stderr.contains("err"), "stderr was: {stderr}");
}

#[cfg(windows)]
#[test]
fn builtin_to_external_large_output_does_not_hang_windows() {
    assert_large_payload_roundtrip_returns_quickly("echo $BIG | cmd /C more");
}

#[cfg(windows)]
#[test]
fn external_stderr_pipes_into_next_command_windows() {
    let output = run_shell(&["cmd /C \"echo err 1>&2\" 2>&1 | sort"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("err"), "stdout was: {stdout}");
    assert!(!stderr.contains("err"), "stderr was: {stderr}");
}
