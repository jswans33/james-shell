/// Integration tests for Module 10 — Line Editing & History.
///
/// Each test uses its own isolated temp HOME directory so concurrent test runs
/// cannot race on the shared `~/.jsh_history` file.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs::File;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Spawn the shell with `HOME`/`USERPROFILE` overridden to `home`,
/// feed `lines` via stdin (followed by `exit`), and return the full output.
fn run_shell_with_home(lines: &[&str], home: &Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_james-shell"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", home)
        .env("USERPROFILE", home)
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

fn run_shell_with_home_from_script_file(
    script_path: &Path,
    home: &Path,
) -> std::process::Output {
    let stdin = File::open(script_path).expect("open script file");
    let output = Command::new(env!("CARGO_BIN_EXE_james-shell"))
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()
        .expect("run james-shell from script file");

    output
}

/// RAII temp directory — created on construction, deleted on drop.
struct TempHome(PathBuf);

impl TempHome {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "jsh_test_home_{label}_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&dir).expect("create temp home");
        TempHome(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn history_path(&self) -> PathBuf {
        self.0.join(".jsh_history")
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn history_file_written_after_command() {
    let home = TempHome::new("written");
    let marker = "echo HISTORY_WRITTEN_MARKER";

    let output = run_shell_with_home(&[marker], home.path());
    assert!(output.status.success(), "shell did not exit cleanly");

    let path = home.history_path();
    assert!(path.exists(), ".jsh_history was not created");

    let contents = std::fs::read_to_string(&path).expect("read .jsh_history");
    assert!(
        contents.contains(marker),
        "expected marker in history; contents:\n{contents}"
    );
}

#[test]
fn script_file_input_is_fully_consumed_without_raw_mode() {
    let home = TempHome::new("script_file");
    let script = home.path().join("session.jsh");
    std::fs::write(
        &script,
        ["echo SCRIPT_FILE_MARKER", "echo SECOND:$?"]
            .join("\n"),
    )
    .expect("write script input");

    let output = run_shell_with_home_from_script_file(&script, home.path());
    assert!(output.status.success(), "script-fed shell did not exit cleanly");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout
        .find("SCRIPT_FILE_MARKER")
        .expect("expected marker output");
    let second = stdout.find("SECOND:0").expect("expected exit-code output");
    assert!(
        first < second,
        "commands did not run in order; stdout was:\n{stdout}"
    );
    // Clean exit is verified by output.status.success() above.
    // "Goodbye!" is only printed for interactive (TTY) sessions; non-interactive
    // script mode exits cleanly without it.
}

#[test]
fn script_file_input_has_no_terminal_control_sequences() {
    let home = TempHome::new("script_no_ansi");
    let script = home.path().join("plain.jsh");
    std::fs::write(&script, "echo plain-output").expect("write script input");

    let output = run_shell_with_home_from_script_file(&script, home.path());
    assert!(output.status.success(), "script-fed shell did not exit cleanly");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "non-interactive output should not include ANSI escapes; stdout was:\n{stdout}"
    );
}

#[test]
fn history_persists_across_sessions() {
    let home = TempHome::new("persists");
    let s1_marker = "echo SESSION1_HISTORY_MARKER";
    let s2_marker = "echo SESSION2_HISTORY_MARKER";

    // Session 1: write a distinctive command to disk.
    let out1 = run_shell_with_home(&[s1_marker], home.path());
    assert!(out1.status.success(), "session 1 did not exit cleanly");

    // Session 2: a completely fresh process that must load the persisted file
    // on startup and then append its own command without overwriting.
    let out2 = run_shell_with_home(&[s2_marker], home.path());
    assert!(out2.status.success(), "session 2 did not exit cleanly");

    // After session 2 both entries must be present — session 1's entry
    // survived (load_history didn't overwrite) and session 2's entry was
    // appended (proving append-mode persistence).
    let contents = std::fs::read_to_string(home.history_path())
        .expect("read .jsh_history after session 2");
    assert!(
        contents.contains(s1_marker),
        "session 1 entry missing after session 2;\ncontents:\n{contents}"
    );
    assert!(
        contents.contains(s2_marker),
        "session 2 entry missing from history;\ncontents:\n{contents}"
    );
}

#[test]
fn history_file_is_appended_instead_of_overwritten() {
    let home = TempHome::new("history_append");
    let marker = "echo SESSION_APPEND_MARKER";
    let seeded_entry = "seeded-history-line";
    std::fs::write(home.history_path(), format!("{seeded_entry}\n"))
        .expect("seed history file");

    let output = run_shell_with_home(&[marker], home.path());
    assert!(output.status.success(), "shell did not exit cleanly");

    let contents = std::fs::read_to_string(home.history_path())
        .expect("read .jsh_history after session");
    let lines: Vec<_> = contents.lines().collect();
    assert!(
        lines.first() == Some(&seeded_entry),
        "expected seeded entry to remain first; contents:\n{contents}"
    );
    assert!(
        lines.contains(&"echo SESSION_APPEND_MARKER"),
        "append marker missing; contents:\n{contents}"
    );
}

#[test]
fn empty_commands_not_written_to_history() {
    let home = TempHome::new("empty");
    // Send a valid command, then blank lines that should be filtered out.
    let _ = run_shell_with_home(&["echo sentinel", "", "   "], home.path());

    let contents = std::fs::read_to_string(home.history_path())
        .expect("read .jsh_history");
    // No blank entries should appear in the file.
    assert!(
        !contents.lines().any(|l| l.trim().is_empty()),
        "blank line found in history file:\n{contents}"
    );
}
