/// Integration tests for Module 10 — Line Editing & History.
///
/// Each test uses its own isolated temp HOME directory so concurrent test runs
/// cannot race on the shared `~/.jsh_history` file.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// RAII temp directory — created on construction, deleted on drop.
struct TempHome(PathBuf);

impl TempHome {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("jsh_test_home_{label}"));
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
fn history_persists_across_sessions() {
    let home = TempHome::new("persists");
    let marker = "echo HISTORY_PERSISTENT_MARKER";

    // Session 1: run the distinctive command.
    let _ = run_shell_with_home(&[marker], home.path());

    // Session 2: a fresh shell instance must still find the entry on disk.
    let contents = std::fs::read_to_string(home.history_path())
        .expect("read .jsh_history after second session");
    assert!(
        contents.contains(marker),
        "history should persist across sessions; contents:\n{contents}"
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
