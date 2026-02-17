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

/// Tokenize a shell input line into a list of words.
///
/// Handles:
/// - Unquoted words split by whitespace
/// - Double-quoted strings ("hello world" → one token)
/// - Single-quoted strings ('hello world' → one token)
/// - Backslash escapes (hello\ world → one token)
pub fn tokenize(input: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut state = State::Normal;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match (&state, ch) {
            // ── Normal state: between tokens ──
            (State::Normal, ' ' | '\t') => {
                // Skip whitespace between tokens
            }
            (State::Normal, '"') => {
                // Start a double-quoted string
                state = State::InDoubleQuote;
            }
            (State::Normal, '\'') => {
                // Start a single-quoted string
                state = State::InSingleQuote;
            }
            (State::Normal, '\\') => {
                // Escape: take the next character literally
                if let Some(next) = chars.next() {
                    current.push(next);
                }
                state = State::InWord;
            }
            (State::Normal, c) => {
                // Start a new word
                current.push(c);
                state = State::InWord;
            }

            // ── InWord state: building an unquoted token ──
            (State::InWord, ' ' | '\t') => {
                // Whitespace ends the current word
                tokens.push(std::mem::take(&mut current));
                state = State::Normal;
            }
            (State::InWord, '"') => {
                // Transition into double quotes mid-word (e.g., wo"rld")
                state = State::InDoubleQuote;
            }
            (State::InWord, '\'') => {
                // Transition into single quotes mid-word
                state = State::InSingleQuote;
            }
            (State::InWord, '\\') => {
                // Escape: take the next character literally
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (State::InWord, c) => {
                current.push(c);
            }

            // ── InDoubleQuote state: inside "..." ──
            (State::InDoubleQuote, '"') => {
                // Closing quote — return to InWord (there might be more after the quote)
                state = State::InWord;
            }
            (State::InDoubleQuote, '\\') => {
                // Inside double quotes, backslash only escapes: \ " $ ` newline
                match chars.peek() {
                    Some(&'"' | &'\\' | &'$' | &'`') => {
                        current.push(chars.next().unwrap());
                    }
                    _ => {
                        // Backslash is literal if not followed by a special char
                        current.push('\\');
                    }
                }
            }
            (State::InDoubleQuote, c) => {
                current.push(c);
            }

            // ── InSingleQuote state: inside '...' ──
            (State::InSingleQuote, '\'') => {
                // Closing quote
                state = State::InWord;
            }
            (State::InSingleQuote, c) => {
                // Everything is literal inside single quotes — no escaping at all
                current.push(c);
            }
        }
    }

    // Don't forget the last token if we were mid-word
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Parse a shell input line into a Command.
/// Returns None if the input is empty after tokenization.
pub fn parse(input: &str) -> Option<Command> {
    let tokens = tokenize(input);

    if tokens.is_empty() {
        return None;
    }

    Some(Command {
        program: tokens[0].clone(),
        args: tokens[1..].to_vec(),
    })
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
        // e.g., he"llo wor"ld → hello world  (one token)
        let tokens = tokenize(r#"he"llo wor"ld"#);
        assert_eq!(tokens, vec!["hello world"]);
    }

    #[test]
    fn backslash_in_double_quotes() {
        // Inside double quotes, \\ → \ and \" → "
        let tokens = tokenize(r#""hello\\world""#);
        assert_eq!(tokens, vec![r"hello\world"]);

        let tokens = tokenize(r#""hello\"world""#);
        assert_eq!(tokens, vec![r#"hello"world"#]);
    }

    #[test]
    fn single_quotes_no_escaping() {
        // Single quotes: backslash is literal
        let tokens = tokenize(r"'hello\nworld'");
        assert_eq!(tokens, vec![r"hello\nworld"]);
    }
}
