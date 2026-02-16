# james-shell Architecture Overview

This document provides a big-picture view of how all the pieces of james-shell
fit together: data flow, module dependencies, file layout, core data structures,
and the design principles guiding every decision.

---

## 1. High-Level Data Flow

```
                           james-shell data flow
 ============================================================================

   User Input (keyboard)
        |
        v
  +-----------+     history recall,       +------------+
  |   Line    | <-- tab completion,   --> |  History   |
  |  Editor   |     syntax highlight      |  Engine    |
  +-----------+                           +------------+
        |
        | raw input string
        v
  +-----------+
  | Tokenizer |  (lexer.rs)
  |  (Lexer)  |
  +-----------+
        |
        | Vec<Token>
        v
  +-----------+
  |  Parser   |  (parser.rs)
  +-----------+
        |
        | AstNode (abstract syntax tree)
        v
  +-----------+
  | Expander  |  (expander.rs)
  +-----------+  variable substitution, tilde expansion,
        |        glob expansion, brace expansion
        | AstNode (expanded)
        v
  +-----------+
  | Executor  |  (executor.rs)
  +-----------+
        |
        +-------+--------+-----------+-------------+
        |       |        |           |             |
        v       v        v           v             v
   Builtins  External  Pipeline   Redirect    Scripting
   (cd, echo  Command   (|, |>)   (<, >, >>)  (if, for,
    set, ...)  (spawn)                         functions)
        |       |        |           |             |
        v       v        v           v             v
  +-----------+   +-----------+   +-----------+
  |   Value   |   | Raw text  |   | ShellError|
  | (struct)  |   | (stdout)  |   | (on fail) |
  +-----------+   +-----------+   +-----------+
        |               |               |
        +-------+-------+               |
                |                        |
                v                        v
  +-------------------+    +-------------------+
  | Output Formatter  |    |  Error Formatter  |
  | (table, json,     |    |  (colored, with   |
  |  plain text)      |    |   source spans)   |
  +-------------------+    +-------------------+
                |                        |
                v                        v
          +---------+             +---------+
          | stdout  |             | stderr  |
          +---------+             +---------+
```

### Pipeline detail

```
  cmd1 | cmd2 | cmd3
   |      |      |
   v      v      v
  exec   exec   exec
   |      |      |
   +--pipe-+--pipe-+
   stdout->stdin   stdout->stdin
                      |
                      v
                   final output
```

### Typed pipeline detail (Module 15+)

```
  internal_cmd |> filter |> transform |> format

  Value::Table ──> Value::Table ──> Value::Table ──> String (display)

  Structured data flows between internal commands without
  serialization. External commands receive/produce raw text;
  boundaries are converted automatically.
```

---

## 2. Module Dependency Map

Each module builds incrementally on earlier ones. The numbers correspond to the
20 curriculum modules.

```
  Module 1: REPL Loop
      |
      v
  Module 2: Parsing (Tokenizer + Parser + AST)
      |
      v
  Module 3: Execution ──────────────────────────+
      |                                          |
      +---> Module 4: Builtins                   |
      |         |                                |
      |         v                                |
      +---> Module 5: Expansion                  |
      |         |                                |
      |         v                                |
      +---> Module 6: Redirection                |
      |         |                                |
      |         v                                |
      +---> Module 7: Pipes & Pipelines          |
      |         |                                |
      |         v                                |
      +---> Module 8: Job Control                |
      |         |                                |
      |         v                                |
      +---> Module 9: Signal Handling            |
                |                                |
                v                                |
  Module 10: Line Editing (editor, history,      |
             completer, highlighter)             |
                |                                |
                v                                |
  Module 11: Scripting (if/else, for, while,     |
             functions, sourcing)                |
                |                                |
                v                                |
  Module 12: Advanced Shell Features             |
             (aliases, prompts, startup files)   |
                |                                |
                v                                |
  Module 13: Testing & Quality                   |
             (unit, integration, fuzzing)        |
                |                                |
                v                                |
  Module 14: Structured Types ───────────────────+
                |
                v
  Module 15: Typed Pipelines
                |
                v
  Module 16: Data Format Parsers (JSON, CSV, TOML, ...)
                |
                v
  Module 17: Completions Engine (context-aware)
                |
                v
  Module 18: Error Handling (rich diagnostics, spans)
                |
                v
  Module 19: Modern Scripting (closures, pattern matching)
                |
                v
  Module 20: Plugin System (dynamic loading, protocol)
```

### Cross-cutting concerns (used by many modules)

```
  error.rs        ── used by every module
  types/value.rs  ── used by modules 14-20
  shell.rs        ── owns global state, referenced by most modules
```

---

## 3. File Structure

Complete planned `src/` layout after all 20 modules are implemented.

```
src/
├── main.rs               Entry point; argument parsing, launches Shell
├── shell.rs              Shell struct: environment, REPL loop, global state
├── lexer.rs              Tokenizer: input string -> Vec<Token>
├── parser.rs             Recursive-descent parser: Vec<Token> -> AstNode
├── ast.rs                AST node type definitions (Command, Pipeline, If, ...)
├── expander.rs           Variable, tilde, glob, and brace expansion
├── executor.rs           Evaluates an AstNode: dispatches to builtins or spawns processes
├── pipeline.rs           Sets up pipes between processes, manages pipeline lifetime
├── redirect.rs           I/O redirection: <, >, >>, 2>&1, here-docs
├── jobs.rs               Job table, foreground/background management, fg/bg/jobs builtins
├── signals.rs            Signal handler registration (SIGINT, SIGTSTP, SIGCHLD, etc.)
├── editor.rs             Line editor integration (rustyline wrapper or custom)
├── history.rs            Command history: save, search, recall, persistent storage
├── completer.rs          Tab completion: paths, commands, arguments, context-aware
├── highlighter.rs        Live syntax highlighting for the input line
├── scripting.rs          Control flow (if/else/for/while), function defs, source command
├── environment.rs        Environment variable management, shell variable scoping
├── prompt.rs             Prompt rendering: PS1-style strings, git status, etc.
├── config.rs             Startup file loading (~/.jamesrc), configuration parsing
│
├── builtins/             Built-in commands (no child process needed)
│   ├── mod.rs            Builtin registry: name -> handler dispatch table
│   ├── cd.rs             Change directory (cross-platform path handling)
│   ├── echo.rs           Echo with flag support (-n, -e)
│   ├── exit.rs           Exit the shell with optional status code
│   ├── export.rs         Export variables to child process environment
│   ├── set.rs            Set/unset shell options and variables
│   ├── alias.rs          Alias definition and expansion
│   ├── history_cmd.rs    History builtin (list, search, clear)
│   ├── type_cmd.rs       Identify whether a name is builtin, alias, or external
│   ├── test.rs           [ ] / test expression evaluator
│   ├── which.rs          Locate a command on PATH
│   └── help.rs           Built-in help system
│
├── types/                Structured data types (Module 14+)
│   ├── mod.rs            Re-exports and Value enum definition
│   ├── value.rs          Value type: String, Int, Float, Bool, List, Table, Record, Null
│   ├── table.rs          Table type: column-oriented structured data with display
│   ├── record.rs         Record type: ordered key-value pairs
│   ├── convert.rs        Conversion traits between Value variants
│   └── display.rs        Pretty-printing for structured values (table formatter)
│
├── formats/              Data format parsers (Module 16)
│   ├── mod.rs            Format registry and auto-detection
│   ├── json.rs           JSON <-> Value conversion
│   ├── csv.rs            CSV <-> Value (Table) conversion
│   ├── toml.rs           TOML <-> Value conversion
│   ├── yaml.rs           YAML <-> Value conversion
│   └── lines.rs          Plain text line-by-line <-> Value::List conversion
│
├── plugins/              Plugin system (Module 20)
│   ├── mod.rs            Plugin trait and re-exports
│   ├── manager.rs        Plugin discovery, loading, lifecycle management
│   ├── protocol.rs       IPC protocol between shell and plugin processes
│   └── registry.rs       Registered plugin command lookup
│
└── error.rs              ShellError enum, source spans, diagnostic formatting
```

---

## 4. Key Data Structures

These are the core types that flow through the system. They form a pipeline of
transformations:

```
  Input String ──> Vec<Token> ──> AstNode ──> Command/Pipeline ──> Value/Output
```

### Token (from lexer)

```rust
/// A single lexical token produced by the tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(String),           // unquoted or quoted word
    Pipe,                   // |
    TypedPipe,              // |>  (structured data pipe)
    RedirectIn,             // <
    RedirectOut,            // >
    RedirectAppend,         // >>
    Background,             // &
    Semicolon,              // ;
    And,                    // &&
    Or,                     // ||
    LeftParen,              // (
    RightParen,             // )
    Newline,                // \n (significant in scripts)
    // ... more as needed
}

/// Token with source location for error reporting.
#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,         // byte offset range in source
}
```

### AstNode (from parser)

```rust
/// Abstract syntax tree nodes. The parser produces these from tokens.
#[derive(Debug, Clone)]
pub enum AstNode {
    /// A simple command: name + arguments
    Command {
        name: String,
        args: Vec<String>,
        redirects: Vec<Redirect>,
    },

    /// A pipeline: cmd1 | cmd2 | cmd3
    Pipeline {
        commands: Vec<AstNode>,
        typed: bool,          // true for |> (structured pipe)
    },

    /// A list of commands: cmd1 ; cmd2 ; cmd3
    CommandList {
        commands: Vec<(AstNode, ListOp)>,  // ListOp: Semi, And, Or
    },

    /// if condition { body } else { else_body }
    If {
        condition: Box<AstNode>,
        body: Box<AstNode>,
        else_body: Option<Box<AstNode>>,
    },

    /// for var in list { body }
    For {
        var: String,
        iterable: Box<AstNode>,
        body: Box<AstNode>,
    },

    /// while condition { body }
    While {
        condition: Box<AstNode>,
        body: Box<AstNode>,
    },

    /// Function definition: fn name(params) { body }
    FunctionDef {
        name: String,
        params: Vec<String>,
        body: Box<AstNode>,
    },

    /// Variable assignment: name = value
    Assignment {
        name: String,
        value: Box<AstNode>,
    },

    /// A block of statements
    Block {
        statements: Vec<AstNode>,
    },
}
```

### Redirect

```rust
/// I/O redirection specification.
#[derive(Debug, Clone)]
pub struct Redirect {
    pub fd: i32,              // file descriptor (0=stdin, 1=stdout, 2=stderr)
    pub kind: RedirectKind,
    pub target: String,       // filename or fd number for dup
}

#[derive(Debug, Clone)]
pub enum RedirectKind {
    Input,        // <
    Output,       // >
    Append,       // >>
    DupOutput,    // 2>&1
    HereDoc,      // <<
    HereString,   // <<<
}
```

### Value (structured data, Module 14+)

```rust
/// The core value type for structured data flowing through typed pipelines.
/// Internal commands produce and consume Values; external commands use text.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nothing,                              // null/void
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Record(IndexMap<String, Value>),      // ordered key-value pairs
    Table(Vec<IndexMap<String, Value>>),  // list of records (column-oriented display)
    Binary(Vec<u8>),                      // raw bytes
    Error(ShellError),                    // errors are values
}

impl Value {
    /// Convert to a human-readable string for display or piping to external commands.
    pub fn to_text(&self) -> String { /* ... */ }

    /// Try to coerce to a specific type.
    pub fn as_int(&self) -> Result<i64, ShellError> { /* ... */ }
    pub fn as_string(&self) -> Result<&str, ShellError> { /* ... */ }
    pub fn as_table(&self) -> Result<&[IndexMap<String, Value>], ShellError> { /* ... */ }
}
```

### ShellError

```rust
/// All errors in james-shell are represented as ShellError values.
/// They carry source spans for precise diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellError {
    /// Lexer error: unexpected character, unterminated string, etc.
    LexError { message: String, span: Span },

    /// Parser error: unexpected token, missing closing bracket, etc.
    ParseError { message: String, span: Span },

    /// Command not found on PATH or in builtins.
    CommandNotFound { name: String, span: Span },

    /// Type mismatch in a typed pipeline or expression.
    TypeError { expected: String, got: String, span: Span },

    /// I/O error (file not found, permission denied, broken pipe, etc.)
    IOError { message: String, source: Option<std::io::Error> },

    /// External command exited with non-zero status.
    ExternalError { command: String, exit_code: i32 },

    /// Variable not defined.
    UndefinedVariable { name: String, span: Span },

    /// Plugin communication failure.
    PluginError { plugin: String, message: String },

    /// Generic error with a message.
    General { message: String },
}

/// A byte-offset range in the source input, for error reporting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
```

### How They Connect

```
  "ls -la | grep foo > out.txt"
       |
       | Lexer
       v
  [Word("ls"), Word("-la"), Pipe, Word("grep"), Word("foo"),
   RedirectOut, Word("out.txt")]
       |
       | Parser
       v
  AstNode::Pipeline {
      commands: [
          AstNode::Command { name: "ls", args: ["-la"], redirects: [] },
          AstNode::Command { name: "grep", args: ["foo"],
                             redirects: [Redirect { fd: 1, kind: Output,
                                                    target: "out.txt" }] },
      ],
      typed: false,
  }
       |
       | Expander (no-op here, nothing to expand)
       v
  (same AstNode, but with variables/globs resolved)
       |
       | Executor
       v
  - Creates a pipe (os pipe)
  - Spawns "ls" with stdout -> pipe write end
  - Spawns "grep" with stdin -> pipe read end, stdout -> "out.txt"
  - Waits for both, collects exit status
       |
       v
  Value::Nothing (output went to file)
  or ShellError::ExternalError if a command failed
```

---

## 5. Design Principles

### Cross-platform first

All OS-specific operations are abstracted behind traits or conditional
compilation (`cfg(unix)` / `cfg(windows)`). The primary targets are Linux,
macOS, and Windows. Platform-specific modules:

- Process spawning: `std::process` covers most needs; raw syscalls (`fork`,
  `execvp`, `pipe`) are used only when necessary on Unix and wrapped in a
  platform abstraction.
- Signals: Unix has real signals (SIGINT, SIGTSTP); Windows uses
  `SetConsoleCtrlHandler`. The `signals.rs` module presents a unified interface.
- Terminal control: `crossterm` handles cross-platform terminal manipulation.
- Paths: Use `std::path::PathBuf` everywhere; never hardcode `/` or `\`.

### Incremental complexity

Each of the 20 modules adds exactly one capability. The shell is usable (if
minimal) after Module 1 and gets progressively more powerful. No module requires
understanding all 20 to work on it. Modules are designed to be implemented in
order because later modules build on earlier ones, but the code for each module
is isolated enough to be understood independently.

### Internal commands return structured data

Builtins and internal commands return `Value` (structured data). External
commands produce raw text on stdout. The boundary between structured and
unstructured is explicit:

- Internal -> Internal (typed pipe `|>`): pass `Value` directly, no serialization.
- Internal -> External (text pipe `|`): call `value.to_text()` to serialize.
- External -> Internal (text pipe `|`): read stdout as `Value::String`, then
  optionally parse (e.g., `from json`).

This is the same model nushell uses, and it enables powerful data manipulation
without sacrificing compatibility with the Unix tool ecosystem.

### Errors are values, not panics

The shell must never panic in normal operation. All errors are represented as
`ShellError` values that carry source location spans for precise diagnostics.
Errors propagate through `Result<Value, ShellError>` and can be caught by
scripting constructs (`try/catch`). When displayed, errors show:

```
Error: command not found
  --> input:1:5
  |
1 | foo | bar | baz
  |       ^^^ 'bar' not found in PATH or builtins
  |
  = help: did you mean 'tar'?
```

### Performance: streaming and zero-copy

- **Streaming**: Pipelines stream data between stages. We never buffer an
  entire pipeline's output in memory when we can avoid it. For typed pipelines,
  values are passed one record at a time where possible.
- **Zero-copy parsing**: The lexer produces tokens that reference the original
  input string (using spans) rather than always cloning substrings. Owned
  strings are created only when expansion modifies the text.
- **Lazy evaluation**: Glob expansion and command substitution produce iterators,
  not fully materialized vectors, when the consumer can handle it.
- **Efficient process spawning**: External commands are spawned directly via
  `execvp` on Unix (no intermediate shell), and via `CreateProcessW` on Windows.

### Other guiding principles

- **Familiarity**: POSIX-compatible syntax for common operations. A user who
  knows bash should feel at home for basic commands. New syntax (typed pipes,
  structured data) is additive, not replacing.
- **Discoverability**: Good error messages, helpful tab completion, and a `help`
  builtin make the shell easy to explore.
- **Testability**: Every module has unit tests. The parser and executor are
  designed to be testable without launching real processes (mock execution
  backends).
- **Minimal dependencies**: Use external crates where they save significant
  effort (serde, crossterm, rustyline), but keep the dependency tree reasonable.
  Avoid pulling in heavy frameworks.

---

## Appendix: Module-to-File Mapping

| Module | Primary Files                                    |
|--------|--------------------------------------------------|
| 1      | `main.rs`, `shell.rs`                            |
| 2      | `lexer.rs`, `parser.rs`, `ast.rs`                |
| 3      | `executor.rs`                                    |
| 4      | `builtins/mod.rs`, `builtins/cd.rs`, ...         |
| 5      | `expander.rs`                                    |
| 6      | `redirect.rs`                                    |
| 7      | `pipeline.rs`                                    |
| 8      | `jobs.rs`                                        |
| 9      | `signals.rs`                                     |
| 10     | `editor.rs`, `history.rs`, `completer.rs`, `highlighter.rs` |
| 11     | `scripting.rs`                                   |
| 12     | `config.rs`, `prompt.rs`, `environment.rs`       |
| 13     | (tests throughout, `tests/` directory)           |
| 14     | `types/mod.rs`, `types/value.rs`, `types/table.rs`, ... |
| 15     | `pipeline.rs` (extended), `executor.rs` (extended) |
| 16     | `formats/mod.rs`, `formats/json.rs`, ...         |
| 17     | `completer.rs` (extended)                        |
| 18     | `error.rs` (extended)                            |
| 19     | `scripting.rs` (extended)                        |
| 20     | `plugins/mod.rs`, `plugins/manager.rs`, ...      |
