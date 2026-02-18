/// A segment of a word, tagged with its quote context.
/// The expander uses this to decide what expansions to apply.
#[derive(Debug, Clone, PartialEq)]
pub enum WordSegment {
    /// Unquoted text — all expansions apply (tilde, variable, glob, word split)
    Unquoted(String),
    /// Double-quoted text — variable expansion only, no glob or word split
    DoubleQuoted(String),
    /// Single-quoted text — no expansion at all, everything literal
    SingleQuoted(String),
}

/// A single word (argument) made up of one or more segments.
/// Mixed quoting like `he"llo"'world'` produces multiple segments in one word.
pub type Word = Vec<WordSegment>;

/// A parsed command with a program name and its arguments.
#[derive(Debug)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
}

/// States for the tokenizer state machine.
enum State {
    /// Between tokens — whitespace is skipped
    Normal,
    /// Building an unquoted word — whitespace ends it
    InWord,
    /// Inside double quotes — whitespace is preserved
    InDoubleQuote,
    /// Inside single quotes — everything is literal
    InSingleQuote,
}

/// Consume a redirect operator starting with `>` or `<`.
/// Handles multi-character operators: >>, <<<, >&N
fn consume_redirect_op(first: char, chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut op = String::new();
    op.push(first);

    match first {
        '>' => {
            if chars.peek() == Some(&'>') {
                op.push(chars.next().unwrap()); // >>
            } else if chars.peek() == Some(&'&') {
                op.push(chars.next().unwrap()); // >&
                // Consume the fd number (e.g., >&1, >&2)
                if let Some(&c) = chars.peek()
                    && c.is_ascii_digit()
                {
                    op.push(chars.next().unwrap());
                }
            }
        }
        '<' => {
            if chars.peek() == Some(&'<') {
                op.push(chars.next().unwrap()); // <<
                if chars.peek() == Some(&'<') {
                    op.push(chars.next().unwrap()); // <<<
                }
            }
        }
        _ => {}
    }
    op
}

/// Tokenize input into a list of words, each preserving quote context.
pub fn tokenize(input: &str) -> Result<Vec<Word>, String> {
    let mut words: Vec<Word> = Vec::new();
    let mut current_segment = String::new();
    let mut current_word: Word = Vec::new();
    let mut state = State::Normal;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match (&state, ch) {
            // ── Normal state: between tokens ──
            (State::Normal, ' ' | '\t') => {}
            (State::Normal, '"') => {
                state = State::InDoubleQuote;
            }
            (State::Normal, '\'') => {
                state = State::InSingleQuote;
            }
            (State::Normal, '\\') => {
                // Escaped char is literal — emit as SingleQuoted so expander
                // won't touch it (e.g., \$VAR stays as $VAR, not expanded)
                if let Some(next) = chars.next() {
                    current_word.push(WordSegment::SingleQuoted(next.to_string()));
                } else {
                    current_word.push(WordSegment::SingleQuoted("\\".to_string()));
                }
                state = State::InWord;
            }
            (State::Normal, '|') => {
                // Pipe operator — treat as a segment separator.
                words.push(vec![WordSegment::Unquoted("|".to_string())]);
            }
            (State::Normal, '&') => {
                // Background operator — emit as a standalone token.
                words.push(vec![WordSegment::Unquoted("&".to_string())]);
            }
            (State::Normal, '>' | '<') => {
                // Redirect operator — emit as its own token
                let op = consume_redirect_op(ch, &mut chars);
                words.push(vec![WordSegment::Unquoted(op)]);
            }
            (State::Normal, c) => {
                current_segment.push(c);
                state = State::InWord;
            }

            // ── InWord state: building an unquoted token ──
            (State::InWord, ' ' | '\t') => {
                // Finish current unquoted segment and word
                if !current_segment.is_empty() {
                    current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
                }
                if !current_word.is_empty() {
                    words.push(std::mem::take(&mut current_word));
                }
                state = State::Normal;
            }
            (State::InWord, '"') => {
                // Flush unquoted segment, switch to double quotes
                if !current_segment.is_empty() {
                    current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
                }
                state = State::InDoubleQuote;
            }
            (State::InWord, '\'') => {
                if !current_segment.is_empty() {
                    current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
                }
                state = State::InSingleQuote;
            }
            (State::InWord, '\\') => {
                // Flush current unquoted segment, emit escaped char as literal
                if !current_segment.is_empty() {
                    current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
                }
                if let Some(next) = chars.next() {
                    current_word.push(WordSegment::SingleQuoted(next.to_string()));
                } else {
                    current_word.push(WordSegment::SingleQuoted("\\".to_string()));
                }
            }
            (State::InWord, '|') => {
                // Pipe inside a word breaks tokenization.
                if !current_segment.is_empty() {
                    current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
                }
                if !current_word.is_empty() {
                    words.push(std::mem::take(&mut current_word));
                }
                words.push(vec![WordSegment::Unquoted("|".to_string())]);
                state = State::Normal;
            }
            (State::InWord, '&') => {
                // Background operator breaks a word just like pipe does.
                if !current_segment.is_empty() {
                    current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
                }
                if !current_word.is_empty() {
                    words.push(std::mem::take(&mut current_word));
                }
                words.push(vec![WordSegment::Unquoted("&".to_string())]);
                state = State::Normal;
            }
            (State::InWord, '>' | '<') => {
                // Check if the current segment is a lone fd digit (e.g. "2" in "2>&1").
                // If so, merge it into the operator token instead of emitting as a word.
                let fd_prefix = if ch == '>'
                    && current_word.is_empty()
                    && current_segment.len() == 1
                    && current_segment.chars().next().unwrap().is_ascii_digit()
                {
                    let prefix = std::mem::take(&mut current_segment);
                    Some(prefix)
                } else {
                    // Flush the current segment/word normally
                    if !current_segment.is_empty() {
                        current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
                    }
                    if !current_word.is_empty() {
                        words.push(std::mem::take(&mut current_word));
                    }
                    None
                };

                let mut op = consume_redirect_op(ch, &mut chars);
                if let Some(prefix) = fd_prefix {
                    op = format!("{prefix}{op}");
                }
                words.push(vec![WordSegment::Unquoted(op)]);
                state = State::Normal;
            }
            (State::InWord, c) => {
                current_segment.push(c);
            }

            // ── InDoubleQuote state: inside "..." ──
            (State::InDoubleQuote, '"') => {
                // Flush double-quoted segment (even if empty — "" is a valid empty arg)
                current_word.push(WordSegment::DoubleQuoted(std::mem::take(&mut current_segment)));
                state = State::InWord;
            }
            (State::InDoubleQuote, '\\') => {
                match chars.peek() {
                    Some(&'"' | &'\\' | &'$' | &'`') => {
                        current_segment.push(chars.next().unwrap());
                    }
                    _ => {
                        current_segment.push('\\');
                    }
                }
            }
            (State::InDoubleQuote, c) => {
                current_segment.push(c);
            }

            // ── InSingleQuote state: inside '...' ──
            (State::InSingleQuote, '\'') => {
                // Flush single-quoted segment (even if empty)
                current_word.push(WordSegment::SingleQuoted(std::mem::take(&mut current_segment)));
                state = State::InWord;
            }
            (State::InSingleQuote, c) => {
                current_segment.push(c);
            }
        }
    }

    // Flush remaining segment and word
    match state {
        State::InWord => {
            if !current_segment.is_empty() {
                current_word.push(WordSegment::Unquoted(std::mem::take(&mut current_segment)));
            }
            // Push word even if segments produced empty text (e.g. trailing "")
            if !current_word.is_empty() {
                words.push(current_word);
            }
        }
        State::InDoubleQuote => {
            return Err("jsh: syntax error: unterminated double quote".to_string());
        }
        State::InSingleQuote => {
            return Err("jsh: syntax error: unterminated single quote".to_string());
        }
        State::Normal => {}
    }

    Ok(words)
}

/// Flatten words into plain strings, discarding quote context.
#[cfg(test)]
pub fn words_to_strings(words: &[Word]) -> Vec<String> {
    words
        .iter()
        .map(|word| {
            word.iter()
                .map(|seg| match seg {
                    WordSegment::Unquoted(s)
                    | WordSegment::DoubleQuoted(s)
                    | WordSegment::SingleQuoted(s) => s.as_str(),
                })
                .collect()
        })
        .collect()
}

/// Parse a shell input line into a Command (flat strings, no expansion).
#[cfg(test)]
pub fn parse(input: &str) -> Option<Command> {
    let words = tokenize(input).ok()?;
    let strings = words_to_strings(&words);

    if strings.is_empty() {
        return None;
    }

    Some(Command {
        program: strings[0].clone(),
        args: strings[1..].to_vec(),
    })
}

/// Parse input into raw words with quote context preserved.
/// Used by the expander pipeline.
pub fn parse_words(input: &str) -> Result<Vec<Word>, String> {
    tokenize(input)
}

/// Split tokenized words into pipeline segments.
///
/// Pipe separators are returned as standalone unquoted `|` words.
/// Returns a vector of commands (`Vec<Word>`), one per pipeline segment.
pub fn split_pipeline(words: &[Word]) -> Result<Vec<Vec<Word>>, String> {
    let mut commands = Vec::new();
    let mut current: Vec<Word> = Vec::new();

    for word in words {
        if is_pipe_word(word) {
            if current.is_empty() {
                return Err("jsh: syntax error: missing command before '|'".to_string());
            }
            commands.push(std::mem::take(&mut current));
            continue;
        }

        current.push(word.clone());
    }

    if current.is_empty() {
        return Err("jsh: syntax error: expected command after '|'".to_string());
    }

    commands.push(current);
    Ok(commands)
}

fn is_pipe_word(word: &Word) -> bool {
    word.len() == 1
        && matches!(&word[0], WordSegment::Unquoted(token) if token == "|")
}

/// Returns true if this word is a bare `&` background operator.
pub fn is_background_word(word: &Word) -> bool {
    word.len() == 1
        && matches!(&word[0], WordSegment::Unquoted(token) if token == "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_command() {
        let cmd = parse("echo hello world").unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello", "world"]);
    }

    #[test]
    fn double_quotes_preserve_spaces() {
        let cmd = parse(r#"echo "hello   world""#).unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello   world"]);
    }

    #[test]
    fn single_quotes_preserve_spaces() {
        let cmd = parse("echo 'hello   world'").unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello   world"]);
    }

    #[test]
    fn backslash_escapes_space() {
        let cmd = parse(r"echo hello\ world").unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello world"]);
    }

    #[test]
    fn mixed_quoting() {
        let cmd = parse(r#"echo "hello   world" foo\ bar 'single quotes'"#).unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello   world", "foo bar", "single quotes"]);
    }

    #[test]
    fn empty_input_returns_none() {
        assert!(parse("").is_none());
        assert!(parse("   ").is_none());
    }

    #[test]
    fn single_command_no_args() {
        let cmd = parse("ls").unwrap();
        assert_eq!(cmd.program, "ls");
        assert!(cmd.args.is_empty());
    }

    #[test]
    fn quotes_mid_word() {
        let strings = words_to_strings(&tokenize(r#"he"llo wor"ld"#).unwrap());
        assert_eq!(strings, vec!["hello world"]);
    }

    #[test]
    fn backslash_in_double_quotes() {
        let strings = words_to_strings(&tokenize(r#""hello\\world""#).unwrap());
        assert_eq!(strings, vec![r"hello\world"]);

        let strings = words_to_strings(&tokenize(r#""hello\"world""#).unwrap());
        assert_eq!(strings, vec![r#"hello"world"#]);
    }

    #[test]
    fn single_quotes_no_escaping() {
        let strings = words_to_strings(&tokenize(r"'hello\nworld'").unwrap());
        assert_eq!(strings, vec![r"hello\nworld"]);
    }

    #[test]
    fn empty_double_quoted_arg() {
        let cmd = parse(r#"echo """#).unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec![""]);
    }

    #[test]
    fn empty_single_quoted_arg() {
        let cmd = parse("echo ''").unwrap();
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec![""]);
    }

    #[test]
    fn multiple_empty_quoted_args() {
        let cmd = parse(r#"cmd "" '' """#).unwrap();
        assert_eq!(cmd.program, "cmd");
        assert_eq!(cmd.args, vec!["", "", ""]);
    }

    #[test]
    fn trailing_backslash_in_word() {
        let strings = words_to_strings(&tokenize(r"foo\").unwrap());
        assert_eq!(strings, vec![r"foo\"]);
    }

    #[test]
    fn trailing_backslash_standalone() {
        let strings = words_to_strings(&tokenize(r"\").unwrap());
        assert_eq!(strings, vec![r"\"]);
    }

    // ── Quote context tests ──

    #[test]
    fn quote_context_preserved() {
        let words = tokenize(r#"echo "hello" '$HOME'"#).unwrap();
        assert_eq!(words.len(), 3);
        assert_eq!(words[0], vec![WordSegment::Unquoted("echo".into())]);
        assert_eq!(words[1], vec![WordSegment::DoubleQuoted("hello".into())]);
        assert_eq!(words[2], vec![WordSegment::SingleQuoted("$HOME".into())]);
    }

    #[test]
    fn mixed_quote_segments() {
        let words = tokenize(r#"he"llo"'world'"#).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0], vec![
            WordSegment::Unquoted("he".into()),
            WordSegment::DoubleQuoted("llo".into()),
            WordSegment::SingleQuoted("world".into()),
        ]);
    }

    // ── Escaped metacharacter tests ──

    #[test]
    fn escaped_dollar_is_literal() {
        // \$VAR should NOT expand — the $ is escaped
        let words = tokenize(r"\$HOME").unwrap();
        assert_eq!(words.len(), 1);
        // The $ should be in a SingleQuoted segment (literal)
        assert!(words[0].iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "$")));
    }

    #[test]
    fn escaped_tilde_is_literal() {
        let words = tokenize(r"\~").unwrap();
        assert_eq!(words.len(), 1);
        assert!(words[0].iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "~")));
    }

    #[test]
    fn escaped_glob_is_literal() {
        let words = tokenize(r"\*.rs").unwrap();
        assert_eq!(words.len(), 1);
        // The * should be SingleQuoted (literal), not Unquoted (expandable)
        assert!(words[0].iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "*")));
    }

    #[test]
    fn escaped_char_mid_word_is_literal() {
        // echo foo\$BAR should have $ as literal
        let words = tokenize(r"echo foo\$BAR").unwrap();
        assert_eq!(words.len(), 2);
        let second = &words[1];
        assert!(second.iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "$")));
    }

    // ── Unterminated quote tests ──

    #[test]
    fn unterminated_double_quote_is_error() {
        // Missing closing " — syntax error
        let words = tokenize(r#"echo "$HOME"#);
        assert!(words.is_err());
    }

    #[test]
    fn unterminated_single_quote_is_error() {
        // Missing closing ' — syntax error
        let words = tokenize("echo '$HOME");
        assert!(words.is_err());
    }

    // ── Redirect operator tokenization tests ──

    #[test]
    fn fd_prefix_merged_with_redirect() {
        // "2>" should be a single token, not "2" + ">"
        let strings = words_to_strings(&tokenize("ls 2>err.txt").unwrap());
        assert_eq!(strings, vec!["ls", "2>", "err.txt"]);
    }

    #[test]
    fn fd_prefix_merged_with_append() {
        let strings = words_to_strings(&tokenize("ls 2>>err.txt").unwrap());
        assert_eq!(strings, vec!["ls", "2>>", "err.txt"]);
    }

    #[test]
    fn fd_prefix_merged_with_dup() {
        // "2>&1" should be a single token
        let strings = words_to_strings(&tokenize("ls 2>&1").unwrap());
        assert_eq!(strings, vec!["ls", "2>&1"]);
    }

    #[test]
    fn fd_prefix_1_merged_with_dup() {
        let strings = words_to_strings(&tokenize("echo err 1>&2").unwrap());
        assert_eq!(strings, vec!["echo", "err", "1>&2"]);
    }

    #[test]
    fn plain_redirect_no_fd_prefix() {
        let strings = words_to_strings(&tokenize("echo hello > out.txt").unwrap());
        assert_eq!(strings, vec!["echo", "hello", ">", "out.txt"]);
    }

    #[test]
    fn multi_digit_not_merged() {
        // "12>" — only single-digit fd prefixes are merged
        let strings = words_to_strings(&tokenize("12>file").unwrap());
        assert_eq!(strings, vec!["12", ">", "file"]);
    }

    #[test]
    fn stdin_redirect_not_merged_with_digit() {
        // Only > merges with fd prefix, not <
        let strings = words_to_strings(&tokenize("sort < data.txt").unwrap());
        assert_eq!(strings, vec!["sort", "<", "data.txt"]);
    }

    #[test]
    fn split_simple_pipeline() {
        let words = tokenize("echo hello | tr h H").unwrap();
        let segments = split_pipeline(&words).unwrap();
        let strings = segments
            .iter()
            .map(|segment| words_to_strings(segment))
            .collect::<Vec<_>>();
        assert_eq!(strings, vec![vec!["echo", "hello"], vec!["tr", "h", "H"]]);
    }

    #[test]
    fn split_pipeline_errors_on_leading_pipe() {
        let words = tokenize("| echo hi").unwrap();
        assert!(split_pipeline(&words).is_err());
    }

    #[test]
    fn split_pipeline_errors_on_consecutive_pipes() {
        let words = tokenize("echo hi || tr").unwrap();
        assert!(split_pipeline(&words).is_err());
    }

    #[test]
    fn split_pipeline_errors_on_trailing_pipe() {
        let words = tokenize("echo hi |").unwrap();
        assert!(split_pipeline(&words).is_err());
    }
}
