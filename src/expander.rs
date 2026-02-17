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
    let mut partials: Vec<(String, bool)> = vec![(String::new(), false)];

    for segment in segments {
        let replacements: Vec<(String, bool)> = match segment {
            WordSegment::SingleQuoted(text) => vec![(text.clone(), false)],
            WordSegment::DoubleQuoted(text) => {
                vec![(expand_variables(text, last_exit_code), false)]
            }
            WordSegment::Unquoted(text) => {
                let expanded = expand_variables(&expand_tilde(text), last_exit_code);
                let split_fields = if has_unquoted_expansion(text) {
                    let split: Vec<String> = expanded.split_whitespace().map(str::to_string).collect();
                    if split.is_empty() {
                        vec![String::new()]
                    } else {
                        split
                    }
                } else {
                    vec![expanded]
                };

                split_fields.into_iter().map(|field| (field, true)).collect()
            }
        };

        let mut next: Vec<(String, bool)> = Vec::with_capacity(
            partials.len().saturating_mul(replacements.len()),
        );

        for (base, base_glob) in partials {
            for (field, field_glob) in &replacements {
                next.push((format!("{base}{field}"), base_glob || *field_glob));
            }
        }

        partials = next;
    }

    partials
        .into_iter()
        .flat_map(|(text, can_glob)| {
            if can_glob && contains_glob_chars(&text) {
                expand_globs(&text).into_iter().collect::<Vec<_>>()
            } else {
                vec![text]
            }
        })
        .collect()
}

fn has_unquoted_expansion(text: &str) -> bool {
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            continue;
        }

        match chars.peek() {
            None => continue,
            Some('?') | Some('$') | Some('0') => return true,
            Some(&'{') => {
                chars.next();
                let mut has_end = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        has_end = true;
                        break;
                    }
                }
                if has_end {
                    return true;
                }
            }
            Some(c) if c.is_ascii_alphabetic() || *c == '_' => return true,
            _ => {}
        }
    }

    false
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
                let mut name = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }

                if !closed {
                    result.push_str("${");
                    result.push_str(&name);
                } else if name.is_empty() {
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
    fn variable_braced_missing_close_is_literal() {
        let result = expand_variables("${JSH_MISSING", 0);
        assert_eq!(result, "${JSH_MISSING");
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

    #[test]
    fn word_split_for_unquoted_variable() {
        unsafe { std::env::set_var("JSH_SPLIT_TEST", "alpha beta") };
        let word = vec![WordSegment::Unquoted("$JSH_SPLIT_TEST".into())];
        let result = expand_word(&word, 0);
        assert_eq!(result, vec!["alpha", "beta"]);
        unsafe { std::env::remove_var("JSH_SPLIT_TEST") };
    }

    #[test]
    fn no_word_split_in_quotes() {
        unsafe { std::env::set_var("JSH_SPLIT_TEST", "alpha beta") };
        let word = vec![WordSegment::DoubleQuoted("$JSH_SPLIT_TEST".into())];
        let result = expand_word(&word, 0);
        assert_eq!(result, vec!["alpha beta"]);
        unsafe { std::env::remove_var("JSH_SPLIT_TEST") };
    }
}
