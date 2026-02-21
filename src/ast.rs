use crate::parser::Word;

/// Controls whether a chained command runs based on the previous exit code.
#[derive(Debug, Clone, PartialEq)]
pub enum Connector {
    /// `;` — run unconditionally regardless of the previous exit code.
    Sequence,
    /// `&&` — run only if the previous command succeeded (exit code 0).
    And,
    /// `||` — run only if the previous command failed (exit code != 0).
    Or,
}

/// One pipeline's worth of words, annotated with the connector that
/// determines whether it should run given the previous exit code.
///
/// The first entry in a chain always uses [`Connector::Sequence`].
#[derive(Debug, Clone)]
pub struct ChainEntry {
    /// Raw words for this pipeline segment (pipe `|` tokens still embedded).
    pub words: Vec<Word>,
    /// How to decide whether to run this entry based on the last exit code.
    pub connector: Connector,
}
