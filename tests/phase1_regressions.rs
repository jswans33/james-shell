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

fn run_shell_with_env(lines: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_james-shell");
    let mut command = Command::new(bin);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        ;

    for (k, v) in envs {
        command.env(k, v);
    }

    let mut child = command.spawn().expect("spawn james-shell");

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

#[test]
fn cd_failed_directory_does_not_update_oldpwd() {
    let root = std::env::temp_dir().join(format!("jsh_oldpwd_reg_{}", std::process::id()));
    let orig_dir = root.join("orig");
    let valid_dir = root.join("valid");
    let missing_dir = root.join("missing");
    let orig = orig_dir.to_string_lossy().to_string();
    let valid = valid_dir.to_string_lossy().to_string();
    let missing = missing_dir.to_string_lossy().to_string();

    std::fs::create_dir_all(&orig_dir).unwrap();
    std::fs::create_dir_all(&valid_dir).unwrap();

    let output = run_shell_with_env(
        &[
            "cd \"$JSH_ORIG_DIR\"",
            "cd \"$JSH_VALID_DIR\"",
            "cd \"$JSH_MISSING_DIR\"",
            "pwd",
            "echo OLDPWD:$OLDPWD",
        ],
        &[
            ("JSH_ORIG_DIR", orig.as_str()),
            ("JSH_VALID_DIR", valid.as_str()),
            ("JSH_MISSING_DIR", missing.as_str()),
        ],
    );

    let _ = std::fs::remove_dir_all(&root);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains(&format!("OLDPWD:{orig}")),
        "stdout was: {stdout}"
    );
}

#[test]
fn stateful_builtin_in_nonterminal_pipeline_is_rejected() {
    let root = std::env::temp_dir().join(format!("jsh_pipeline_builtin_reg_{}", std::process::id()));
    let valid_dir = root.join("valid");
    let valid = valid_dir.to_string_lossy().to_string();
    std::fs::create_dir_all(&valid_dir).unwrap();

    let output = run_shell_with_env(
        &["cd \"$JSH_VALID_DIR\" | echo DONE", "echo PIPE:$?"],
        &[("JSH_VALID_DIR", valid.as_str())],
    );

    let _ = std::fs::remove_dir_all(&root);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("builtin 'cd' is not supported in non-terminal pipeline positions"),
        "stderr was: {stderr}"
    );
    assert!(stdout.contains("PIPE:1"), "stdout was: {stdout}");
    assert!(!stdout.contains("DONE"), "stdout was: {stdout}");
}

#[test]
fn stateful_builtin_export_in_nonterminal_pipeline_is_rejected() {
    let output = run_shell(&["export FOO=bar | echo DONE", "echo PIPE:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("builtin 'export' is not supported in non-terminal pipeline positions"),
        "stderr was: {stderr}"
    );
    assert!(stdout.contains("PIPE:1"), "stdout was: {stdout}");
    assert!(!stdout.contains("DONE"), "stdout was: {stdout}");
}

#[test]
fn nonterminal_pure_builtins_are_allowed() {
    let output = run_shell(&["echo payload | pwd | echo PIPE"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "nonterminal pure builtin pipeline failed: status={:?}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("PIPE"), "stdout was: {stdout}");
}

#[test]
fn background_builtin_warning_is_emitted_once() {
    let output = run_shell(&["pwd | echo PIPELINE &", "echo PIPE:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let warning_count = stderr.matches("does not support background execution").count();
    assert_eq!(
        warning_count,
        1,
        "expected exactly one background builtin warning, saw {warning_count}; stderr={stderr}"
    );
    assert!(
        stdout.contains("PIPE:0"),
        "stdout was: {stdout}"
    );
}

#[test]
fn pipeline_stdout_redirect_on_nonterminal_command_is_rejected() {
    let output_file = std::env::temp_dir().join(format!(
        "jsh_pipeline_redir_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let redirect_cmd = format!(
        "echo hi > \"{}\" | echo ignored",
        output_file.to_string_lossy().replace('\\', "/")
    );
    let commands = vec![redirect_cmd.as_str(), "echo PIPE:$?"];
    let output = run_shell(commands.as_slice());

    let _ = std::fs::remove_file(&output_file);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output_file.exists(), "redirect file should not be created on rejection");
    assert!(
        stderr.contains("cannot redirect stdout of non-terminal pipeline command"),
        "stderr was: {stderr}"
    );
    assert!(stdout.contains("PIPE:1"), "stdout was: {stdout}");
}

#[test]
fn cd_minus_ignores_failed_cd_for_oldpwd() {
    let root = std::env::temp_dir().join(format!("jsh_cdminus_reg_{}", std::process::id()));
    let base_dir = root.join("base");
    let alt_dir = root.join("alt");
    let missing_dir = root.join("missing");
    let base = base_dir.to_string_lossy().to_string();
    let alt = alt_dir.to_string_lossy().to_string();
    let missing = missing_dir.to_string_lossy().to_string();

    std::fs::create_dir_all(&base_dir).unwrap();
    std::fs::create_dir_all(&alt_dir).unwrap();

    let output = run_shell_with_env(
        &[
            "cd \"$JSH_BASE\"",
            "cd \"$JSH_ALT\"",
            "cd \"$JSH_MISSING\"",
            "cd -",
            "pwd",
            "echo CDM:$?",
        ],
        &[
            ("JSH_BASE", base.as_str()),
            ("JSH_ALT", alt.as_str()),
            ("JSH_MISSING", missing.as_str()),
        ],
    );

    let _ = std::fs::remove_dir_all(&root);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.contains(&base),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("CDM:0"),
        "stdout was: {stdout}"
    );
    assert!(
        stderr.contains("cd:"),
        "stderr was: {stderr}"
    );
}

#[test]
fn builtin_background_prints_warning_and_runs_foreground() {
    let output = run_shell(&["pwd &", "echo FG:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("does not support background execution"),
        "stderr was: {stderr}"
    );
    assert!(stdout.contains("FG:0"), "stdout was: {stdout}");
}

#[test]
fn pipeline_with_background_builtin_warns_and_runs_foreground() {
    let output = run_shell(&["pwd | echo PIPELINE &", "echo PIPE:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("does not support background execution"),
        "stderr was: {stderr}"
    );
    assert!(
        stdout.contains("PIPE:0"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("PIPELINE"), "stdout was: {stdout}");
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

#[test]
fn help_no_args_lists_builtins() {
    let output = run_shell(&["help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cd"), "stdout was: {stdout}");
    assert!(stdout.contains("echo"), "stdout was: {stdout}");
    assert!(stdout.contains("exit"), "stdout was: {stdout}");
    assert!(stdout.contains("Topics:"), "stdout was: {stdout}");
    assert!(output.status.success(), "exit code was not 0");
}

#[test]
fn help_builtin_name_shows_usage() {
    let output = run_shell(&["help cd"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cd"), "stdout was: {stdout}");
    assert!(stdout.contains("OLDPWD"), "stdout was: {stdout}");
    assert!(output.status.success(), "exit code was not 0");
}

#[test]
fn help_topic_shows_section() {
    let output = run_shell(&["help redirection"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2>&1"), "stdout was: {stdout}");
    assert!(stdout.contains("<<<"), "stdout was: {stdout}");
    assert!(output.status.success(), "exit code was not 0");
}

#[test]
fn help_unknown_topic_exits_nonzero() {
    let output = run_shell(&["help nonexistent_topic_xyzzy", "echo AFTER:$?"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("AFTER:1"), "stdout was: {stdout}");
    assert!(stderr.contains("nonexistent_topic_xyzzy"), "stderr was: {stderr}");
}
