use crate::parser::{Word, WordSegment};

/// Expand a list of parsed words into final argument strings.
/// Applies tilde, variable, and glob expansion according to quote context.
pub fn expand_words(words: &[Word], last_exit_code: i32) -> Vec<String> {
    let mut result = Vec::new();
    for word in words {
        result.extend(expand_word(word, last_exit_code));
    }
    result
}

/// Expand a single word (which may have mixed quoting) into one or more strings.
fn expand_word(segments: &[WordSegment], last_exit_code: i32) -> Vec<String> {
    let mut combined = String::new();
    let mut is_globbable = false;

    for segment in segments {
        match segment {
            WordSegment::SingleQuoted(text) => {
                // No expansion — everything literal
                combined.push_str(text);
            }
            WordSegment::DoubleQuoted(text) => {
                // Variable expansion only — no tilde, no glob
                let expanded = expand_variables(text, last_exit_code);
                combined.push_str(&expanded);
            }
            WordSegment::Unquoted(text) => {
                // Full expansion pipeline: tilde → variable → (mark for glob)
                let expanded = expand_tilde(text);
                let expanded = expand_variables(&expanded, last_exit_code);
                if contains_glob_chars(&expanded) {
                    is_globbable = true;
                }
                combined.push_str(&expanded);
            }
        }
    }

    if is_globbable {
        expand_globs(&combined)
    } else {
        vec![combined]
    }
}

// ── Tilde Expansion ──

fn expand_tilde(token: &str) -> String {
    if !token.starts_with('~') {
        return token.to_string();
    }

    let home = get_home_dir();

    if token == "~" {
        return home;
    }

    if token.starts_with("~/") || token.starts_with("~\\") {
        return format!("{home}{}", &token[1..]);
    }

    // ~username not supported yet — return as-is
    token.to_string()
}

fn get_home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "~".to_string())
}

// ── Variable Expansion ──

fn expand_variables(input: &str, last_exit_code: i32) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }

        // Peek at what follows the $
        match chars.peek() {
            None => {
                // Trailing $ — literal
                result.push('$');
            }
            Some(&'?') => {
                chars.next();
                result.push_str(&last_exit_code.to_string());
            }
            Some(&'$') => {
                chars.next();
                result.push_str(&std::process::id().to_string());
            }
            Some(&'0') => {
                chars.next();
                result.push_str("jsh");
            }
            Some(&'{') => {
                chars.next(); // consume '{'
                let name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                if name.is_empty() {
                    result.push_str("${}");
                } else {
                    let value = std::env::var(&name).unwrap_or_default();
                    result.push_str(&value);
                }
            }
            Some(&c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                name.push(chars.next().unwrap());
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                let value = std::env::var(&name).unwrap_or_default();
                result.push_str(&value);
            }
            Some(_) => {
                // $ followed by something that's not a valid var start — literal $
                result.push('$');
            }
        }
    }

    result
}

// ── Glob Expansion ──

fn contains_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

fn expand_globs(pattern: &str) -> Vec<String> {
    if !contains_glob_chars(pattern) {
        return vec![pattern.to_string()];
    }

    match glob::glob(pattern) {
        Ok(paths) => {
            let mut matches: Vec<String> = paths
                .filter_map(|entry| entry.ok())
                .map(|path| path.to_string_lossy().into_owned())
                .collect();

            if matches.is_empty() {
                // No matches — bash keeps the pattern literal
                vec![pattern.to_string()]
            } else {
                matches.sort();
                matches
            }
        }
        Err(_) => vec![pattern.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_alone() {
        let expanded = expand_tilde("~");
        assert!(!expanded.is_empty());
        assert_ne!(expanded, "~");
    }

    #[test]
    fn tilde_with_path() {
        let expanded = expand_tilde("~/projects");
        assert!(expanded.ends_with("/projects") || expanded.ends_with("\\projects"));
        assert!(!expanded.starts_with('~'));
    }

    #[test]
    fn tilde_in_middle_not_expanded() {
        assert_eq!(expand_tilde("foo~bar"), "foo~bar");
    }

    #[test]
    fn variable_simple() {
        // Set a test variable
        unsafe { std::env::set_var("JSH_TEST_VAR", "hello") };
        let result = expand_variables("$JSH_TEST_VAR", 0);
        assert_eq!(result, "hello");
        unsafe { std::env::remove_var("JSH_TEST_VAR") };
    }

    #[test]
    fn variable_braced() {
        unsafe { std::env::set_var("JSH_TEST_VAR2", "world") };
        let result = expand_variables("${JSH_TEST_VAR2}!", 0);
        assert_eq!(result, "world!");
        unsafe { std::env::remove_var("JSH_TEST_VAR2") };
    }

    #[test]
    fn variable_exit_code() {
        assert_eq!(expand_variables("$?", 42), "42");
        assert_eq!(expand_variables("$?", 0), "0");
    }

    #[test]
    fn variable_pid() {
        let result = expand_variables("$$", 0);
        let pid: u32 = result.parse().expect("$$ should be a number");
        assert!(pid > 0);
    }

    #[test]
    fn variable_shell_name() {
        assert_eq!(expand_variables("$0", 0), "jsh");
    }

    #[test]
    fn variable_undefined_is_empty() {
        let result = expand_variables("$DEFINITELY_NOT_SET_XYZ123", 0);
        assert_eq!(result, "");
    }

    #[test]
    fn trailing_dollar_literal() {
        assert_eq!(expand_variables("price$", 0), "price$");
    }

    #[test]
    fn dollar_followed_by_non_var_char() {
        assert_eq!(expand_variables("$+foo", 0), "$+foo");
    }

    #[test]
    fn single_quoted_no_expansion() {
        let word = vec![WordSegment::SingleQuoted("$HOME".into())];
        let result = expand_word(&word, 0);
        assert_eq!(result, vec!["$HOME"]);
    }

    #[test]
    fn double_quoted_expands_vars() {
        unsafe { std::env::set_var("JSH_DQ_TEST", "expanded") };
        let word = vec![WordSegment::DoubleQuoted("$JSH_DQ_TEST".into())];
        let result = expand_word(&word, 0);
        assert_eq!(result, vec!["expanded"]);
        unsafe { std::env::remove_var("JSH_DQ_TEST") };
    }

    #[test]
    fn double_quoted_no_glob() {
        let word = vec![WordSegment::DoubleQuoted("*.rs".into())];
        let result = expand_word(&word, 0);
        assert_eq!(result, vec!["*.rs"]);
    }

    #[test]
    fn no_glob_matches_keeps_literal() {
        let result = expand_globs("*.definitely_not_a_real_extension_xyz");
        assert_eq!(result, vec!["*.definitely_not_a_real_extension_xyz"]);
    }
}
