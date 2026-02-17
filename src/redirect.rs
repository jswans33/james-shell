/// What a file descriptor should be connected to.
#[derive(Debug, Clone)]
pub enum RedirectTarget {
    /// Write to file (truncate)
    File(String),
    /// Write to file (append)
    FileAppend(String),
    /// Read from file
    FileRead(String),
    /// Duplicate another fd (e.g., 2>&1)
    Fd(i32),
    /// Feed a string as stdin
    HereString(String),
}

/// A single I/O redirection instruction.
#[derive(Debug, Clone)]
pub struct Redirection {
    pub fd: i32,
    pub target: RedirectTarget,
}

/// Separate redirect operators from regular arguments.
/// Returns (args, redirections) or an error message for syntax errors.
///
/// Handles: >, >>, <, <<<, 2>, 2>>, >&N, N>&M
/// Also merges a standalone digit before > into a fd-prefixed redirect (e.g., "2" ">" → 2>).
pub fn extract_redirections(tokens: &[String]) -> Result<(Vec<String>, Vec<Redirection>), String> {
    let mut args = Vec::new();
    let mut redirections = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        let token = &tokens[i];

        // Check for fd-prefixed redirects: "2>" "2>>"
        if let Some(rest) = token.strip_prefix("2>") {
            if rest == "&1" {
                redirections.push(Redirection { fd: 2, target: RedirectTarget::Fd(1) });
            } else if rest == ">" {
                // 2>> (append stderr)
                i += 1;
                let path = expect_filename(i, tokens, "2>>")?;
                redirections.push(Redirection { fd: 2, target: RedirectTarget::FileAppend(path) });
            } else if rest.is_empty() {
                // 2> file
                i += 1;
                let path = expect_filename(i, tokens, "2>")?;
                redirections.push(Redirection { fd: 2, target: RedirectTarget::File(path) });
            } else {
                // 2>something — treat as 2> with filename attached
                redirections.push(Redirection { fd: 2, target: RedirectTarget::File(rest.to_string()) });
            }
        } else if let Some(rest) = token.strip_prefix("1>") {
            if rest == "&2" {
                redirections.push(Redirection { fd: 1, target: RedirectTarget::Fd(2) });
            } else if rest == ">" {
                i += 1;
                let path = expect_filename(i, tokens, "1>>")?;
                redirections.push(Redirection { fd: 1, target: RedirectTarget::FileAppend(path) });
            } else if rest.is_empty() {
                i += 1;
                let path = expect_filename(i, tokens, "1>")?;
                redirections.push(Redirection { fd: 1, target: RedirectTarget::File(path) });
            } else {
                redirections.push(Redirection { fd: 1, target: RedirectTarget::File(rest.to_string()) });
            }
        } else if token == ">" {
            i += 1;
            let path = expect_filename(i, tokens, ">")?;
            redirections.push(Redirection { fd: 1, target: RedirectTarget::File(path) });
        } else if token == ">>" {
            i += 1;
            let path = expect_filename(i, tokens, ">>")?;
            redirections.push(Redirection { fd: 1, target: RedirectTarget::FileAppend(path) });
        } else if token == ">&1" {
            // standalone >&1 — stdout to stdout (no-op, but for completeness)
            redirections.push(Redirection { fd: 1, target: RedirectTarget::Fd(1) });
        } else if token == ">&2" {
            redirections.push(Redirection { fd: 1, target: RedirectTarget::Fd(2) });
        } else if token == "<" {
            i += 1;
            let path = expect_filename(i, tokens, "<")?;
            redirections.push(Redirection { fd: 0, target: RedirectTarget::FileRead(path) });
        } else if token == "<<<" {
            i += 1;
            let text = expect_filename(i, tokens, "<<<")?;
            redirections.push(Redirection { fd: 0, target: RedirectTarget::HereString(text) });
        } else {
            args.push(token.clone());
        }

        i += 1;
    }

    Ok((args, redirections))
}

fn expect_filename(i: usize, tokens: &[String], operator: &str) -> Result<String, String> {
    if i < tokens.len() {
        Ok(tokens[i].clone())
    } else {
        Err(format!("jsh: syntax error: expected filename after '{operator}'"))
    }
}

/// Check if a path refers to a null device (cross-platform).
pub fn is_null_device(path: &str) -> bool {
    if cfg!(windows) {
        path.eq_ignore_ascii_case("NUL") || path.eq_ignore_ascii_case("/dev/null")
    } else {
        path == "/dev/null"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_stdout_redirect() {
        let (args, redirs) = extract_redirections(
            ["echo", "hello", ">", "out.txt"].map(String::from).as_ref(),
        ).unwrap();
        assert_eq!(args, vec!["echo", "hello"]);
        assert_eq!(redirs.len(), 1);
        assert_eq!(redirs[0].fd, 1);
        assert!(matches!(&redirs[0].target, RedirectTarget::File(p) if p == "out.txt"));
    }

    #[test]
    fn append_redirect() {
        let (args, redirs) = extract_redirections(
            ["echo", "hello", ">>", "out.txt"].map(String::from).as_ref(),
        ).unwrap();
        assert_eq!(args, vec!["echo", "hello"]);
        assert!(matches!(&redirs[0].target, RedirectTarget::FileAppend(p) if p == "out.txt"));
    }

    #[test]
    fn stdin_redirect() {
        let (args, redirs) = extract_redirections(
            ["sort", "<", "data.txt"].map(String::from).as_ref(),
        ).unwrap();
        assert_eq!(args, vec!["sort"]);
        assert!(matches!(&redirs[0].target, RedirectTarget::FileRead(p) if p == "data.txt"));
        assert_eq!(redirs[0].fd, 0);
    }

    #[test]
    fn stderr_redirect() {
        let (args, redirs) = extract_redirections(
            ["ls", "/bad", "2>", "err.txt"].map(String::from).as_ref(),
        ).unwrap();
        assert_eq!(args, vec!["ls", "/bad"]);
        assert_eq!(redirs[0].fd, 2);
        assert!(matches!(&redirs[0].target, RedirectTarget::File(p) if p == "err.txt"));
    }

    #[test]
    fn stderr_to_stdout() {
        let (args, redirs) = extract_redirections(
            ["ls", "2>&1"].map(String::from).as_ref(),
        ).unwrap();
        assert_eq!(args, vec!["ls"]);
        assert_eq!(redirs[0].fd, 2);
        assert!(matches!(&redirs[0].target, RedirectTarget::Fd(1)));
    }

    #[test]
    fn here_string() {
        let (args, redirs) = extract_redirections(
            ["cat", "<<<", "hello world"].map(String::from).as_ref(),
        ).unwrap();
        assert_eq!(args, vec!["cat"]);
        assert!(matches!(&redirs[0].target, RedirectTarget::HereString(s) if s == "hello world"));
    }

    #[test]
    fn missing_filename_is_error() {
        let result = extract_redirections(["echo", ">"].map(String::from).as_ref());
        assert!(result.is_err());
    }

    #[test]
    fn multiple_redirections() {
        let (args, redirs) = extract_redirections(
            ["cmd", ">", "out.txt", "2>", "err.txt", "<", "in.txt"]
                .map(String::from).as_ref(),
        ).unwrap();
        assert_eq!(args, vec!["cmd"]);
        assert_eq!(redirs.len(), 3);
    }

    #[test]
    fn null_device_detection() {
        assert!(is_null_device("/dev/null"));
        if cfg!(windows) {
            assert!(is_null_device("NUL"));
            assert!(is_null_device("nul"));
        }
    }
}
