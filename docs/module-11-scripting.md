# Module 11: Control Flow & Scripting

## What are we building?

Up to this point, james-shell can execute individual commands, pipe them together,
redirect I/O, manage jobs, and expand variables. But a real shell is also a
**programming language**. In this module we transform james-shell from a command
launcher into a scriptable interpreter. By the end you will be able to write
`.jsh` script files with conditionals, loops, functions, and command substitution
-- and execute them just like any other program.

We will:

1. Add conditional operators (`&&`, `||`, `;`) to chain commands.
2. Implement `if`/`elif`/`else`/`end` blocks (a cleaner syntax than bash).
3. Implement `while` and `for` loops.
4. Support command substitution with `$(...)`.
5. Support subshells with `(...)`.
6. Add user-defined functions with `fn name(args) { body }`.
7. Handle local variables, return values, and exit codes.
8. Execute `.jsh` scripts and support the `source` builtin.
9. Design an AST (Abstract Syntax Tree) for the shell language.
10. Build a recursive descent parser.

---

## Concept 1: Conditional Execution Operators

### The Three Operators

Shells use three fundamental chaining operators to combine commands on a single
line. Each has different semantics around the **exit code** of the preceding
command.

| Operator | Name     | Behaviour                                        |
|----------|----------|--------------------------------------------------|
| `;`      | Sequence | Run the next command unconditionally.             |
| `&&`     | And      | Run the next command only if the previous **succeeded** (exit 0). |
| `\|\|`   | Or       | Run the next command only if the previous **failed** (exit != 0). |

### Tokenising the Operators

During lexing we need to distinguish these multi-character tokens from single
characters. A `&` by itself means "run in background", but `&&` means "logical
and". Similarly, `|` starts a pipe while `||` means "logical or".

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(String),
    Pipe,          // |
    And,           // &&
    Or,            // ||
    Semi,          // ;
    Background,    // &
    // ... other tokens
}

/// Peek at the next character to decide between single and double tokens.
fn lex_operator(&mut self, first: char) -> Token {
    match first {
        '&' => {
            if self.peek() == Some('&') {
                self.advance(); // consume second '&'
                Token::And
            } else {
                Token::Background
            }
        }
        '|' => {
            if self.peek() == Some('|') {
                self.advance(); // consume second '|'
                Token::Or
            } else {
                Token::Pipe
            }
        }
        ';' => Token::Semi,
        _ => unreachable!(),
    }
}
```

### Evaluating Chains

Once parsed, a chain of commands becomes a list of `(Command, Connector)` pairs.
We walk through them left to right, tracking the most recent exit code.

```rust
#[derive(Debug, Clone)]
pub enum Connector {
    /// Always execute the next command.
    Sequence,
    /// Execute only if the previous command succeeded.
    And,
    /// Execute only if the previous command failed.
    Or,
}

pub fn execute_chain(chain: &[(Command, Connector)]) -> i32 {
    let mut last_status: i32 = 0;

    for (i, (cmd, connector)) in chain.iter().enumerate() {
        let should_run = match connector {
            Connector::Sequence => true,
            Connector::And => last_status == 0,
            Connector::Or => last_status != 0,
        };

        if should_run {
            last_status = execute_command(cmd);
        }
        // If we skip, last_status is unchanged -- this matters for
        // longer chains like: false || echo hi && echo bye
    }

    last_status
}
```

### Precedence

`&&` and `||` have **equal precedence** and associate **left to right**. The `;`
operator has *lower* precedence -- it separates independent statements. Consider:

```
cmd1 && cmd2 || cmd3 ; cmd4
```

This is grouped as:

```
( (cmd1 && cmd2) || cmd3 ) ; cmd4
```

`cmd4` always runs. `cmd3` runs only if `cmd1 && cmd2` produces a non-zero exit.

---

## Concept 2: If / Elif / Else / End

### Syntax Design

Bash uses `if ... then ... elif ... then ... else ... fi`. The `fi` keyword is
`if` spelled backwards -- a convention inherited from Algol 68 that many
developers find surprising. For james-shell we use a cleaner block syntax:

```
if <condition> {
    <body>
} elif <condition> {
    <body>
} else {
    <body>
}
```

The `<condition>` is any command (or pipeline). The condition is **true** when
the command exits with status 0, and **false** otherwise. This is identical to
how bash `if` works -- the test is an exit code, not a boolean expression.

### AST Representation

```rust
#[derive(Debug, Clone)]
pub struct IfBlock {
    /// The primary condition and its body.
    pub condition: Vec<AstNode>,
    pub body: Vec<AstNode>,
    /// Zero or more elif branches.
    pub elif_branches: Vec<(Vec<AstNode>, Vec<AstNode>)>,
    /// Optional else body.
    pub else_body: Option<Vec<AstNode>>,
}
```

### Evaluation

```rust
pub fn eval_if(block: &IfBlock, env: &mut ShellEnv) -> i32 {
    // Evaluate the primary condition.
    let cond_status = eval_nodes(&block.condition, env);
    if cond_status == 0 {
        return eval_nodes(&block.body, env);
    }

    // Try each elif in order.
    for (elif_cond, elif_body) in &block.elif_branches {
        let elif_status = eval_nodes(elif_cond, env);
        if elif_status == 0 {
            return eval_nodes(elif_body, env);
        }
    }

    // Fall through to else.
    if let Some(else_body) = &block.else_body {
        return eval_nodes(else_body, env);
    }

    // No branch matched, return the last condition's exit code.
    1
}
```

### The `test` / `[` Builtin

To make conditions useful we need a way to test things like "does this file
exist?" or "is this string empty?". Shells provide the `test` builtin (also
accessible as `[`).

```rust
pub fn builtin_test(args: &[&str]) -> i32 {
    match args {
        // String tests
        ["-z", s] => if s.is_empty() { 0 } else { 1 },
        ["-n", s] => if !s.is_empty() { 0 } else { 1 },
        [a, "=", b] => if a == b { 0 } else { 1 },
        [a, "!=", b] => if a != b { 0 } else { 1 },

        // Integer comparisons
        [a, "-eq", b] => int_cmp(a, b, |x, y| x == y),
        [a, "-ne", b] => int_cmp(a, b, |x, y| x != y),
        [a, "-lt", b] => int_cmp(a, b, |x, y| x < y),
        [a, "-gt", b] => int_cmp(a, b, |x, y| x > y),
        [a, "-le", b] => int_cmp(a, b, |x, y| x <= y),
        [a, "-ge", b] => int_cmp(a, b, |x, y| x >= y),

        // File tests
        ["-e", path] => if Path::new(path).exists() { 0 } else { 1 },
        ["-f", path] => if Path::new(path).is_file() { 0 } else { 1 },
        ["-d", path] => if Path::new(path).is_dir() { 0 } else { 1 },

        // Negation
        ["!", rest @ ..] => {
            let inner = builtin_test(rest);
            if inner == 0 { 1 } else { 0 }
        }

        _ => {
            eprintln!("test: unrecognised expression");
            2
        }
    }
}

fn int_cmp(a: &str, b: &str, op: fn(i64, i64) -> bool) -> i32 {
    match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(x), Ok(y)) => if op(x, y) { 0 } else { 1 },
        _ => {
            eprintln!("test: integer expression expected");
            2
        }
    }
}
```

### Example Session

```
jsh> if test -f Cargo.toml {
...>     echo "This is a Rust project"
...> } elif test -f package.json {
...>     echo "This is a Node project"
...> } else {
...>     echo "Unknown project type"
...> }
This is a Rust project
```

---

## Concept 3: While and For Loops

### While Loop

The `while` loop repeatedly evaluates a condition and executes its body as long
as the condition succeeds (exit code 0).

```
while <condition> {
    <body>
}
```

AST node:

```rust
#[derive(Debug, Clone)]
pub struct WhileLoop {
    pub condition: Vec<AstNode>,
    pub body: Vec<AstNode>,
}
```

Evaluation:

```rust
pub fn eval_while(wl: &WhileLoop, env: &mut ShellEnv) -> i32 {
    let mut last_status = 0;
    loop {
        let cond = eval_nodes(&wl.condition, env);
        if cond != 0 {
            break;
        }
        last_status = eval_nodes(&wl.body, env);

        // Support `break` and `continue` via a control flow signal.
        match env.take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => continue,
            _ => {}
        }
    }
    last_status
}
```

### For Loop

Our `for` loop iterates over a list of words:

```
for var in word1 word2 word3 {
    <body>
}
```

AST node:

```rust
#[derive(Debug, Clone)]
pub struct ForLoop {
    pub variable: String,
    pub items: Vec<String>,  // after expansion
    pub body: Vec<AstNode>,
}
```

Evaluation:

```rust
pub fn eval_for(fl: &ForLoop, env: &mut ShellEnv) -> i32 {
    let mut last_status = 0;

    for item in &fl.items {
        env.set_var(&fl.variable, item);
        last_status = eval_nodes(&fl.body, env);

        match env.take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => continue,
            _ => {}
        }
    }

    last_status
}
```

### Break and Continue

`break` and `continue` are builtins that set a flag in the environment. The loop
evaluator checks this flag after each iteration.

```rust
#[derive(Debug, Clone)]
pub enum ControlFlow {
    Break,
    Continue,
    Return(i32),
}

impl ShellEnv {
    pub fn signal_break(&mut self) {
        self.control_flow = Some(ControlFlow::Break);
    }

    pub fn signal_continue(&mut self) {
        self.control_flow = Some(ControlFlow::Continue);
    }

    pub fn take_control_flow(&mut self) -> Option<ControlFlow> {
        self.control_flow.take()
    }
}
```

### Example Session

```
jsh> for f in *.rs {
...>     echo "Source file: $f"
...> }
Source file: main.rs
Source file: lexer.rs
Source file: parser.rs

jsh> let i = 0
jsh> while test $i -lt 5 {
...>     echo "count: $i"
...>     let i = $((i + 1))
...> }
count: 0
count: 1
count: 2
count: 3
count: 4
```

---

## Concept 4: Command Substitution

### What It Does

Command substitution captures the **standard output** of a command and inserts
it as text wherever the substitution appeared. The syntax is `$(command)`.

```
jsh> echo "Today is $(date +%A)"
Today is Monday
```

### Nesting

Because `$(...)` uses balanced delimiters, nesting is straightforward:

```
jsh> echo "Kernel: $(uname -r | cut -d. -f1-2)"
Kernel: 6.1
```

### Implementation

During the **expansion** phase (after parsing, before execution), we walk
through each word looking for `$(...)` sequences. When we find one, we:

1. Extract the inner command string.
2. Parse and execute it, capturing stdout into a `String`.
3. Replace the `$(...)` with the captured output (trimming trailing newlines).

```rust
use std::process::{Command, Stdio};
use std::io::Read;

/// Expand all `$(...)` sequences in `input`.
pub fn expand_command_substitution(input: &str, env: &mut ShellEnv) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'(') {
            chars.next(); // consume '('
            let inner = extract_balanced(&mut chars, '(', ')');
            let output = capture_output(&inner, env);
            // Trim trailing newlines, matching POSIX behaviour.
            result.push_str(output.trim_end_matches('\n'));
        } else {
            result.push(c);
        }
    }

    result
}

/// Read characters until we find the matching closing delimiter,
/// respecting nested pairs.
fn extract_balanced(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    open: char,
    close: char,
) -> String {
    let mut depth = 1;
    let mut buf = String::new();

    while let Some(c) = chars.next() {
        if c == open {
            depth += 1;
            buf.push(c);
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
            buf.push(c);
        } else {
            buf.push(c);
        }
    }

    buf
}

/// Execute `cmd_str` in a subshell and return its stdout as a String.
fn capture_output(cmd_str: &str, env: &mut ShellEnv) -> String {
    // We re-enter our own parser and evaluator here.
    let tokens = lex(cmd_str);
    let ast = parse(&tokens);

    // Create a pipe to capture output.
    let (read_fd, write_fd) = os_pipe::pipe().expect("pipe creation failed");

    // Evaluate with stdout redirected to write_fd.
    let saved_stdout = env.redirect_stdout(write_fd);
    let _status = eval_nodes(&ast, env);
    env.restore_stdout(saved_stdout);

    // Read everything from the pipe.
    let mut output = String::new();
    let mut reader = std::io::BufReader::new(read_fd);
    reader.read_to_string(&mut output).unwrap_or_default();

    output
}
```

### Cross-Platform Note

On Windows, `os_pipe` works correctly for anonymous pipes. We rely on it (or
the standard library's `Stdio::piped()`) to remain cross-platform.

---

## Concept 5: Subshells

### What Is a Subshell?

A subshell is a **child copy** of the current shell environment. Any variable
changes, directory changes, or other state modifications inside the subshell do
**not** affect the parent.

```
jsh> let x = hello
jsh> (let x = goodbye; echo $x)
goodbye
jsh> echo $x
hello
```

### Implementation Strategy

We clone the `ShellEnv` before entering the subshell, run the commands against
the clone, and then discard the clone.

```rust
pub fn eval_subshell(nodes: &[AstNode], env: &mut ShellEnv) -> i32 {
    // Clone the environment so changes don't leak out.
    let mut sub_env = env.clone();
    let status = eval_nodes(nodes, &mut sub_env);

    // The only thing that escapes is the exit code.
    status
}
```

On Unix, a real shell would `fork()` here. We avoid `fork()` for two reasons:

1. **Windows has no `fork()`**. We want cross-platform behaviour.
2. Cloning the environment is simpler and safer in Rust.

If the subshell's output needs to be captured (as in command substitution), we
combine the subshell with pipe redirection as shown in Concept 4.

---

## Concept 6: Functions

### Syntax

We deliberately depart from bash's `function name { ... }` syntax and use
something closer to Rust/JavaScript:

```
fn greet(name) {
    echo "Hello, $name!"
}

greet World
```

### AST Representation

```rust
#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<AstNode>,
}
```

Functions are stored in the shell environment:

```rust
use std::collections::HashMap;

pub struct ShellEnv {
    pub variables: HashMap<String, String>,
    pub functions: HashMap<String, FunctionDef>,
    pub control_flow: Option<ControlFlow>,
    // ...
}
```

### Calling a Function

When the evaluator encounters a simple command, it checks `env.functions`
**before** searching `PATH` for an external binary:

```rust
pub fn execute_simple_command(cmd: &SimpleCommand, env: &mut ShellEnv) -> i32 {
    let name = &cmd.argv[0];

    // 1. Check for builtins.
    if let Some(builtin) = lookup_builtin(name) {
        return builtin(&cmd.argv[1..], env);
    }

    // 2. Check for user-defined functions.
    if let Some(func) = env.functions.get(name).cloned() {
        return call_function(&func, &cmd.argv[1..], env);
    }

    // 3. Search PATH for external command.
    execute_external(cmd, env)
}

fn call_function(func: &FunctionDef, args: &[String], env: &mut ShellEnv) -> i32 {
    // Create a new scope for local variables.
    let saved_locals = env.push_scope();

    // Bind positional parameters.
    for (i, param) in func.params.iter().enumerate() {
        let value = args.get(i).map(|s| s.as_str()).unwrap_or("");
        env.set_local(param, value);
    }

    // Also set $1, $2, ... and $# for compatibility.
    for (i, arg) in args.iter().enumerate() {
        env.set_local(&format!("{}", i + 1), arg);
    }
    env.set_local("#", &args.len().to_string());

    // Evaluate the body.
    let status = eval_nodes(&func.body, env);

    // Check for explicit return.
    let final_status = match env.take_control_flow() {
        Some(ControlFlow::Return(code)) => code,
        _ => status,
    };

    env.pop_scope(saved_locals);
    final_status
}
```

### Local Variables

We implement scoping with a **scope stack**. Each scope is a `HashMap` that
shadows the outer scope's variables.

```rust
impl ShellEnv {
    /// Push a new local scope. Returns a token to restore later.
    pub fn push_scope(&mut self) -> usize {
        self.scope_stack.push(HashMap::new());
        self.scope_stack.len() - 1
    }

    /// Pop back to a previous scope depth.
    pub fn pop_scope(&mut self, depth: usize) {
        self.scope_stack.truncate(depth);
    }

    /// Set a variable in the current (topmost) scope.
    pub fn set_local(&mut self, name: &str, value: &str) {
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.insert(name.to_string(), value.to_string());
        }
    }

    /// Look up a variable, searching from the innermost scope outward.
    pub fn get_var(&self, name: &str) -> Option<&str> {
        for scope in self.scope_stack.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val);
            }
        }
        self.variables.get(name).map(|s| s.as_str())
    }
}
```

### Return Values vs Exit Codes

In shells, functions do not "return" data the way Rust functions do. They have
two output channels:

| Channel      | How to produce        | How to consume                    |
|--------------|-----------------------|-----------------------------------|
| **Exit code** | `return N`           | `$?`, or `&&` / `||` chains      |
| **Stdout**   | `echo`, `printf`, etc | Command substitution `$(func)`    |

```
fn add(a, b) {
    echo $(( a + b ))
}

result=$(add 3 4)
echo "3 + 4 = $result"    # prints: 3 + 4 = 7
```

The `return` builtin sets `ControlFlow::Return(n)` and the evaluator propagates
it upward.

```rust
pub fn builtin_return(args: &[&str], env: &mut ShellEnv) -> i32 {
    let code = args.first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    env.control_flow = Some(ControlFlow::Return(code));
    code
}
```

---

## Concept 7: Script Execution

### Shebang Lines

A james-shell script file uses the `.jsh` extension by convention. The first
line can be a shebang to make it directly executable on Unix:

```bash
#!/usr/bin/env jsh
# my-script.jsh

echo "Hello from james-shell!"
for f in *.txt {
    echo "Processing $f"
}
```

On Windows, we can associate `.jsh` files with our binary, or the user runs
them explicitly: `jsh script.jsh`.

### How Script Execution Works

When james-shell receives a filename as an argument (or a command that resolves
to a `.jsh` file), we:

1. Read the entire file into a `String`.
2. If the first line starts with `#!`, skip it.
3. Lex, parse, and evaluate the rest.

```rust
use std::fs;
use std::path::Path;

pub fn execute_script(path: &Path, args: &[String], env: &mut ShellEnv) -> i32 {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("jsh: {}: {}", path.display(), e);
            return 1;
        }
    };

    // Strip shebang if present.
    let code = if source.starts_with("#!") {
        match source.find('\n') {
            Some(pos) => &source[pos + 1..],
            None => return 0, // file is only a shebang
        }
    } else {
        &source
    };

    // Set positional parameters.
    let saved = env.push_scope();
    env.set_local("0", &path.display().to_string());
    for (i, arg) in args.iter().enumerate() {
        env.set_local(&format!("{}", i + 1), arg);
    }
    env.set_local("#", &args.len().to_string());

    let tokens = lex(code);
    let ast = parse(&tokens);
    let status = eval_nodes(&ast, env);

    env.pop_scope(saved);
    status
}
```

### The `source` Builtin

`source` (or `.`) reads a file and executes it in the **current** shell
environment. Unlike script execution, there is no new scope -- variables set
inside the sourced file persist.

```rust
pub fn builtin_source(args: &[&str], env: &mut ShellEnv) -> i32 {
    let path = match args.first() {
        Some(p) => Path::new(p),
        None => {
            eprintln!("source: filename argument required");
            return 1;
        }
    };

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("source: {}: {}", path.display(), e);
            return 1;
        }
    };

    let code = if source.starts_with("#!") {
        match source.find('\n') {
            Some(pos) => &source[pos + 1..],
            None => return 0,
        }
    } else {
        &source
    };

    // Execute in the CURRENT environment -- no new scope.
    let tokens = lex(code);
    let ast = parse(&tokens);
    eval_nodes(&ast, env)
}
```

---

## Concept 8: Designing an AST

### Why an AST?

Until now we may have been executing commands almost directly from tokens. With
control flow, nested structures, and functions, we need a proper intermediate
representation -- the **Abstract Syntax Tree**.

An AST is a tree where each node represents a syntactic construct. The tree
structure naturally represents nesting (an `if` body contains statements, which
may themselves contain `if` blocks, etc.).

### The Full Node Enum

```rust
#[derive(Debug, Clone)]
pub enum AstNode {
    /// A simple command: `ls -la`
    SimpleCommand(SimpleCommand),

    /// A pipeline: `ls | grep foo`
    Pipeline(Vec<AstNode>),

    /// A chain with connectors: `cmd1 && cmd2 || cmd3`
    Chain(Vec<(AstNode, Connector)>),

    /// An if/elif/else block.
    If(IfBlock),

    /// A while loop.
    While(WhileLoop),

    /// A for loop.
    For(ForLoop),

    /// A function definition.
    FunctionDef(FunctionDef),

    /// A subshell: `( commands )`
    Subshell(Vec<AstNode>),

    /// Variable assignment: `let x = value` or `x=value`
    Assignment { name: String, value: String },

    /// Background execution: `command &`
    Background(Box<AstNode>),
}

#[derive(Debug, Clone)]
pub struct SimpleCommand {
    pub argv: Vec<String>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub fd: i32,
    pub kind: RedirectKind,
    pub target: String,
}

#[derive(Debug, Clone)]
pub enum RedirectKind {
    Output,       // >
    Append,       // >>
    Input,        // <
    HereDoc,      // <<
    HereString,   // <<<
}
```

### Visualising the Tree

For the input:

```
if test -f Cargo.toml {
    echo "building" && cargo build
} else {
    echo "not a Rust project"
}
```

The AST looks like:

```
If
+-- condition: SimpleCommand ["test", "-f", "Cargo.toml"]
+-- body:
|   +-- Chain
|       +-- SimpleCommand ["echo", "building"]  -- And -->
|       +-- SimpleCommand ["cargo", "build"]
+-- else_body:
    +-- SimpleCommand ["echo", "not a Rust project"]
```

---

## Concept 9: Recursive Descent Parsing

### What Is Recursive Descent?

A recursive descent parser is a top-down parser where each grammar rule maps to
a function. The functions call each other (hence "recursive") and consume tokens
from left to right (hence "descent" through the grammar).

### Grammar for james-shell

Here is a simplified grammar in EBNF notation:

```
program     = statement*
statement   = if_stmt | while_stmt | for_stmt | fn_def | chain
chain       = pipeline ( ( '&&' | '||' | ';' ) pipeline )*
pipeline    = command ( '|' command )*
command     = '(' program ')'         -- subshell
            | simple_command
simple_cmd  = WORD+ redirect*
redirect    = ( '>' | '>>' | '<' ) WORD
if_stmt     = 'if' chain '{' program '}'
              ( 'elif' chain '{' program '}' )*
              ( 'else' '{' program '}' )?
while_stmt  = 'while' chain '{' program '}'
for_stmt    = 'for' WORD 'in' WORD* '{' program '}'
fn_def      = 'fn' WORD '(' WORD* ')' '{' program '}'
```

### Parser Structure

```rust
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ---- Helpers ----

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        match self.advance() {
            Some(tok) if tok == expected => Ok(()),
            Some(tok) => Err(ParseError::Unexpected(tok.clone())),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    fn expect_word(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(Token::Word(w)) => Ok(w.clone()),
            other => Err(ParseError::ExpectedWord(format!("{:?}", other))),
        }
    }

    // ---- Grammar rules ----

    /// program = statement*
    pub fn parse_program(&mut self) -> Result<Vec<AstNode>, ParseError> {
        let mut nodes = Vec::new();
        while self.peek().is_some() && self.peek() != Some(&Token::CloseBrace) {
            nodes.push(self.parse_statement()?);
        }
        Ok(nodes)
    }

    /// statement = if_stmt | while_stmt | for_stmt | fn_def | chain
    fn parse_statement(&mut self) -> Result<AstNode, ParseError> {
        match self.peek() {
            Some(Token::Word(w)) if w == "if" => self.parse_if(),
            Some(Token::Word(w)) if w == "while" => self.parse_while(),
            Some(Token::Word(w)) if w == "for" => self.parse_for(),
            Some(Token::Word(w)) if w == "fn" => self.parse_fn_def(),
            _ => self.parse_chain(),
        }
    }

    /// chain = pipeline ( ( '&&' | '||' | ';' ) pipeline )*
    fn parse_chain(&mut self) -> Result<AstNode, ParseError> {
        let first = self.parse_pipeline()?;
        let mut parts = vec![(first, Connector::Sequence)];

        loop {
            let connector = match self.peek() {
                Some(Token::And) => { self.advance(); Connector::And }
                Some(Token::Or) => { self.advance(); Connector::Or }
                Some(Token::Semi) => { self.advance(); Connector::Sequence }
                _ => break,
            };
            let next = self.parse_pipeline()?;
            parts.push((next, connector));
        }

        if parts.len() == 1 {
            Ok(parts.remove(0).0)
        } else {
            Ok(AstNode::Chain(parts))
        }
    }

    /// pipeline = command ( '|' command )*
    fn parse_pipeline(&mut self) -> Result<AstNode, ParseError> {
        let first = self.parse_command()?;
        let mut cmds = vec![first];

        while self.peek() == Some(&Token::Pipe) {
            self.advance();
            cmds.push(self.parse_command()?);
        }

        if cmds.len() == 1 {
            Ok(cmds.remove(0))
        } else {
            Ok(AstNode::Pipeline(cmds))
        }
    }

    /// if_stmt = 'if' chain '{' program '}' ...
    fn parse_if(&mut self) -> Result<AstNode, ParseError> {
        self.advance(); // consume 'if'
        let condition = vec![self.parse_chain()?];
        self.expect(&Token::OpenBrace)?;
        let body = self.parse_program()?;
        self.expect(&Token::CloseBrace)?;

        let mut elif_branches = Vec::new();
        while self.peek() == Some(&Token::Word("elif".to_string())) {
            self.advance();
            let elif_cond = vec![self.parse_chain()?];
            self.expect(&Token::OpenBrace)?;
            let elif_body = self.parse_program()?;
            self.expect(&Token::CloseBrace)?;
            elif_branches.push((elif_cond, elif_body));
        }

        let else_body = if self.peek() == Some(&Token::Word("else".to_string())) {
            self.advance();
            self.expect(&Token::OpenBrace)?;
            let body = self.parse_program()?;
            self.expect(&Token::CloseBrace)?;
            Some(body)
        } else {
            None
        };

        Ok(AstNode::If(IfBlock {
            condition,
            body,
            elif_branches,
            else_body,
        }))
    }

    // parse_while, parse_for, parse_fn_def follow the same pattern...
}
```

### Error Recovery

When a parse error occurs we want to give the user a helpful message and
continue if we are in interactive mode. A simple strategy: skip tokens until
we reach a newline or `;`, then resume parsing.

```rust
#[derive(Debug)]
pub enum ParseError {
    UnexpectedEof,
    Unexpected(Token),
    ExpectedWord(String),
    UnclosedBlock(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseError::Unexpected(tok) => write!(f, "unexpected token: {:?}", tok),
            ParseError::ExpectedWord(ctx) => write!(f, "expected a word, got {}", ctx),
            ParseError::UnclosedBlock(kind) => write!(f, "unclosed {} block", kind),
        }
    }
}
```

---

## Concept 10: Putting It All Together -- The Evaluation Loop

Here is how the main evaluation function dispatches on AST nodes:

```rust
pub fn eval_nodes(nodes: &[AstNode], env: &mut ShellEnv) -> i32 {
    let mut last_status = 0;

    for node in nodes {
        // Check for break/continue/return before each statement.
        if env.control_flow.is_some() {
            break;
        }

        last_status = eval_node(node, env);
    }

    last_status
}

pub fn eval_node(node: &AstNode, env: &mut ShellEnv) -> i32 {
    match node {
        AstNode::SimpleCommand(cmd) => execute_simple_command(cmd, env),
        AstNode::Pipeline(cmds) => execute_pipeline(cmds, env),
        AstNode::Chain(parts) => execute_chain(parts),
        AstNode::If(block) => eval_if(block, env),
        AstNode::While(wl) => eval_while(wl, env),
        AstNode::For(fl) => eval_for(fl, env),
        AstNode::FunctionDef(fd) => {
            env.functions.insert(fd.name.clone(), fd.clone());
            0
        }
        AstNode::Subshell(nodes) => eval_subshell(nodes, env),
        AstNode::Assignment { name, value } => {
            let expanded = expand_all(value, env);
            env.set_var(name, &expanded);
            0
        }
        AstNode::Background(inner) => {
            spawn_background(inner, env);
            0
        }
    }
}
```

---

## Key Rust Concepts Used

| Concept | Where it appears |
|---------|-----------------|
| **Enums with data** | `AstNode`, `Token`, `ControlFlow`, `Connector` |
| **Recursive data structures** | `AstNode` contains `Vec<AstNode>` |
| **`Box<T>`** | Heap-allocated nodes in `Background(Box<AstNode>)` |
| **Pattern matching** | The parser's `match self.peek()` and evaluator's `match node` |
| **`Clone` trait** | Cloning `ShellEnv` for subshells, cloning `FunctionDef` for calls |
| **Ownership & borrowing** | Parser borrows the token list; evaluator borrows the AST |
| **`HashMap`** | Function table, variable scopes |
| **`Option<T>`** | `peek()` returns `Option`, control flow uses `Option<ControlFlow>` |
| **Error handling** | `ParseError` enum, `Result<T, ParseError>` |
| **Iterators** | `for item in &fl.items`, `chars.peekable()` |

---

## Milestone

After implementing this module, a session should look like this:

```
jsh> fn fib(n) {
...>     if test $n -le 1 {
...>         echo $n
...>         return
...>     }
...>     let a = $(fib $(( n - 1 )))
...>     let b = $(fib $(( n - 2 )))
...>     echo $(( a + b ))
...> }

jsh> fib 10
55

jsh> for i in 1 2 3 4 5 {
...>     if test $i -eq 3 {
...>         echo "skipping 3"
...>         continue
...>     }
...>     echo "number: $i"
...> }
number: 1
number: 2
skipping 3
number: 4
number: 5

jsh> echo "Files: $(ls | wc -l)"
Files: 42

jsh> (cd /tmp; echo "In subshell: $(pwd)")
In subshell: /tmp
jsh> pwd
/home/user/james-shell

jsh> cat deploy.jsh
#!/usr/bin/env jsh
echo "Building..."
cargo build --release && echo "Build succeeded" || echo "Build failed"

jsh> source deploy.jsh
Building...
Build succeeded
```

---

## What's Next?

In **Module 12** we will add advanced features that make james-shell pleasant to
use interactively: aliases, prompt customization with git integration, arithmetic
expansion, process substitution, shell options, startup files, and more. The
scripting engine from this module forms the foundation for all of that.
