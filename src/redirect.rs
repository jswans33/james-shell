use crate::expander;
use crate::parser::{Word, WordSegment};

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
                redirections.push(Redirection {
                    fd: 2,
                    target: RedirectTarget::Fd(1),
                });
            } else if rest == ">" {
                // 2>> (append stderr)
                i += 1;
                let path = expect_filename(i, tokens, "2>>")?;
                redirections.push(Redirection {
                    fd: 2,
                    target: RedirectTarget::FileAppend(path),
                });
            } else if rest.is_empty() {
                // 2> file
                i += 1;
                let path = expect_filename(i, tokens, "2>")?;
                redirections.push(Redirection {
                    fd: 2,
                    target: RedirectTarget::File(path),
                });
            } else {
                // 2>something — treat as 2> with filename attached
                redirections.push(Redirection {
                    fd: 2,
                    target: RedirectTarget::File(rest.to_string()),
                });
            }
        } else if let Some(rest) = token.strip_prefix("1>") {
            if rest == "&2" {
                redirections.push(Redirection {
                    fd: 1,
                    target: RedirectTarget::Fd(2),
                });
            } else if rest == ">" {
                i += 1;
                let path = expect_filename(i, tokens, "1>>")?;
                redirections.push(Redirection {
                    fd: 1,
                    target: RedirectTarget::FileAppend(path),
                });
            } else if rest.is_empty() {
                i += 1;
                let path = expect_filename(i, tokens, "1>")?;
                redirections.push(Redirection {
                    fd: 1,
                    target: RedirectTarget::File(path),
                });
            } else {
                redirections.push(Redirection {
                    fd: 1,
                    target: RedirectTarget::File(rest.to_string()),
                });
            }
        } else if token == ">" {
            i += 1;
            let path = expect_filename(i, tokens, ">")?;
            redirections.push(Redirection {
                fd: 1,
                target: RedirectTarget::File(path),
            });
        } else if token == ">>" {
            i += 1;
            let path = expect_filename(i, tokens, ">>")?;
            redirections.push(Redirection {
                fd: 1,
                target: RedirectTarget::FileAppend(path),
            });
        } else if token == ">&1" {
            // standalone >&1 — stdout to stdout (no-op, but for completeness
            redirections.push(Redirection {
                fd: 1,
                target: RedirectTarget::Fd(1),
            });
        } else if token == ">&2" {
            redirections.push(Redirection {
                fd: 1,
                target: RedirectTarget::Fd(2),
            });
        } else if token == "<" {
            i += 1;
            let path = expect_filename(i, tokens, "<")?;
            redirections.push(Redirection {
                fd: 0,
                target: RedirectTarget::FileRead(path),
            });
        } else if token == "<<<" {
            i += 1;
            let text = expect_filename(i, tokens, "<<<")?;
            redirections.push(Redirection {
                fd: 0,
                target: RedirectTarget::HereString(text),
            });
        } else {
            args.push(token.clone());
        }

        i += 1;
    }

    Ok((args, redirections))
}

/// Separate redirect operators from parsed words.
/// Quote-aware: operators hidden behind escapes or quotes are not treated as redirections.
pub fn extract_redirections_from_words(
    words: &[Word],
    last_exit_code: i32,
) -> Result<(Vec<Word>, Vec<Redirection>), String> {
    let mut args = Vec::new();
    let mut redirections = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let redir = parse_redirect_word(&words[i]);
        if let Some(op) = redir {
            i = apply_parsed_redirect(&mut redirections, op, words, i, last_exit_code, false)?;
            continue;
        }

        // Support spaced fd-prefixed redirects like: `2 > file`, `2 >> file`, `2 >&1`.
        if let Some(fd) = parse_standalone_fd_prefix(&words[i]) {
            if let Some(op) = words
                .get(i + 1)
                .and_then(|word| {
                    if word.len() == 1 {
                        parse_unprefixed_redirect_word(&word[0])
                    } else {
                        None
                    }
                })
            {
                i = apply_spaced_prefixed_redirect(
                    &mut redirections,
                    fd,
                    op,
                    words,
                    i,
                    last_exit_code,
                )?;
                continue;
            }
        }

        args.push(words[i].clone());
        i += 1;
    }

    Ok((args, redirections))
}

#[derive(Debug)]
enum ParsedRedirect {
    File { fd: i32, append: bool },
    FileRead,
    HereString,
    Duplicate { fd: i32, target: i32 },
    FileWithAttachedPath {
        fd: i32,
        append: bool,
        path: String,
    },
}

fn parse_redirect_word(word: &Word) -> Option<ParsedRedirect> {
    if word.len() != 1 {
        return None;
    }

    let token = match &word[0] {
        WordSegment::Unquoted(s) => s.as_str(),
        _ => return None,
    };

    parse_unprefixed_redirect_word(&word[0])
        .or_else(|| parse_prefixed_redirect(token))
}

fn parse_unprefixed_redirect_word(segment: &WordSegment) -> Option<ParsedRedirect> {
    let token = match segment {
        WordSegment::Unquoted(s) => s.as_str(),
        _ => return None,
    };

    match token {
        ">" => Some(ParsedRedirect::File { fd: 1, append: false }),
        ">>" => Some(ParsedRedirect::File { fd: 1, append: true }),
        "<" => Some(ParsedRedirect::FileRead),
        "<<<" => Some(ParsedRedirect::HereString),
        ">&1" => Some(ParsedRedirect::Duplicate { fd: 1, target: 1 }),
        ">&2" => Some(ParsedRedirect::Duplicate { fd: 1, target: 2 }),
        _ => None
    }
    .or_else(|| parse_prefixed_redirect(token))
}

fn parse_prefixed_redirect(token: &str) -> Option<ParsedRedirect> {
    let (fd_char, rest) = token.chars().next().map(|c| (c, &token[1..]))?;
    let fd = match fd_char {
        '1' => 1,
        '2' => 2,
        _ => return None,
    };

    if let Some(path) = rest.strip_prefix(">&") {
        let target = path.parse::<i32>().ok()?;
        return Some(ParsedRedirect::Duplicate { fd, target });
    }

    if rest == ">" {
        return Some(ParsedRedirect::File { fd, append: false });
    }
    if rest == ">>" {
        return Some(ParsedRedirect::File { fd, append: true });
    }

    if let Some(path) = rest.strip_prefix(">") {
        if let Some(path) = path.strip_prefix(">") {
            return Some(ParsedRedirect::FileWithAttachedPath {
                fd,
                append: true,
                path: path.to_string(),
            });
        }

        return Some(ParsedRedirect::FileWithAttachedPath {
            fd,
            append: false,
            path: path.to_string(),
        });
    }

    None
}

fn parse_standalone_fd_prefix(word: &Word) -> Option<i32> {
    if word.len() != 1 {
        return None;
    }

    let token = match &word[0] {
        WordSegment::Unquoted(s) => s.as_str(),
        _ => return None,
    };

    if token.len() != 1 {
        return None;
    }

    token.parse::<i32>().ok()
}

fn normalize_redirection_op(fd: i32, op: ParsedRedirect) -> ParsedRedirect {
    match op {
        ParsedRedirect::File { append, .. } => ParsedRedirect::File {
            fd,
            append,
        },
        ParsedRedirect::Duplicate { fd: _, target } => ParsedRedirect::Duplicate { fd, target },
        ParsedRedirect::FileRead => ParsedRedirect::FileRead,
        ParsedRedirect::HereString => ParsedRedirect::HereString,
        ParsedRedirect::FileWithAttachedPath { append, path, .. } => ParsedRedirect::FileWithAttachedPath {
            fd,
            append,
            path,
        },
    }
}

fn apply_parsed_redirect(
    redirections: &mut Vec<Redirection>,
    op: ParsedRedirect,
    words: &[Word],
    idx: usize,
    last_exit_code: i32,
    spaced: bool,
) -> Result<usize, String> {
    let increment = if spaced { 2 } else { 1 };
    let next = if spaced { idx + 2 } else { idx + 1 };
    match op {
        ParsedRedirect::File { fd, append: false } => {
            let path = extract_target(words, idx + increment, "redirection target", last_exit_code)?;
            redirections.push(Redirection {
                fd,
                target: RedirectTarget::File(path),
            });
            Ok(idx + increment + 1)
        }
        ParsedRedirect::File { fd, append: true } => {
            let path = extract_target(words, idx + increment, "redirection target", last_exit_code)?;
            redirections.push(Redirection {
                fd,
                target: RedirectTarget::FileAppend(path),
            });
            Ok(idx + increment + 1)
        }
        ParsedRedirect::Duplicate { fd, target } => {
            redirections.push(Redirection { fd, target: RedirectTarget::Fd(target) });
            Ok(next)
        }
        ParsedRedirect::FileRead => {
            let path = extract_target(words, idx + increment, "redirection target", last_exit_code)?;
            redirections.push(Redirection {
                fd: 0,
                target: RedirectTarget::FileRead(path),
            });
            Ok(idx + increment + 1)
        }
        ParsedRedirect::HereString => {
            let text = extract_target(words, idx + increment, "here-string target", last_exit_code)?;
            redirections.push(Redirection {
                fd: 0,
                target: RedirectTarget::HereString(text),
            });
            Ok(idx + increment + 1)
        }
        ParsedRedirect::FileWithAttachedPath { fd, append, path } => {
            let target = if append {
                RedirectTarget::FileAppend(path)
            } else {
                RedirectTarget::File(path)
            };
            redirections.push(Redirection { fd, target });
            Ok(next)
        }
    }
}

fn apply_spaced_prefixed_redirect(
    redirections: &mut Vec<Redirection>,
    fd: i32,
    op: ParsedRedirect,
    words: &[Word],
    idx: usize,
    last_exit_code: i32,
) -> Result<usize, String> {
    let adjusted = normalize_redirection_op(fd, op);
    apply_parsed_redirect(redirections, adjusted, words, idx, last_exit_code, true)
}

fn extract_target(
    words: &[Word],
    idx: usize,
    context: &str,
    last_exit_code: i32,
) -> Result<String, String> {
    if idx >= words.len() {
        return Err(format!(
            "jsh: syntax error: expected filename after {context}"
        ));
    }

    let expanded = expander::expand_words(&[words[idx].clone()], last_exit_code);

    match expanded.as_slice() {
        [] => Err(format!("jsh: syntax error: expected filename after {context}")),
        [one] => Ok(one.clone()),
        _ => Err("jsh: ambiguous redirect target".to_string()),
    }
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
        )
        .unwrap();
        assert_eq!(args, vec!["echo", "hello"]);
        assert_eq!(redirs.len(), 1);
        assert_eq!(redirs[0].fd, 1);
        assert!(matches!(&redirs[0].target, RedirectTarget::File(p) if p == "out.txt"));
    }

    #[test]
    fn append_redirect() {
        let (args, redirs) = extract_redirections(
            ["echo", "hello", ">>", "out.txt"].map(String::from).as_ref(),
        )
        .unwrap();
        assert_eq!(args, vec!["echo", "hello"]);
        assert_eq!(redirs.len(), 1);
        assert!(matches!(
            &redirs[0].target,
            RedirectTarget::FileAppend(p) if p == "out.txt"
        ));
    }

    #[test]
    fn stdin_redirect() {
        let (args, redirs) = extract_redirections(
            ["sort", "<", "data.txt"].map(String::from).as_ref(),
        )
        .unwrap();
        assert_eq!(args, vec!["sort"]);
        assert!(matches!(&redirs[0].target, RedirectTarget::FileRead(p) if p == "data.txt"));
        assert_eq!(redirs[0].fd, 0);
    }

    #[test]
    fn stderr_redirect() {
        let (args, redirs) = extract_redirections(
            ["ls", "/bad", "2>", "err.txt"].map(String::from).as_ref(),
        )
        .unwrap();
        assert_eq!(args, vec!["ls", "/bad"]);
        assert_eq!(redirs[0].fd, 2);
        assert!(matches!(
            &redirs[0].target,
            RedirectTarget::File(p) if p == "err.txt"
        ));
    }

    #[test]
    fn stderr_to_stdout() {
        let (args, redirs) = extract_redirections(["ls", "2>&1"].map(String::from).as_ref()).unwrap();
        assert_eq!(args, vec!["ls"]);
        assert_eq!(redirs[0].fd, 2);
        assert!(matches!(&redirs[0].target, RedirectTarget::Fd(1)));
    }

    #[test]
    fn here_string() {
        let (args, redirs) = extract_redirections(
            ["cat", "<<<", "hello world"].map(String::from).as_ref(),
        )
        .unwrap();
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
                .map(String::from)
                .as_ref(),
        )
        .unwrap();
        assert_eq!(args, vec!["cmd"]);
        assert_eq!(redirs.len(), 3);
    }

    #[test]
    fn spaced_stderr_redirect() {
        let parsed = crate::parser::tokenize("printf hi 2 > err.txt").unwrap();
        let (args, redirs) =
            extract_redirections_from_words(&parsed, 0).expect("parse");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], vec![WordSegment::Unquoted("printf".into())]);
        assert_eq!(args[1], vec![WordSegment::Unquoted("hi".into())]);
        assert_eq!(redirs[0].fd, 2);
        assert!(matches!(&redirs[0].target, RedirectTarget::File(p) if p == "err.txt"));
    }

    #[test]
    fn spaced_stderr_append_redirect() {
        let parsed = crate::parser::tokenize("printf hi 2 >> err.txt").unwrap();
        let (args, redirs) =
            extract_redirections_from_words(&parsed, 0).expect("parse");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], vec![WordSegment::Unquoted("printf".into())]);
        assert_eq!(args[1], vec![WordSegment::Unquoted("hi".into())]);
        assert_eq!(redirs[0].fd, 2);
        assert!(matches!(
            &redirs[0].target,
            RedirectTarget::FileAppend(p) if p == "err.txt"
        ));
    }

    #[test]
    fn spaced_fd_dup_redirect() {
        let parsed = crate::parser::tokenize("cmd 2 >&1").unwrap();
        let (args, redirs) =
            extract_redirections_from_words(&parsed, 0).expect("parse");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], vec![WordSegment::Unquoted("cmd".into())]);
        assert_eq!(redirs[0].fd, 2);
        assert!(matches!(&redirs[0].target, RedirectTarget::Fd(1)));
    }

    #[test]
    fn null_device_detection() {
        assert!(is_null_device("/dev/null"));
        if cfg!(windows) {
            assert!(is_null_device("NUL"));
            assert!(is_null_device("nul"));
        }
    }

    #[test]
    fn escaped_redirect_is_literal() {
        let parsed = crate::parser::tokenize(r"echo \> out.txt").unwrap();
        let (args, redirs) =
            extract_redirections_from_words(&parsed, 0).expect("parse");
        let args = crate::expander::expand_words(&args, 0);
        assert!(redirs.is_empty());
        assert_eq!(args, vec!["echo".to_string(), ">".to_string(), "out.txt".to_string()]);
    }
}
