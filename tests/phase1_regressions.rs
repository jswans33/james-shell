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

#[test]
fn builtin_to_external_pipeline_outputs() {
    let output = run_shell(&["echo hello | sort"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello"), "stdout was: {stdout}");
}

#[test]
fn builtin_stdin_redirection_is_accepted() {
    let temp_dir = std::env::temp_dir().join(format!(
        "jsh_builtin_stdin_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("input.txt");
    std::fs::write(&input_path, "ignored").unwrap();

    let cmd = format!("pwd < {}", input_path.display());
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
fn external_stderr_pipes_into_next_command_unix() {
    let output = run_shell(&["sh -c 'echo err 1>&2' 2>&1 | sort"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("err"), "stdout was: {stdout}");
    assert!(!stderr.contains("err"), "stderr was: {stderr}");
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
