# Module 12: Advanced Features

## What are we building?

With a scripting engine in place from Module 11, james-shell is now a capable
command interpreter. But day-to-day usability depends on dozens of smaller
features that collectively make a shell feel polished. In this module we add the
features that power users expect:

1. **Aliases** -- shorthand names for common commands.
2. **Prompt customization** -- a PS1-like system with colours and git info.
3. **Arithmetic expansion** -- `$((1 + 2))`.
4. **Process substitution** -- `<(command)` and `>(command)`.
5. **Shell options** -- `set -e`, `set -x`, `set -u`, `set -o pipefail`.
6. **Startup files** -- `.jshrc` equivalent.
7. **`exec` builtin** -- replace the shell process.
8. **Command timing** -- `time command`.
9. **String manipulation builtins**.

Each feature is self-contained, so you can implement them in any order.

---

## Concept 1: Aliases and Alias Expansion

### What Are Aliases?

An alias maps a short name to a replacement string. When the shell sees the
alias name as the **first word** of a simple command, it substitutes the
replacement before any further parsing.

```
jsh> alias ll = "ls -la"
jsh> ll /tmp
# Executes: ls -la /tmp
```

### Storage

```rust
use std::collections::HashMap;

pub struct ShellEnv {
    pub aliases: HashMap<String, String>,
    // ... other fields
}
```

### The `alias` and `unalias` Builtins

```rust
pub fn builtin_alias(args: &[&str], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // Print all defined aliases.
        let mut sorted: Vec<_> = env.aliases.iter().collect();
        sorted.sort_by_key(|(k, _)| k.clone());
        for (name, value) in sorted {
            println!("alias {} = {:?}", name, value);
        }
        return 0;
    }

    // Parse: alias name = "value"
    // We accept: alias name="value" OR alias name = "value"
    let input = args.join(" ");
    if let Some(eq_pos) = input.find('=') {
        let name = input[..eq_pos].trim().to_string();
        let value = input[eq_pos + 1..].trim().trim_matches('"').to_string();
        env.aliases.insert(name, value);
        0
    } else {
        // Show a single alias.
        let name = args[0];
        match env.aliases.get(name) {
            Some(value) => {
                println!("alias {} = {:?}", name, value);
                0
            }
            None => {
                eprintln!("alias: {}: not found", name);
                1
            }
        }
    }
}

pub fn builtin_unalias(args: &[&str], env: &mut ShellEnv) -> i32 {
    for name in args {
        if env.aliases.remove(*name).is_none() {
            eprintln!("unalias: {}: not found", name);
        }
    }
    0
}
```

### Alias Expansion

Expansion happens **after lexing** but **before parsing**. We only expand the
first token of each simple command, and we guard against infinite recursion.

```rust
const MAX_ALIAS_DEPTH: usize = 16;

pub fn expand_aliases(
    tokens: &mut Vec<Token>,
    aliases: &HashMap<String, String>,
) {
    expand_aliases_inner(tokens, aliases, 0);
}

fn expand_aliases_inner(
    tokens: &mut Vec<Token>,
    aliases: &HashMap<String, String>,
    depth: usize,
) {
    if depth >= MAX_ALIAS_DEPTH {
        eprintln!("jsh: alias expansion: maximum depth reached");
        return;
    }

    // Find positions where a command starts (beginning, after ;, after &&,
    // after ||, after |, after {, after ().
    let mut i = 0;
    let mut at_command_start = true;

    while i < tokens.len() {
        if at_command_start {
            if let Token::Word(word) = &tokens[i] {
                if let Some(replacement) = aliases.get(word.as_str()) {
                    // Re-lex the replacement.
                    let mut expanded = lex(replacement);
                    // Remove the original token and splice in the expansion.
                    tokens.remove(i);
                    let count = expanded.len();
                    for (j, tok) in expanded.drain(..).enumerate() {
                        tokens.insert(i + j, tok);
                    }
                    // Recursively expand (in case the expansion starts with
                    // another alias).
                    expand_aliases_inner(tokens, aliases, depth + 1);
                    return; // Start over since indices shifted.
                }
            }
        }

        // Determine if the next token starts a new command.
        at_command_start = matches!(
            &tokens[i],
            Token::Semi | Token::And | Token::Or | Token::Pipe
                | Token::OpenBrace | Token::OpenParen
        );
        i += 1;
    }
}
```

### Alias vs Function

| Aspect | Alias | Function |
|--------|-------|----------|
| Expansion time | Before parsing | During evaluation |
| Can contain control flow | No (text substitution only) | Yes |
| Parameters | No (`$1` etc. do not work) | Yes |
| Recursion | Limited by depth guard | Full recursion |

Use aliases for simple abbreviations. Use functions for anything complex.

---

## Concept 2: Prompt Customization

### The Prompt String

Bash uses `PS1` to define the prompt. We support a similar variable, `JSH_PROMPT`,
with escape sequences for dynamic content.

| Escape | Meaning |
|--------|---------|
| `\u` | Current username |
| `\h` | Hostname (short) |
| `\w` | Current working directory (with `~` for home) |
| `\W` | Basename of current directory |
| `\$` | `#` if root/admin, `$` otherwise |
| `\?` | Exit code of last command |
| `\g` | Current git branch (empty if not in a repo) |
| `\t` | Current time (HH:MM:SS) |
| `\e[Nm` | ANSI escape (for colours) |
| `\\` | Literal backslash |

### Default Prompt

```rust
pub const DEFAULT_PROMPT: &str = r"\e[32m\u@\h\e[0m:\e[34m\w\e[0m\g\$ ";
```

This produces something like:

```
james@laptop:~/james-shell (main)$
```

with the username/host in green, the path in blue, and the git branch in
parentheses.

### Prompt Rendering

```rust
use std::env;
use std::path::PathBuf;

pub fn render_prompt(template: &str, env: &ShellEnv) -> String {
    let mut result = String::new();
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('u') => result.push_str(&get_username()),
                Some('h') => result.push_str(&get_hostname()),
                Some('w') => result.push_str(&get_cwd_display(env)),
                Some('W') => result.push_str(&get_cwd_basename()),
                Some('$') => {
                    result.push(if is_elevated() { '#' } else { '$' });
                }
                Some('?') => {
                    result.push_str(&env.last_exit_code.to_string());
                }
                Some('g') => {
                    let branch = get_git_branch();
                    if !branch.is_empty() {
                        result.push_str(&format!(" ({})", branch));
                    }
                }
                Some('t') => {
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%H:%M:%S").to_string());
                }
                Some('e') => {
                    result.push('\x1b'); // ESC character
                }
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}
```

### Getting the Git Branch

```rust
use std::process::Command;

pub fn get_git_branch() -> String {
    // Fast path: read .git/HEAD directly.
    let head = std::fs::read_to_string(".git/HEAD").unwrap_or_default();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return branch.trim().to_string();
    }

    // Fallback: detached HEAD, show short hash.
    if head.len() >= 8 {
        return head[..8].to_string();
    }

    // Not in a git repo.
    String::new()
}
```

Reading `.git/HEAD` directly is much faster than spawning `git rev-parse` on
every prompt. We avoid the overhead of a child process for something that
happens on every single command.

### Cross-Platform Username and Hostname

```rust
pub fn get_username() -> String {
    #[cfg(unix)]
    {
        env::var("USER").unwrap_or_else(|_| "user".to_string())
    }
    #[cfg(windows)]
    {
        env::var("USERNAME").unwrap_or_else(|_| "user".to_string())
    }
}

pub fn get_hostname() -> String {
    #[cfg(unix)]
    {
        std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "localhost".to_string())
            .trim()
            .to_string()
    }
    #[cfg(windows)]
    {
        env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string())
    }
}

pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        // On Unix, UID 0 is root.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        // Check if running as administrator.
        // A full implementation would use the Windows API:
        // OpenProcessToken + CheckTokenMembership with BUILTIN_ADMINISTRATORS.
        // For simplicity we check an environment heuristic.
        env::var("USERPROFILE")
            .map(|p| p.contains("Administrator"))
            .unwrap_or(false)
    }
}
```

### Colour Table

For reference, here are the common ANSI colour codes used in prompts:

| Code | Colour |
|------|--------|
| `\e[30m` | Black |
| `\e[31m` | Red |
| `\e[32m` | Green |
| `\e[33m` | Yellow |
| `\e[34m` | Blue |
| `\e[35m` | Magenta |
| `\e[36m` | Cyan |
| `\e[37m` | White |
| `\e[0m`  | Reset |
| `\e[1m`  | Bold |
| `\e[4m`  | Underline |

Bright variants use `90-97` instead of `30-37`.

---

## Concept 3: Arithmetic Expansion

### Syntax

Arithmetic expansion evaluates a mathematical expression and replaces itself
with the result:

```
jsh> echo $((3 + 4 * 2))
11
jsh> let x = 10
jsh> echo $(( x / 3 ))
3
```

### Supported Operators

| Operator | Meaning | Precedence (higher = tighter) |
|----------|---------|-------------------------------|
| `( )` | Grouping | 7 |
| `- +` (unary) | Negation, no-op | 6 |
| `* / %` | Multiply, divide, modulo | 5 |
| `+ -` | Add, subtract | 4 |
| `<< >>` | Bit shift | 3 |
| `& \| ^` | Bitwise and, or, xor | 2 |
| `== != < > <= >=` | Comparison (returns 0 or 1) | 1 |

### Implementation: A Pratt Parser

We use a Pratt parser (top-down operator precedence) because it handles
precedence and associativity naturally.

```rust
#[derive(Debug, Clone)]
pub enum ArithExpr {
    Literal(i64),
    Variable(String),
    UnaryMinus(Box<ArithExpr>),
    BinaryOp {
        op: ArithOp,
        left: Box<ArithExpr>,
        right: Box<ArithExpr>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ArithOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Gt, Le, Ge,
    BitAnd, BitOr, BitXor,
    Shl, Shr,
}

impl ArithOp {
    fn precedence(self) -> u8 {
        match self {
            ArithOp::BitAnd | ArithOp::BitOr | ArithOp::BitXor => 2,
            ArithOp::Shl | ArithOp::Shr => 3,
            ArithOp::Add | ArithOp::Sub => 4,
            ArithOp::Mul | ArithOp::Div | ArithOp::Mod => 5,
            ArithOp::Eq | ArithOp::Ne
            | ArithOp::Lt | ArithOp::Gt
            | ArithOp::Le | ArithOp::Ge => 1,
        }
    }
}

pub struct ArithParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> ArithParser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len()
            && self.input.as_bytes()[self.pos].is_ascii_whitespace()
        {
            self.pos += 1;
        }
    }

    fn peek_char(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn advance(&mut self) -> u8 {
        let ch = self.input.as_bytes()[self.pos];
        self.pos += 1;
        ch
    }

    /// Parse a complete expression.
    pub fn parse_expr(&mut self) -> Result<ArithExpr, String> {
        self.parse_expr_bp(0)
    }

    /// Parse an expression with a minimum binding power.
    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<ArithExpr, String> {
        self.skip_whitespace();
        let mut lhs = self.parse_atom()?;

        loop {
            self.skip_whitespace();
            let Some(op) = self.peek_operator() else {
                break;
            };
            let prec = op.precedence();
            if prec <= min_bp {
                break;
            }
            self.consume_operator(op);
            let rhs = self.parse_expr_bp(prec)?;
            lhs = ArithExpr::BinaryOp {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    fn parse_atom(&mut self) -> Result<ArithExpr, String> {
        self.skip_whitespace();
        match self.peek_char() {
            // Parenthesised sub-expression.
            Some(b'(') => {
                self.advance();
                let expr = self.parse_expr()?;
                self.skip_whitespace();
                if self.peek_char() != Some(b')') {
                    return Err("expected ')'".to_string());
                }
                self.advance();
                Ok(expr)
            }
            // Unary minus.
            Some(b'-') => {
                self.advance();
                let inner = self.parse_atom()?;
                Ok(ArithExpr::UnaryMinus(Box::new(inner)))
            }
            // Number literal.
            Some(ch) if ch.is_ascii_digit() => {
                let start = self.pos;
                while self.pos < self.input.len()
                    && self.input.as_bytes()[self.pos].is_ascii_digit()
                {
                    self.pos += 1;
                }
                let num: i64 = self.input[start..self.pos]
                    .parse()
                    .map_err(|e| format!("invalid number: {}", e))?;
                Ok(ArithExpr::Literal(num))
            }
            // Variable name.
            Some(ch) if ch.is_ascii_alphabetic() || ch == b'_' => {
                let start = self.pos;
                while self.pos < self.input.len() && {
                    let c = self.input.as_bytes()[self.pos];
                    c.is_ascii_alphanumeric() || c == b'_'
                } {
                    self.pos += 1;
                }
                Ok(ArithExpr::Variable(self.input[start..self.pos].to_string()))
            }
            other => Err(format!("unexpected character: {:?}", other.map(|c| c as char))),
        }
    }

    fn peek_operator(&self) -> Option<ArithOp> {
        let bytes = &self.input.as_bytes()[self.pos..];
        match bytes {
            [b'=', b'=', ..] => Some(ArithOp::Eq),
            [b'!', b'=', ..] => Some(ArithOp::Ne),
            [b'<', b'=', ..] => Some(ArithOp::Le),
            [b'>', b'=', ..] => Some(ArithOp::Ge),
            [b'<', b'<', ..] => Some(ArithOp::Shl),
            [b'>', b'>', ..] => Some(ArithOp::Shr),
            [b'+', ..] => Some(ArithOp::Add),
            [b'-', ..] => Some(ArithOp::Sub),
            [b'*', ..] => Some(ArithOp::Mul),
            [b'/', ..] => Some(ArithOp::Div),
            [b'%', ..] => Some(ArithOp::Mod),
            [b'&', ..] => Some(ArithOp::BitAnd),
            [b'|', ..] => Some(ArithOp::BitOr),
            [b'^', ..] => Some(ArithOp::BitXor),
            [b'<', ..] => Some(ArithOp::Lt),
            [b'>', ..] => Some(ArithOp::Gt),
            _ => None,
        }
    }

    fn consume_operator(&mut self, op: ArithOp) {
        // Two-character operators.
        let len = match op {
            ArithOp::Eq | ArithOp::Ne | ArithOp::Le | ArithOp::Ge
            | ArithOp::Shl | ArithOp::Shr => 2,
            _ => 1,
        };
        self.pos += len;
    }
}

/// Evaluate an arithmetic expression tree.
pub fn eval_arith(expr: &ArithExpr, env: &ShellEnv) -> Result<i64, String> {
    match expr {
        ArithExpr::Literal(n) => Ok(*n),
        ArithExpr::Variable(name) => {
            let val_str = env.get_var(name).unwrap_or("0");
            val_str.parse::<i64>()
                .map_err(|_| format!("{}: not a valid number", name))
        }
        ArithExpr::UnaryMinus(inner) => {
            Ok(-eval_arith(inner, env)?)
        }
        ArithExpr::BinaryOp { op, left, right } => {
            let l = eval_arith(left, env)?;
            let r = eval_arith(right, env)?;
            match op {
                ArithOp::Add => Ok(l + r),
                ArithOp::Sub => Ok(l - r),
                ArithOp::Mul => Ok(l * r),
                ArithOp::Div => {
                    if r == 0 {
                        Err("division by zero".to_string())
                    } else {
                        Ok(l / r)
                    }
                }
                ArithOp::Mod => {
                    if r == 0 {
                        Err("modulo by zero".to_string())
                    } else {
                        Ok(l % r)
                    }
                }
                ArithOp::Eq => Ok(if l == r { 1 } else { 0 }),
                ArithOp::Ne => Ok(if l != r { 1 } else { 0 }),
                ArithOp::Lt => Ok(if l < r { 1 } else { 0 }),
                ArithOp::Gt => Ok(if l > r { 1 } else { 0 }),
                ArithOp::Le => Ok(if l <= r { 1 } else { 0 }),
                ArithOp::Ge => Ok(if l >= r { 1 } else { 0 }),
                ArithOp::BitAnd => Ok(l & r),
                ArithOp::BitOr => Ok(l | r),
                ArithOp::BitXor => Ok(l ^ r),
                ArithOp::Shl => Ok(l << r),
                ArithOp::Shr => Ok(l >> r),
            }
        }
    }
}
```

### Integration with the Expansion Phase

In the word expansion function, we look for `$((...))` patterns. Note the
**double** parentheses -- this distinguishes arithmetic expansion from command
substitution `$(...)`.

```rust
pub fn expand_word(word: &str, env: &ShellEnv) -> String {
    let mut result = String::new();
    let mut chars = word.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'(') {
                // Check for $(( ... )) vs $( ... )
                let mut lookahead = chars.clone();
                lookahead.next(); // skip first '('
                if lookahead.peek() == Some(&'(') {
                    // Arithmetic expansion: $(( expr ))
                    chars.next(); // consume first '('
                    chars.next(); // consume second '('
                    let expr_str = extract_until_double_close(&mut chars);
                    match parse_and_eval_arith(&expr_str, env) {
                        Ok(value) => result.push_str(&value.to_string()),
                        Err(e) => eprintln!("jsh: arithmetic error: {}", e),
                    }
                } else {
                    // Command substitution: $( command )
                    chars.next(); // consume '('
                    let inner = extract_balanced_paren(&mut chars);
                    let output = capture_command_output(&inner, env);
                    result.push_str(output.trim_end_matches('\n'));
                }
            } else {
                // Variable expansion: $name or ${name}
                result.push_str(&expand_variable(&mut chars, env));
            }
        } else {
            result.push(c);
        }
    }

    result
}
```

---

## Concept 4: Process Substitution

### What Is Process Substitution?

Process substitution lets you use the output (or input) of a command where a
**filename** is expected. The shell creates a temporary file (or named pipe) and
passes its path to the outer command.

```
jsh> diff <(sort file1.txt) <(sort file2.txt)
```

Here, `<(sort file1.txt)` creates a temporary file containing the sorted output
of `file1.txt`, and its path is passed to `diff`.

| Syntax | Meaning |
|--------|---------|
| `<(command)` | The output of `command` is available as a readable file. |
| `>(command)` | A writable file whose contents are fed as input to `command`. |

### Implementation on Unix: Named Pipes (FIFOs)

On Unix we create a FIFO with `mkfifo`, spawn the inner command writing to it
in a background thread, and pass the FIFO path to the outer command.

```rust
#[cfg(unix)]
pub fn process_substitution_read(
    cmd_str: &str,
    env: &ShellEnv,
) -> std::io::Result<String> {
    use std::os::unix::fs::OpenOptionsExt;
    use nix::unistd::mkfifo;
    use nix::sys::stat::Mode;

    // Create a unique FIFO path.
    let fifo_path = format!("/tmp/jsh_procsub_{}", std::process::id());
    mkfifo(fifo_path.as_str(), Mode::S_IRUSR | Mode::S_IWUSR)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Spawn a thread that writes to the FIFO.
    let cmd_owned = cmd_str.to_string();
    let path_owned = fifo_path.clone();
    let env_clone = env.clone();
    std::thread::spawn(move || {
        let output = capture_command_output(&cmd_owned, &env_clone);
        if let Ok(mut f) = std::fs::File::create(&path_owned) {
            use std::io::Write;
            let _ = f.write_all(output.as_bytes());
        }
    });

    Ok(fifo_path)
}
```

### Implementation on Windows: Temporary Files

Windows does not have named pipes in the Unix sense that can appear in the
filesystem. Instead, we use temporary files.

```rust
#[cfg(windows)]
pub fn process_substitution_read(
    cmd_str: &str,
    env: &ShellEnv,
) -> std::io::Result<String> {
    use std::io::Write;

    // Create a temporary file.
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!("jsh_procsub_{}.tmp", std::process::id()));

    // Run the command and capture its output.
    let output = capture_command_output(cmd_str, env);

    // Write output to the temp file.
    let mut file = std::fs::File::create(&temp_path)?;
    file.write_all(output.as_bytes())?;
    file.flush()?;

    // Return the path so the outer command can read from it.
    Ok(temp_path.to_string_lossy().to_string())
}
```

### Cleanup

We register temporary files for cleanup when the command finishes:

```rust
pub struct ProcessSubCleanup {
    paths: Vec<String>,
}

impl Drop for ProcessSubCleanup {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}
```

### Integration with the Parser

During expansion, when we encounter `<(` or `>(` at the start of a word, we
treat it as a process substitution rather than a redirect. The key distinction:

- `< (` (with a space) is input redirect from a subshell.
- `<(` (no space) is process substitution.

```rust
fn expand_process_substitutions(
    args: &[String],
    env: &mut ShellEnv,
) -> (Vec<String>, ProcessSubCleanup) {
    let mut expanded = Vec::new();
    let mut cleanup = ProcessSubCleanup { paths: Vec::new() };

    for arg in args {
        if arg.starts_with("<(") && arg.ends_with(")") {
            let inner = &arg[2..arg.len() - 1];
            match process_substitution_read(inner, env) {
                Ok(path) => {
                    cleanup.paths.push(path.clone());
                    expanded.push(path);
                }
                Err(e) => {
                    eprintln!("jsh: process substitution failed: {}", e);
                    expanded.push(arg.clone());
                }
            }
        } else if arg.starts_with(">(") && arg.ends_with(")") {
            let inner = &arg[2..arg.len() - 1];
            match process_substitution_write(inner, env) {
                Ok(path) => {
                    cleanup.paths.push(path.clone());
                    expanded.push(path);
                }
                Err(e) => {
                    eprintln!("jsh: process substitution failed: {}", e);
                    expanded.push(arg.clone());
                }
            }
        } else {
            expanded.push(arg.clone());
        }
    }

    (expanded, cleanup)
}
```

---

## Concept 5: Shell Options (`set`)

### What Are Shell Options?

Shell options change how the interpreter behaves. They are toggled with the
`set` builtin using flags.

| Flag | Meaning |
|------|---------|
| `-e` | **errexit**: Exit the script immediately if any command fails. |
| `-x` | **xtrace**: Print each command before executing it (for debugging). |
| `-u` | **nounset**: Treat unset variables as an error. |
| `-o pipefail` | A pipeline fails if *any* command in it fails (not just the last). |

### Storage

```rust
#[derive(Debug, Clone, Default)]
pub struct ShellOptions {
    /// Exit on error (-e).
    pub errexit: bool,
    /// Print commands before execution (-x).
    pub xtrace: bool,
    /// Error on unset variables (-u).
    pub nounset: bool,
    /// Pipeline fails if any component fails.
    pub pipefail: bool,
}
```

### The `set` Builtin

```rust
pub fn builtin_set(args: &[&str], env: &mut ShellEnv) -> i32 {
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-e" => env.options.errexit = true,
            "+e" => env.options.errexit = false,
            "-x" => env.options.xtrace = true,
            "+x" => env.options.xtrace = false,
            "-u" => env.options.nounset = true,
            "+u" => env.options.nounset = false,
            "-o" => {
                i += 1;
                if i < args.len() {
                    match args[i] {
                        "pipefail" => env.options.pipefail = true,
                        other => eprintln!("set: unknown option: {}", other),
                    }
                }
            }
            "+o" => {
                i += 1;
                if i < args.len() {
                    match args[i] {
                        "pipefail" => env.options.pipefail = false,
                        other => eprintln!("set: unknown option: {}", other),
                    }
                }
            }
            other => {
                eprintln!("set: unrecognised flag: {}", other);
                return 1;
            }
        }
        i += 1;
    }
    0
}
```

### Integrating `errexit`

In the main evaluation loop, check after each command:

```rust
pub fn eval_nodes(nodes: &[AstNode], env: &mut ShellEnv) -> i32 {
    let mut last_status = 0;

    for node in nodes {
        if env.control_flow.is_some() {
            break;
        }

        last_status = eval_node(node, env);

        // errexit: if the command failed and we are not in a condition context,
        // abort execution.
        if env.options.errexit && last_status != 0 && !env.in_condition {
            env.control_flow = Some(ControlFlow::Exit(last_status));
            break;
        }
    }

    last_status
}
```

The `in_condition` flag is set to `true` while evaluating the condition part of
an `if` or `while` -- we do not want `set -e` to trigger on a deliberately
failing test.

### Integrating `xtrace`

Before executing each command, print it to stderr with a `+` prefix:

```rust
if env.options.xtrace {
    eprintln!("+ {}", cmd.argv.join(" "));
}
```

> **See also:** Module 21 (Diagnostics & Logging) extends xtrace into a full
> feature-flagged tracing system with structured events, per-subsystem
> filtering, file logging, and a command audit trail.

### Integrating `nounset`

During variable expansion, if a variable is not set:

```rust
fn expand_variable(name: &str, env: &ShellEnv) -> String {
    match env.get_var(name) {
        Some(val) => val.to_string(),
        None => {
            if env.options.nounset {
                eprintln!("jsh: {}: unbound variable", name);
                env.control_flow = Some(ControlFlow::Exit(1));
            }
            String::new()
        }
    }
}
```

### Integrating `pipefail`

In the pipeline executor, collect all exit codes:

```rust
pub fn execute_pipeline(cmds: &[AstNode], env: &mut ShellEnv) -> i32 {
    let statuses = run_pipeline_commands(cmds, env);

    if env.options.pipefail {
        // Return the rightmost non-zero exit code, or 0 if all succeeded.
        statuses.iter().rev()
            .find(|&&s| s != 0)
            .copied()
            .unwrap_or(0)
    } else {
        // Traditional: return the last command's exit code.
        *statuses.last().unwrap_or(&0)
    }
}
```

---

## Concept 6: Startup Files (`.jshrc`)

### Load Order

When james-shell starts interactively, it looks for configuration files in this
order:

1. `/etc/jshrc` (system-wide, Unix only)
2. `~/.jshrc` (user configuration)
3. `$JSH_ENV` if set (custom file)

For non-interactive (script) execution, only `$JSH_ENV` is loaded.

### Implementation

```rust
use std::path::PathBuf;
use dirs::home_dir;

pub fn load_startup_files(env: &mut ShellEnv, interactive: bool) {
    if interactive {
        // System-wide config (Unix only).
        #[cfg(unix)]
        {
            let system_rc = PathBuf::from("/etc/jshrc");
            if system_rc.exists() {
                source_file(&system_rc, env);
            }
        }

        // User config.
        if let Some(home) = home_dir() {
            let user_rc = home.join(".jshrc");
            if user_rc.exists() {
                source_file(&user_rc, env);
            }
        }
    }

    // Custom env file.
    if let Some(env_file) = env.get_var("JSH_ENV") {
        let path = PathBuf::from(env_file);
        if path.exists() {
            source_file(&path, env);
        }
    }
}

fn source_file(path: &PathBuf, env: &mut ShellEnv) {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let tokens = lex(&contents);
            let ast = parse(&tokens);
            eval_nodes(&ast, env);
        }
        Err(e) => {
            eprintln!("jsh: warning: could not read {}: {}", path.display(), e);
        }
    }
}
```

### Example `.jshrc`

```bash
# ~/.jshrc -- james-shell configuration

# Prompt
let JSH_PROMPT = "\e[32m\u@\h\e[0m:\e[34m\w\e[0m\g\$ "

# Aliases
alias ll = "ls -la"
alias gs = "git status"
alias gco = "git checkout"

# Options
set -o pipefail

# Functions
fn mkcd(dir) {
    mkdir -p $dir && cd $dir
}

# PATH additions
let PATH = "$HOME/.local/bin:$PATH"

echo "Welcome to james-shell!"
```

---

## Concept 7: The `exec` Builtin

### What Does `exec` Do?

`exec` replaces the current shell process with the specified command. After
`exec`, the shell is gone -- the new process takes its place with the same PID.

Common uses:

- **Wrapper scripts**: `exec` the real program so signals go directly to it.
- **File descriptor manipulation**: `exec 3>file` opens fd 3 (without replacing the process).

### Implementation

```rust
pub fn builtin_exec(args: &[&str], env: &mut ShellEnv) -> i32 {
    if args.is_empty() {
        // No arguments: just apply redirections (not implemented here).
        return 0;
    }

    let program = args[0];
    let cmd_args = &args[1..];

    // On Unix, use execvp to replace the process.
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use nix::unistd::execvp;

        let c_program = CString::new(program).unwrap();
        let c_args: Vec<CString> = std::iter::once(program)
            .chain(cmd_args.iter().copied())
            .map(|s| CString::new(s).unwrap())
            .collect();

        // This only returns if it fails.
        match execvp(&c_program, &c_args) {
            Err(e) => {
                eprintln!("exec: {}: {}", program, e);
                1
            }
            Ok(_) => unreachable!(), // execvp does not return on success.
        }
    }

    // On Windows, there is no direct exec equivalent. We spawn the process
    // and exit the shell.
    #[cfg(windows)]
    {
        use std::process::{Command, exit};

        let status = Command::new(program)
            .args(cmd_args)
            .status();

        match status {
            Ok(s) => exit(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("exec: {}: {}", program, e);
                1
            }
        }
    }
}
```

### Cross-Platform Difference

| Platform | Behaviour |
|----------|-----------|
| Unix | True `exec`: the shell process is replaced in-place via `execvp(2)`. |
| Windows | Simulated: we spawn the child, wait for it, and exit with its code. The PID changes. |

For most practical purposes the difference is invisible to the user.

---

## Concept 8: Command Timing

### The `time` Prefix

Prefixing a command with `time` measures how long it takes to execute and prints
a summary to stderr.

```
jsh> time sleep 2

real    0m2.003s
user    0m0.001s
sys     0m0.002s
```

### Implementation

We measure wall-clock time with `std::time::Instant`. For user/system CPU time,
we use platform-specific APIs.

```rust
use std::time::Instant;

pub fn execute_timed(node: &AstNode, env: &mut ShellEnv) -> i32 {
    let start = Instant::now();

    // On Unix, we can get CPU times before and after.
    #[cfg(unix)]
    let cpu_before = get_cpu_times();

    let status = eval_node(node, env);

    let elapsed = start.elapsed();

    #[cfg(unix)]
    let cpu_after = get_cpu_times();

    // Print timing information.
    let real_secs = elapsed.as_secs_f64();
    let real_min = (real_secs / 60.0).floor() as u64;
    let real_sec = real_secs % 60.0;

    eprintln!();
    eprintln!("real    {}m{:.3}s", real_min, real_sec);

    #[cfg(unix)]
    {
        let user = cpu_after.user - cpu_before.user;
        let sys = cpu_after.system - cpu_before.system;
        eprintln!(
            "user    {}m{:.3}s",
            (user / 60.0).floor() as u64,
            user % 60.0
        );
        eprintln!(
            "sys     {}m{:.3}s",
            (sys / 60.0).floor() as u64,
            sys % 60.0
        );
    }

    #[cfg(windows)]
    {
        // On Windows, we only report wall-clock time.
        eprintln!("user    -");
        eprintln!("sys     -");
    }

    status
}

#[cfg(unix)]
struct CpuTimes {
    user: f64,
    system: f64,
}

#[cfg(unix)]
fn get_cpu_times() -> CpuTimes {
    use libc::{getrusage, rusage, RUSAGE_CHILDREN};
    use std::mem::MaybeUninit;

    let mut usage = MaybeUninit::<rusage>::zeroed();
    unsafe {
        getrusage(RUSAGE_CHILDREN, usage.as_mut_ptr());
    }
    let usage = unsafe { usage.assume_init() };

    CpuTimes {
        user: usage.ru_utime.tv_sec as f64
            + usage.ru_utime.tv_usec as f64 / 1_000_000.0,
        system: usage.ru_stime.tv_sec as f64
            + usage.ru_stime.tv_usec as f64 / 1_000_000.0,
    }
}
```

### Parser Integration

The parser recognises `time` as a keyword prefix:

```rust
fn parse_command(&mut self) -> Result<AstNode, ParseError> {
    if self.peek() == Some(&Token::Word("time".to_string())) {
        self.advance();
        let inner = self.parse_pipeline()?;
        return Ok(AstNode::Timed(Box::new(inner)));
    }

    // ... normal command parsing
}
```

---

## Concept 9: String Manipulation Builtins

### Why Builtins?

External tools like `cut`, `tr`, `sed`, and `awk` handle string manipulation in
traditional shells. But spawning a process for a simple string operation is
expensive. We provide lightweight builtins for the most common operations.

### Builtin Table

| Builtin | Usage | Description |
|---------|-------|-------------|
| `strlen` | `strlen STRING` | Print the length of STRING. |
| `substr` | `substr STRING START [LEN]` | Print a substring. |
| `upper` | `upper STRING` | Convert to uppercase. |
| `lower` | `lower STRING` | Convert to lowercase. |
| `trim` | `trim STRING` | Remove leading/trailing whitespace. |
| `replace` | `replace STRING PATTERN REPLACEMENT` | Replace first occurrence. |
| `replace_all` | `replace_all STRING PATTERN REPLACEMENT` | Replace all occurrences. |
| `split` | `split STRING DELIMITER` | Split and print one element per line. |
| `starts_with` | `starts_with STRING PREFIX` | Exit 0 if true, 1 if false. |
| `ends_with` | `ends_with STRING SUFFIX` | Exit 0 if true, 1 if false. |
| `contains` | `contains STRING SUBSTRING` | Exit 0 if true, 1 if false. |

### Implementation

```rust
pub fn register_string_builtins(builtins: &mut HashMap<String, BuiltinFn>) {
    builtins.insert("strlen".to_string(), builtin_strlen);
    builtins.insert("substr".to_string(), builtin_substr);
    builtins.insert("upper".to_string(), builtin_upper);
    builtins.insert("lower".to_string(), builtin_lower);
    builtins.insert("trim".to_string(), builtin_trim);
    builtins.insert("replace".to_string(), builtin_replace);
    builtins.insert("replace_all".to_string(), builtin_replace_all);
    builtins.insert("split".to_string(), builtin_split);
    builtins.insert("starts_with".to_string(), builtin_starts_with);
    builtins.insert("ends_with".to_string(), builtin_ends_with);
    builtins.insert("contains".to_string(), builtin_contains);
}

fn builtin_strlen(args: &[&str], _env: &mut ShellEnv) -> i32 {
    let s = args.first().unwrap_or(&"");
    println!("{}", s.len());
    0
}

fn builtin_substr(args: &[&str], _env: &mut ShellEnv) -> i32 {
    let s = args.first().unwrap_or(&"");
    let start: usize = args.get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let len: Option<usize> = args.get(2)
        .and_then(|s| s.parse().ok());

    // Work with character indices, not byte indices.
    let chars: Vec<char> = s.chars().collect();
    let end = match len {
        Some(l) => (start + l).min(chars.len()),
        None => chars.len(),
    };

    if start <= chars.len() {
        let result: String = chars[start..end].iter().collect();
        println!("{}", result);
        0
    } else {
        println!();
        1
    }
}

fn builtin_upper(args: &[&str], _env: &mut ShellEnv) -> i32 {
    let s = args.join(" ");
    println!("{}", s.to_uppercase());
    0
}

fn builtin_lower(args: &[&str], _env: &mut ShellEnv) -> i32 {
    let s = args.join(" ");
    println!("{}", s.to_lowercase());
    0
}

fn builtin_trim(args: &[&str], _env: &mut ShellEnv) -> i32 {
    let s = args.join(" ");
    println!("{}", s.trim());
    0
}

fn builtin_replace(args: &[&str], _env: &mut ShellEnv) -> i32 {
    if args.len() < 3 {
        eprintln!("replace: usage: replace STRING PATTERN REPLACEMENT");
        return 1;
    }
    let result = args[0].replacen(args[1], args[2], 1);
    println!("{}", result);
    0
}

fn builtin_replace_all(args: &[&str], _env: &mut ShellEnv) -> i32 {
    if args.len() < 3 {
        eprintln!("replace_all: usage: replace_all STRING PATTERN REPLACEMENT");
        return 1;
    }
    let result = args[0].replace(args[1], args[2]);
    println!("{}", result);
    0
}

fn builtin_split(args: &[&str], _env: &mut ShellEnv) -> i32 {
    if args.len() < 2 {
        eprintln!("split: usage: split STRING DELIMITER");
        return 1;
    }
    for part in args[0].split(args[1]) {
        println!("{}", part);
    }
    0
}

fn builtin_starts_with(args: &[&str], _env: &mut ShellEnv) -> i32 {
    match (args.first(), args.get(1)) {
        (Some(s), Some(prefix)) => if s.starts_with(prefix) { 0 } else { 1 },
        _ => {
            eprintln!("starts_with: usage: starts_with STRING PREFIX");
            2
        }
    }
}

fn builtin_ends_with(args: &[&str], _env: &mut ShellEnv) -> i32 {
    match (args.first(), args.get(1)) {
        (Some(s), Some(suffix)) => if s.ends_with(suffix) { 0 } else { 1 },
        _ => {
            eprintln!("ends_with: usage: ends_with STRING SUFFIX");
            2
        }
    }
}

fn builtin_contains(args: &[&str], _env: &mut ShellEnv) -> i32 {
    match (args.first(), args.get(1)) {
        (Some(s), Some(sub)) => if s.contains(sub) { 0 } else { 1 },
        _ => {
            eprintln!("contains: usage: contains STRING SUBSTRING");
            2
        }
    }
}
```

### Usage Examples

```
jsh> let name = "james-shell"
jsh> strlen $name
11
jsh> upper $name
JAMES-SHELL
jsh> substr $name 6
shell
jsh> replace $name "james" "super"
super-shell
jsh> if contains $name "shell" { echo "It's a shell!" }
It's a shell!
jsh> split "a:b:c:d" ":"
a
b
c
d
```

---

## Key Rust Concepts Used

| Concept | Where it appears |
|---------|-----------------|
| **Conditional compilation (`#[cfg]`)** | Platform-specific code for exec, prompts, process substitution |
| **Trait implementations (`Display`, `Drop`)** | `ParseError`, `ProcessSubCleanup` |
| **Closures** | Pratt parser precedence lookups |
| **`Box<T>` for recursive types** | `ArithExpr::UnaryMinus(Box<ArithExpr>)` |
| **Byte-level string manipulation** | Arithmetic parser working with `as_bytes()` |
| **`HashMap` for dispatch** | Builtin function registration |
| **Thread spawning** | Process substitution background writer |
| **`Instant` and duration** | Command timing |
| **`Iterator` combinators** | `statuses.iter().rev().find(...)` for pipefail |
| **Pattern matching on slices** | Peeking at byte slices in the arithmetic parser |

---

## Milestone

After implementing this module, a session should look like this:

```
jsh> source ~/.jshrc
Welcome to james-shell!

james@laptop:~/james-shell (main)$ alias ll
alias ll = "ls -la"

james@laptop:~/james-shell (main)$ echo $((2 ** 10))
1024

james@laptop:~/james-shell (main)$ diff <(sort names1.txt) <(sort names2.txt)
2a3
> Charlie

james@laptop:~/james-shell (main)$ set -x
james@laptop:~/james-shell (main)$ echo hello
+ echo hello
hello

james@laptop:~/james-shell (main)$ set +x
james@laptop:~/james-shell (main)$ time sleep 1

real    0m1.002s
user    0m0.000s
sys     0m0.001s

james@laptop:~/james-shell (main)$ let msg = "Hello, World!"
james@laptop:~/james-shell (main)$ upper $msg
HELLO, WORLD!
james@laptop:~/james-shell (main)$ strlen $msg
13

james@laptop:~/james-shell (main)$ exec bash
user@laptop:~$
```

---

## What's Next?

In **Module 13** we turn our attention to **testing and robustness**. A shell
must handle every kind of input gracefully -- empty strings, binary data, deeply
nested subshells, and adversarial input. We will write unit tests, integration
tests, fuzz tests, and benchmarks to make james-shell production-quality.
