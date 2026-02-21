use crate::ast::{ChainEntry, Connector};
use crate::parser::{Word, WordSegment};

/// If `word` is a chain operator token (`&&`, `||`, or `;`), return its
/// [`Connector`] variant. Returns `None` for all other tokens.
fn chain_op(word: &Word) -> Option<Connector> {
    if word.len() != 1 {
        return None;
    }
    match &word[0] {
        WordSegment::Unquoted(s) => match s.as_str() {
            "&&" => Some(Connector::And),
            "||" => Some(Connector::Or),
            ";" => Some(Connector::Sequence),
            _ => None,
        },
        _ => None,
    }
}

fn connector_display(c: &Connector) -> &'static str {
    match c {
        Connector::Sequence => ";",
        Connector::And => "&&",
        Connector::Or => "||",
    }
}

/// Split a flat `Vec<Word>` by chain operators (`&&`, `||`, `;`) into a list
/// of [`ChainEntry`] values, each annotated with the connector that gates it.
///
/// The first entry always gets [`Connector::Sequence`] (run unconditionally).
/// Subsequent entries get the connector that appeared before them in the input.
///
/// Returns an error for syntax problems such as leading, trailing, or
/// consecutive chain operators with no command between them.
pub fn parse_chain(words: Vec<Word>) -> Result<Vec<ChainEntry>, String> {
    let mut entries: Vec<ChainEntry> = Vec::new();
    let mut current: Vec<Word> = Vec::new();
    // Connector that will apply to the *next* entry we collect.
    // The first entry always runs unconditionally.
    let mut next_connector = Connector::Sequence;

    for word in words {
        if let Some(connector) = chain_op(&word) {
            if current.is_empty() {
                let op = connector_display(&connector);
                return Err(format!(
                    "jsh: syntax error near unexpected token `{op}'"
                ));
            }
            entries.push(ChainEntry {
                words: std::mem::take(&mut current),
                connector: next_connector,
            });
            next_connector = connector;
        } else {
            current.push(word);
        }
    }

    if current.is_empty() {
        if entries.is_empty() {
            // Completely empty input — callers guard against this, but handle gracefully.
            return Ok(vec![]);
        }
        // Trailing operator, e.g. `echo hi &&`
        let op = connector_display(&next_connector);
        return Err(format!(
            "jsh: syntax error: expected command after `{op}'"
        ));
    }

    entries.push(ChainEntry {
        words: current,
        connector: next_connector,
    });

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::tokenize;

    fn tokenize_chain(input: &str) -> Vec<ChainEntry> {
        let words = tokenize(input).expect("tokenize failed");
        parse_chain(words).expect("parse_chain failed")
    }

    fn entry_strings(entry: &ChainEntry) -> Vec<String> {
        entry
            .words
            .iter()
            .map(|word| {
                word.iter()
                    .map(|seg| match seg {
                        WordSegment::Unquoted(s)
                        | WordSegment::DoubleQuoted(s)
                        | WordSegment::SingleQuoted(s) => s.as_str(),
                    })
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn single_command_no_chain() {
        let entries = tokenize_chain("echo hello");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].connector, Connector::Sequence);
        assert_eq!(entry_strings(&entries[0]), vec!["echo", "hello"]);
    }

    #[test]
    fn and_chain() {
        let entries = tokenize_chain("echo hi && echo bye");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].connector, Connector::Sequence);
        assert_eq!(entry_strings(&entries[0]), vec!["echo", "hi"]);
        assert_eq!(entries[1].connector, Connector::And);
        assert_eq!(entry_strings(&entries[1]), vec!["echo", "bye"]);
    }

    #[test]
    fn or_chain() {
        let entries = tokenize_chain("false || echo fallback");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].connector, Connector::Sequence);
        assert_eq!(entries[1].connector, Connector::Or);
        assert_eq!(entry_strings(&entries[1]), vec!["echo", "fallback"]);
    }

    #[test]
    fn semicolon_chain() {
        let entries = tokenize_chain("echo a ; echo b");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].connector, Connector::Sequence);
    }

    #[test]
    fn multi_step_chain() {
        // false && echo skipped || echo ran
        let entries = tokenize_chain("false && echo skipped || echo ran");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].connector, Connector::Sequence);
        assert_eq!(entries[1].connector, Connector::And);
        assert_eq!(entries[2].connector, Connector::Or);
    }

    #[test]
    fn pipe_inside_chain_segment() {
        // The | inside a chain entry should pass through un-consumed.
        let entries = tokenize_chain("ls | wc && echo done");
        assert_eq!(entries.len(), 2);
        // First entry should still have "ls", "|", "wc" as words.
        let first_words = entry_strings(&entries[0]);
        assert_eq!(first_words, vec!["ls", "|", "wc"]);
    }

    #[test]
    fn leading_operator_is_error() {
        let words = tokenize("&& echo hi").unwrap();
        assert!(parse_chain(words).is_err());
    }

    #[test]
    fn trailing_operator_is_error() {
        let words = tokenize("echo hi &&").unwrap();
        assert!(parse_chain(words).is_err());
    }

    #[test]
    fn empty_input_returns_empty() {
        let entries = parse_chain(vec![]).unwrap();
        assert!(entries.is_empty());
    }
}
