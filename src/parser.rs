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

/// Tokenize input into a list of words, each preserving quote context.
pub fn tokenize(input: &str) -> Vec<Word> {
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
            // Unclosed double quote — preserve quote context
            if !current_segment.is_empty() {
                current_word.push(WordSegment::DoubleQuoted(current_segment));
            }
            if !current_word.is_empty() {
                words.push(current_word);
            }
        }
        State::InSingleQuote => {
            // Unclosed single quote — preserve quote context (no expansion)
            if !current_segment.is_empty() {
                current_word.push(WordSegment::SingleQuoted(current_segment));
            }
            if !current_word.is_empty() {
                words.push(current_word);
            }
        }
        State::Normal => {}
    }

    words
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
    let words = tokenize(input);
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
pub fn parse_words(input: &str) -> Vec<Word> {
    tokenize(input)
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
        let strings = words_to_strings(&tokenize(r#"he"llo wor"ld"#));
        assert_eq!(strings, vec!["hello world"]);
    }

    #[test]
    fn backslash_in_double_quotes() {
        let strings = words_to_strings(&tokenize(r#""hello\\world""#));
        assert_eq!(strings, vec![r"hello\world"]);

        let strings = words_to_strings(&tokenize(r#""hello\"world""#));
        assert_eq!(strings, vec![r#"hello"world"#]);
    }

    #[test]
    fn single_quotes_no_escaping() {
        let strings = words_to_strings(&tokenize(r"'hello\nworld'"));
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
        let strings = words_to_strings(&tokenize(r"foo\"));
        assert_eq!(strings, vec![r"foo\"]);
    }

    #[test]
    fn trailing_backslash_standalone() {
        let strings = words_to_strings(&tokenize(r"\"));
        assert_eq!(strings, vec![r"\"]);
    }

    // ── Quote context tests ──

    #[test]
    fn quote_context_preserved() {
        let words = tokenize(r#"echo "hello" '$HOME'"#);
        assert_eq!(words.len(), 3);
        assert_eq!(words[0], vec![WordSegment::Unquoted("echo".into())]);
        assert_eq!(words[1], vec![WordSegment::DoubleQuoted("hello".into())]);
        assert_eq!(words[2], vec![WordSegment::SingleQuoted("$HOME".into())]);
    }

    #[test]
    fn mixed_quote_segments() {
        let words = tokenize(r#"he"llo"'world'"#);
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
        let words = tokenize(r"\$HOME");
        assert_eq!(words.len(), 1);
        // The $ should be in a SingleQuoted segment (literal)
        assert!(words[0].iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "$")));
    }

    #[test]
    fn escaped_tilde_is_literal() {
        let words = tokenize(r"\~");
        assert_eq!(words.len(), 1);
        assert!(words[0].iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "~")));
    }

    #[test]
    fn escaped_glob_is_literal() {
        let words = tokenize(r"\*.rs");
        assert_eq!(words.len(), 1);
        // The * should be SingleQuoted (literal), not Unquoted (expandable)
        assert!(words[0].iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "*")));
    }

    #[test]
    fn escaped_char_mid_word_is_literal() {
        // echo foo\$BAR should have $ as literal
        let words = tokenize(r"echo foo\$BAR");
        assert_eq!(words.len(), 2);
        let second = &words[1];
        assert!(second.iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "$")));
    }

    // ── Unterminated quote tests ──

    #[test]
    fn unterminated_double_quote_keeps_context() {
        // Missing closing " — content stays DoubleQuoted
        let words = tokenize(r#"echo "$HOME"#);
        assert_eq!(words.len(), 2);
        assert!(words[1].iter().any(|seg| matches!(seg, WordSegment::DoubleQuoted(_))));
    }

    #[test]
    fn unterminated_single_quote_keeps_context() {
        // Missing closing ' — content stays SingleQuoted (no expansion)
        let words = tokenize("echo '$HOME");
        assert_eq!(words.len(), 2);
        assert!(words[1].iter().any(|seg| matches!(seg, WordSegment::SingleQuoted(s) if s == "$HOME")));
    }
}
