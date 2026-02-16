# Module 18: Modern Error Handling

## What are we building?

Error handling in bash is a disaster. By default, commands fail silently — a critical `rm` returns an error and your script charges ahead. The "solutions" (`set -e`, `trap ERR`, `$?`) are all half-measures riddled with gotchas. Every experienced shell scripter has a war story about a script that silently destroyed data because an error went unnoticed.

In this module, we give james-shell **real error handling** — the kind you'd expect from a modern programming language:

- **Strict mode by default** — errors stop execution. No more silent failures.
- **try/catch blocks** — catch errors, inspect them, decide what to do.
- **Structured error objects** — not just a number (exit code 1), but a message, a source location, a stack trace, and a category.
- **The `?` operator** — propagate errors up the call stack cleanly, inspired by Rust itself.
- **Rich error context** — "command `curl` failed with status 7 (connection refused) at line 12 of deploy.jsh, called from line 45 of main.jsh"

This is the module where james-shell scripts become *reliable* — where you can trust that if something goes wrong, you'll know about it.

---

## Concept 1: Why Bash Error Handling Is Terrible

Before building something better, let's understand what we're replacing. This is not academic — every one of these problems has bitten real production systems.

### Problem 1: Errors are silent by default

```bash
# Bash — this script has a catastrophic bug
cd /deployments/staging
rm -rf *

# What if /deployments/staging doesn't exist?
# cd fails silently, rm -rf * runs in whatever directory you were in.
# Congratulations, you just deleted your home directory.
```

The `cd` command failed with exit code 1, but bash didn't care. It ran the next line anyway. The `$?` variable held `1` for a brief moment, but nobody checked it.

### Problem 2: `set -e` is a minefield

Bash's `set -e` ("exit on error") is supposed to fix this, but it has so many exceptions that it's almost worse than useless:

```bash
set -e

# These DO trigger set -e (script exits):
false                          # bare failing command
ls /nonexistent                # command error

# These do NOT trigger set -e (silently ignored!):
false || true                  # part of an OR chain
if false; then echo hi; fi     # condition of an if
false && true                  # part of an AND chain
local x=$(false)               # in a local assignment
x=$(false)                     # this one DOES fail, but...
local x=$(false)               # ...this one DOESN'T. Surprise!

# This is the worst — it CHANGES behavior inside functions:
my_func() {
    false     # does this exit? depends on how my_func is called!
}

my_func         # YES, this exits
my_func || true # NO, set -e is disabled inside my_func for this call!
```

The rules for when `set -e` applies are so complex that the bash man page devotes multiple paragraphs to them, and experienced scripters still get surprised.

### Problem 3: No error context

```bash
$ ./deploy.sh
rm: cannot remove '/opt/app/logs': Permission denied
```

Which line? Which function? What was the script trying to do? You get a raw error message from `rm` and nothing else. Compare this to what we're building:

```
Error: command `rm` failed (exit code 1)
  Message: Permission denied: /opt/app/logs
  Location: deploy.jsh:34 in function clean_logs()
  Called from: deploy.jsh:78 in function deploy()
  Called from: main.jsh:12
  Suggestion: Run with elevated privileges, or skip log cleanup with --no-clean
```

### Comparison table

| Feature | Bash | james-shell |
|---------|------|-------------|
| Default on error | Silent continue | Stop execution |
| Error information | Exit code (0-255) | Structured object |
| Try/catch | No (`trap ERR` is not catch) | `try { } catch { }` |
| Error propagation | Manual `$?` checking | `?` operator |
| Stack traces | No | Yes |
| Error categories | No | Yes (IOError, CommandFailed, etc.) |
| Source locations | No | Line number, file, function |
| Typed errors | No (just integers) | Yes (structured records) |

---

## Concept 2: Designing the Error Type in Rust

Our shell's error type must carry enough information to produce those rich error messages. Here's the Rust side:

```rust
use std::fmt;
use std::path::PathBuf;

/// Every error in james-shell is one of these categories.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    /// An external command returned a non-zero exit code.
    CommandFailed,
    /// A command was not found in PATH or builtins.
    CommandNotFound,
    /// File I/O error (read, write, open, permissions).
    IOError,
    /// A syntax error in the script or interactive input.
    SyntaxError,
    /// Type mismatch or invalid conversion.
    TypeError,
    /// Variable not found or undefined.
    UndefinedVariable,
    /// Division by zero, overflow, etc.
    ArithmeticError,
    /// Error in a pipeline stage.
    PipelineError,
    /// User-defined error thrown with `throw`.
    UserError,
    /// An error from a plugin.
    PluginError,
    /// Catch-all for other errors.
    InternalError,
}

/// A single frame in the error's call stack.
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// The file where this frame originates (None for interactive input).
    pub file: Option<PathBuf>,
    /// The line number (1-indexed).
    pub line: usize,
    /// The column number (1-indexed), if known.
    pub column: Option<usize>,
    /// The function name, if inside a function.
    pub function: Option<String>,
    /// A snippet of the source code at this location.
    pub source_snippet: Option<String>,
}

/// The structured error type used throughout james-shell.
#[derive(Debug, Clone)]
pub struct ShellError {
    /// Human-readable error message.
    pub message: String,
    /// The error category for programmatic handling.
    pub category: ErrorCategory,
    /// The numeric exit code (0-255). Maps to process exit codes.
    pub code: i32,
    /// The call stack at the point of the error.
    pub stack: Vec<StackFrame>,
    /// An optional underlying cause (for chained errors).
    pub cause: Option<Box<ShellError>>,
    /// Optional structured metadata (e.g., the command that failed,
    /// the file that couldn't be opened, etc.)
    pub metadata: std::collections::HashMap<String, String>,
}

impl ShellError {
    /// Create a new error with just a message and category.
    pub fn new(message: impl Into<String>, category: ErrorCategory) -> Self {
        Self {
            message: message.into(),
            category,
            code: 1,
            stack: Vec::new(),
            cause: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Builder: set the exit code.
    pub fn with_code(mut self, code: i32) -> Self {
        self.code = code;
        self
    }

    /// Builder: add a stack frame.
    pub fn with_frame(mut self, frame: StackFrame) -> Self {
        self.stack.push(frame);
        self
    }

    /// Builder: set the cause.
    pub fn with_cause(mut self, cause: ShellError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    /// Builder: add metadata.
    pub fn with_meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Format the error for display, including the full stack trace.
    pub fn format_full(&self) -> String {
        let mut output = String::new();

        // Header
        output.push_str(&format!(
            "Error [{}]: {}\n",
            self.category_name(),
            self.message
        ));

        // Exit code
        output.push_str(&format!("  Exit code: {}\n", self.code));

        // Stack trace
        if !self.stack.is_empty() {
            output.push_str("  Stack trace:\n");
            for (i, frame) in self.stack.iter().enumerate() {
                let location = match (&frame.file, &frame.function) {
                    (Some(file), Some(func)) => {
                        format!("{}:{} in {}()", file.display(), frame.line, func)
                    }
                    (Some(file), None) => {
                        format!("{}:{}", file.display(), frame.line)
                    }
                    (None, Some(func)) => {
                        format!("<interactive>:{} in {}()", frame.line, func)
                    }
                    (None, None) => {
                        format!("<interactive>:{}", frame.line)
                    }
                };

                let prefix = if i == 0 { "    at" } else { "    from" };
                output.push_str(&format!("{} {}\n", prefix, location));

                // Show the source snippet if available
                if let Some(ref snippet) = frame.source_snippet {
                    output.push_str(&format!("       │ {}\n", snippet.trim()));
                }
            }
        }

        // Metadata
        for (key, value) in &self.metadata {
            output.push_str(&format!("  {}: {}\n", key, value));
        }

        // Cause chain
        if let Some(ref cause) = self.cause {
            output.push_str(&format!("  Caused by: {}\n", cause.message));
        }

        output
    }

    fn category_name(&self) -> &'static str {
        match self.category {
            ErrorCategory::CommandFailed => "CommandFailed",
            ErrorCategory::CommandNotFound => "CommandNotFound",
            ErrorCategory::IOError => "IOError",
            ErrorCategory::SyntaxError => "SyntaxError",
            ErrorCategory::TypeError => "TypeError",
            ErrorCategory::UndefinedVariable => "UndefinedVariable",
            ErrorCategory::ArithmeticError => "ArithmeticError",
            ErrorCategory::PipelineError => "PipelineError",
            ErrorCategory::UserError => "UserError",
            ErrorCategory::PluginError => "PluginError",
            ErrorCategory::InternalError => "InternalError",
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|c| c.as_ref() as &dyn std::error::Error)
    }
}
```

### Converting from standard Rust errors

We need to seamlessly convert `std::io::Error`, process exit codes, and other error types into `ShellError`:

```rust
impl From<std::io::Error> for ShellError {
    fn from(err: std::io::Error) -> Self {
        ShellError::new(err.to_string(), ErrorCategory::IOError)
            .with_code(1)
            .with_meta("io_kind", format!("{:?}", err.kind()))
    }
}

/// Convert a failed command execution into a ShellError.
pub fn command_failed_error(
    command: &str,
    exit_code: i32,
    stderr: Option<&str>,
) -> ShellError {
    let message = match stderr {
        Some(msg) if !msg.trim().is_empty() => {
            format!("command `{}` failed: {}", command, msg.trim())
        }
        _ => format!("command `{}` failed with exit code {}", command, exit_code),
    };

    ShellError::new(message, ErrorCategory::CommandFailed)
        .with_code(exit_code)
        .with_meta("command", command.to_string())
}

/// Convert a "command not found" into a ShellError.
pub fn command_not_found_error(command: &str) -> ShellError {
    ShellError::new(
        format!("command not found: {}", command),
        ErrorCategory::CommandNotFound,
    )
    .with_code(127) // standard "command not found" exit code
    .with_meta("command", command.to_string())
}
```

---

## Concept 3: The `$error` Variable

After every command, james-shell sets a special variable `$error` that holds a structured record of the last error (or is empty/null if the last command succeeded). This replaces bash's `$?` with something actually useful.

```
jsh> ls /nonexistent
Error: command `ls` failed with exit code 2
  Message: /nonexistent: No such file or directory

jsh> echo $error
{category: "CommandFailed", code: 2, message: "ls: /nonexistent: No such file or directory"}

jsh> echo $error.code
2

jsh> echo $error.message
ls: /nonexistent: No such file or directory

jsh> echo $error.category
CommandFailed

jsh> ls /tmp     # succeeds
Documents  Downloads  Music

jsh> echo $error
                   # empty — last command succeeded
```

### Implementation in Rust

The shell interpreter maintains the `$error` binding:

```rust
use std::collections::HashMap;

/// Represents a structured value in the shell.
/// (This ties into Module 14's Value type.)
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Record(HashMap<String, Value>),
    List(Vec<Value>),
    Error(Box<ShellError>),
}

/// The shell's environment, tracking variables and the last error.
pub struct ShellState {
    pub variables: HashMap<String, Value>,
    pub last_error: Option<ShellError>,
    pub strict_mode: bool,   // default: true
}

impl ShellState {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            last_error: None,
            strict_mode: true,
        }
    }

    /// Called after every command execution.
    pub fn set_command_result(&mut self, result: Result<Value, ShellError>) {
        match result {
            Ok(_) => {
                self.last_error = None;
                // $error becomes null
                self.variables.insert("error".to_string(), Value::Null);
                // $? equivalent — still available for compatibility
                self.variables.insert("status".to_string(), Value::Int(0));
            }
            Err(err) => {
                // Populate $error as a structured record
                let error_record = self.error_to_record(&err);
                self.variables.insert("error".to_string(), error_record);
                self.variables.insert(
                    "status".to_string(),
                    Value::Int(err.code as i64),
                );
                self.last_error = Some(err);
            }
        }
    }

    /// Convert a ShellError into a Value::Record for use in scripts.
    fn error_to_record(&self, err: &ShellError) -> Value {
        let mut record = HashMap::new();
        record.insert(
            "message".to_string(),
            Value::String(err.message.clone()),
        );
        record.insert(
            "category".to_string(),
            Value::String(err.category_name().to_string()),
        );
        record.insert(
            "code".to_string(),
            Value::Int(err.code as i64),
        );

        // Stack trace as a list of records
        let stack: Vec<Value> = err.stack.iter().map(|frame| {
            let mut f = HashMap::new();
            f.insert("line".to_string(), Value::Int(frame.line as i64));
            if let Some(ref file) = frame.file {
                f.insert("file".to_string(), Value::String(file.display().to_string()));
            }
            if let Some(ref func) = frame.function {
                f.insert("function".to_string(), Value::String(func.clone()));
            }
            Value::Record(f)
        }).collect();
        record.insert("stack".to_string(), Value::List(stack));

        // Metadata
        let meta: HashMap<String, Value> = err.metadata.iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        if !meta.is_empty() {
            record.insert("metadata".to_string(), Value::Record(meta));
        }

        Value::Record(record)
    }
}
```

---

## Concept 4: Try/Catch Blocks

The centerpiece of our error handling: structured try/catch. This is the feature that makes james-shell scripts *trustworthy*.

### Syntax

```
# Basic try/catch
try {
    cd /deployments/staging
    rm -rf old_release/
    mv new_release/ current/
} catch {
    echo "Deployment failed: $error.message"
    echo "Rolling back..."
    rollback
}

# Catch with a named error variable
try {
    curl -f https://api.example.com/health
} catch err {
    if $err.category == "CommandFailed" {
        echo "API is down (exit code $err.code)"
    } else {
        echo "Unexpected error: $err.message"
    }
}

# Try/catch/finally
try {
    acquire_lock "deploy"
    do_deployment
} catch {
    echo "Deployment failed"
    notify_team "Deploy failed: $error.message"
} finally {
    release_lock "deploy"   # always runs, even if catch throws
}
```

### How it works — the execution model

```
Normal execution (strict mode):

    cmd1          ← succeeds, continue
    cmd2          ← FAILS → execution stops, error printed
    cmd3          ← never reached

With try/catch:

    try {
        cmd1      ← succeeds, continue
        cmd2      ← FAILS → jump to catch block
        cmd3      ← never reached
    } catch {
        handler   ← runs with $error set to cmd2's error
    }
    cmd4          ← continues normally after catch
```

### AST representation

```rust
/// AST node for try/catch/finally.
#[derive(Debug, Clone)]
pub struct TryCatch {
    /// The block to attempt.
    pub try_block: Block,
    /// Optional variable name to bind the error to in the catch block.
    /// If None, the error is available via $error.
    pub catch_binding: Option<String>,
    /// The block to run if an error occurs.
    pub catch_block: Option<Block>,
    /// The block to run regardless of success or failure.
    pub finally_block: Option<Block>,
}

/// A block is a list of statements.
#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Statement>,
}

/// Part of the larger Statement enum.
#[derive(Debug, Clone)]
pub enum Statement {
    Command(CommandNode),
    Pipeline(PipelineNode),
    If(IfNode),
    For(ForNode),
    While(WhileNode),
    TryCatch(TryCatch),
    FunctionDef(FunctionDef),
    Assignment(Assignment),
    Return(Option<Expression>),
    Throw(Expression),          // explicitly throw an error
    // ... other statement types
}
```

### Interpreter logic

```rust
impl Interpreter {
    fn execute_try_catch(&mut self, node: &TryCatch) -> Result<Value, ShellError> {
        // Execute the try block
        let try_result = self.execute_block(&node.try_block);

        match try_result {
            Ok(value) => {
                // Try block succeeded — run finally if present, return value
                if let Some(ref finally) = node.finally_block {
                    // finally block errors are NOT caught
                    self.execute_block(finally)?;
                }
                Ok(value)
            }
            Err(error) => {
                // Try block failed — run catch block if present
                if let Some(ref catch_block) = node.catch_block {
                    // Bind the error to the catch variable if specified
                    if let Some(ref var_name) = node.catch_binding {
                        let error_record = self.state.error_to_record(&error);
                        self.state.variables.insert(
                            var_name.clone(),
                            error_record,
                        );
                    }

                    // Always set $error for the catch block
                    self.state.set_command_result(Err(error.clone()));

                    // Execute the catch block
                    let catch_result = self.execute_block(catch_block);

                    // Run finally if present
                    if let Some(ref finally) = node.finally_block {
                        self.execute_block(finally)?;
                    }

                    // Clean up the catch binding
                    if let Some(ref var_name) = node.catch_binding {
                        self.state.variables.remove(var_name);
                    }

                    // If catch itself threw, propagate that error
                    catch_result
                } else {
                    // No catch block — run finally then propagate the error
                    if let Some(ref finally) = node.finally_block {
                        // Even if finally fails, we propagate the original error
                        let _ = self.execute_block(finally);
                    }
                    Err(error)
                }
            }
        }
    }
}
```

### The `throw` statement

Users can explicitly throw errors in their scripts:

```
# Throw a string (becomes a UserError)
throw "invalid configuration: missing API_KEY"

# Throw a structured error
throw {
    message: "validation failed",
    category: "UserError",
    code: 2,
    metadata: { field: "email", reason: "invalid format" }
}
```

```rust
impl Interpreter {
    fn execute_throw(&mut self, expr: &Expression) -> Result<Value, ShellError> {
        let value = self.evaluate(expr)?;

        let error = match value {
            Value::String(msg) => {
                ShellError::new(msg, ErrorCategory::UserError)
            }
            Value::Record(map) => {
                let message = match map.get("message") {
                    Some(Value::String(s)) => s.clone(),
                    _ => "user error".to_string(),
                };

                let category = match map.get("category") {
                    Some(Value::String(s)) => string_to_category(s),
                    _ => ErrorCategory::UserError,
                };

                let code = match map.get("code") {
                    Some(Value::Int(n)) => *n as i32,
                    _ => 1,
                };

                ShellError::new(message, category).with_code(code)
            }
            other => {
                ShellError::new(
                    format!("{:?}", other),
                    ErrorCategory::UserError,
                )
            }
        };

        // Add the current location to the stack trace
        let error = error.with_frame(self.current_stack_frame());

        Err(error)
    }
}
```

---

## Concept 5: Strict Mode — Errors Stop Execution by Default

In james-shell, strict mode is **on by default**. This is the opposite of bash, and it's a deliberate choice: failing silently is almost never what you want.

### What strict mode means

```
# In strict mode (default), this stops at the failing command:
cd /nonexistent        # ERROR — execution stops here
echo "unreachable"     # never runs

# To run a command that might fail without stopping:
# Option 1: try/catch
try {
    cd /nonexistent
} catch {
    echo "directory not found, using fallback"
    cd /tmp
}

# Option 2: the ? operator (see Concept 6)
let result = (cd /nonexistent)?

# Option 3: explicit || handling (like bash, still works)
cd /nonexistent || echo "fallback"

# Option 4: disable strict mode for a block
lenient {
    cd /nonexistent       # fails silently
    echo "this runs"      # still runs
}
```

### Implementation

```rust
impl Interpreter {
    /// Execute a single statement. In strict mode, errors propagate.
    fn execute_statement(&mut self, stmt: &Statement) -> Result<Value, ShellError> {
        let result = match stmt {
            Statement::Command(cmd) => self.execute_command(cmd),
            Statement::Pipeline(pipe) => self.execute_pipeline(pipe),
            Statement::If(if_node) => self.execute_if(if_node),
            Statement::For(for_node) => self.execute_for(for_node),
            Statement::TryCatch(tc) => self.execute_try_catch(tc),
            Statement::Throw(expr) => self.execute_throw(expr),
            // ... other statement types
        };

        // Update $error and $status
        self.state.set_command_result(result.clone());

        match result {
            Ok(value) => Ok(value),
            Err(ref error) => {
                if self.state.strict_mode {
                    // In strict mode, propagate the error up
                    Err(error.clone())
                } else {
                    // In lenient mode, print a warning and continue
                    eprintln!(
                        "Warning: {} (error suppressed in lenient mode)",
                        error.message
                    );
                    Ok(Value::Null)
                }
            }
        }
    }

    /// Execute a block of statements sequentially.
    fn execute_block(&mut self, block: &Block) -> Result<Value, ShellError> {
        let mut last_value = Value::Null;

        for stmt in &block.statements {
            last_value = self.execute_statement(stmt)?;
            // The ? above means: if strict mode caused an error to propagate,
            // we stop executing the rest of the block.
        }

        Ok(last_value)
    }
}
```

### The `lenient` block

Sometimes you genuinely want bash-like behavior — maybe you're running a series of cleanup commands where some might fail. The `lenient` block temporarily disables strict mode:

```rust
impl Interpreter {
    fn execute_lenient_block(&mut self, block: &Block) -> Result<Value, ShellError> {
        let was_strict = self.state.strict_mode;
        self.state.strict_mode = false;
        let result = self.execute_block(block);
        self.state.strict_mode = was_strict;
        result
    }
}
```

---

## Concept 6: The `?` Operator — Propagate Errors Cleanly

Inspired directly by Rust's own `?` operator, this lets scripts propagate errors up the call stack without verbose try/catch blocks.

### In Rust, `?` works like this:

```rust
fn read_config() -> Result<Config, Error> {
    let content = std::fs::read_to_string("config.toml")?;  // returns early on error
    let config = parse_toml(&content)?;                       // returns early on error
    Ok(config)
}
```

### In james-shell, `?` works the same way:

```
def deploy(env: string) {
    let config = (load_config "deploy.toml")?
    let server = $config.servers.$env?           # propagate if key missing

    (ssh $server "systemctl stop app")?
    (scp ./build/ $server:/opt/app/)?
    (ssh $server "systemctl start app")?

    echo "Deployed to $env successfully"
}

# Caller can catch the propagated error
try {
    deploy "staging"
} catch err {
    echo "Deployment failed: $err.message"
    echo "At: $err.stack"
}
```

### Without `?`, you'd need verbose error checking

Compare:

```
# With ? operator — clean and readable
def build_and_deploy() {
    (cargo build --release)?
    (cargo test)?
    (deploy "staging")?
    echo "Done!"
}

# Without ? — verbose and noisy (bash-style)
def build_and_deploy() {
    cargo build --release
    if $status != 0 {
        return $error
    }
    cargo test
    if $status != 0 {
        return $error
    }
    deploy "staging"
    if $status != 0 {
        return $error
    }
    echo "Done!"
}
```

### Implementation

The `?` operator is syntactic sugar in the parser. When the parser sees `(expression)?`, it desugars it:

```rust
/// In the parser, `(expr)?` becomes:
///     match eval(expr) {
///         Ok(val) => val,
///         Err(e) => return Err(e.with_frame(current_location))
///     }

#[derive(Debug, Clone)]
pub enum Expression {
    // ... other expression types ...
    /// The `?` operator — propagate error or unwrap value.
    TryPropagate(Box<Expression>),
}

impl Interpreter {
    fn evaluate(&mut self, expr: &Expression) -> Result<Value, ShellError> {
        match expr {
            Expression::TryPropagate(inner) => {
                match self.evaluate(inner) {
                    Ok(value) => Ok(value),
                    Err(mut error) => {
                        // Add the current location to the stack trace
                        error.stack.push(self.current_stack_frame());
                        Err(error)
                    }
                }
            }
            // ... handle other expressions
        }
    }

    /// Get the current file/line/function for stack traces.
    fn current_stack_frame(&self) -> StackFrame {
        StackFrame {
            file: self.current_file.clone(),
            line: self.current_line,
            column: Some(self.current_column),
            function: self.current_function.clone(),
            source_snippet: self.get_source_line(self.current_line),
        }
    }
}
```

---

## Concept 7: Rich Error Context and Stack Traces

The real power of structured errors is the *context* they carry. When something goes wrong three function calls deep, you want to see the entire path.

### Building the stack trace

Every time an error crosses a function boundary (either via `?`, via strict-mode propagation, or via re-throw in a catch block), we push a new frame:

```rust
impl Interpreter {
    /// Execute a function call, adding a stack frame on error.
    fn call_function(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Result<Value, ShellError> {
        let func = self.lookup_function(name)?;

        // Set up the function's scope
        self.push_scope();
        self.bind_parameters(&func.params, args)?;

        let saved_function = self.current_function.clone();
        self.current_function = Some(name.to_string());

        let result = self.execute_block(&func.body);

        self.current_function = saved_function;
        self.pop_scope();

        // If the function failed, add this call site to the stack trace
        result.map_err(|mut err| {
            err.stack.push(self.current_stack_frame());
            err
        })
    }
}
```

### Example error output

Here's what a real error looks like with full context. Suppose this script:

```
# file: deploy.jsh
def check_health(url: string) {
    curl -sf $url                         # line 3
}

def deploy(env: string) {
    let config = (load_config "deploy.toml")?     # line 7
    let url = $"https://($config.host)/health"
    (check_health $url)?                          # line 9
    echo "Deploy complete"
}
```

And the user runs:

```
jsh> source deploy.jsh
jsh> deploy "production"
```

If `curl` fails because the server is down:

```
Error [CommandFailed]: command `curl` failed with exit code 7
  Exit code: 7
  Stack trace:
    at deploy.jsh:3 in check_health()
       │ curl -sf $url
    from deploy.jsh:9 in deploy()
       │ (check_health $url)?
    from <interactive>:1
       │ deploy "production"
  command: curl
  Suggestion: Exit code 7 means "connection refused". Is the server running?
```

### Smart error suggestions

We can add contextual suggestions based on common error patterns:

```rust
fn suggest_fix(error: &ShellError) -> Option<String> {
    // Suggestions based on error category and code
    match (&error.category, error.code) {
        (ErrorCategory::CommandNotFound, 127) => {
            let cmd = error.metadata.get("command")?;
            // Check for common typos
            let suggestion = find_similar_command(cmd)?;
            Some(format!("Did you mean `{}`?", suggestion))
        }

        (ErrorCategory::CommandFailed, 7) => {
            if error.metadata.get("command").map(|c| c.as_str()) == Some("curl") {
                Some("Exit code 7 means 'connection refused'. Is the server running?".into())
            } else {
                None
            }
        }

        (ErrorCategory::IOError, _) => {
            if error.message.contains("Permission denied") {
                Some("Try running with elevated privileges (sudo/admin).".into())
            } else if error.message.contains("No such file") {
                Some("Check that the path exists and is spelled correctly.".into())
            } else {
                None
            }
        }

        (ErrorCategory::CommandFailed, 128..=192) => {
            let signal = error.code - 128;
            let signal_name = match signal {
                2 => "SIGINT (Ctrl-C)",
                6 => "SIGABRT (abort)",
                9 => "SIGKILL (killed)",
                11 => "SIGSEGV (segfault)",
                15 => "SIGTERM (terminated)",
                _ => "unknown signal",
            };
            Some(format!(
                "Process was killed by signal {} ({}).",
                signal, signal_name
            ))
        }

        _ => None,
    }
}
```

---

## Concept 8: Warnings vs Errors

Not everything is fatal. Sometimes you want to alert the user without stopping execution. james-shell distinguishes between **errors** (stop execution in strict mode) and **warnings** (always print, never stop).

```
# Emit a warning
warn "Config file not found, using defaults"

# Warnings appear in yellow with a prefix
# ⚠ Warning: Config file not found, using defaults

# Warnings are collected and can be reviewed
echo $warnings          # list of warning records from this session
echo $warnings | length # how many warnings
```

### Implementation

```rust
#[derive(Debug, Clone)]
pub struct ShellWarning {
    pub message: String,
    pub location: StackFrame,
}

impl ShellState {
    pub fn emit_warning(&mut self, message: String, location: StackFrame) {
        let warning = ShellWarning {
            message: message.clone(),
            location,
        };
        self.warnings.push(warning);

        // Print immediately in yellow
        eprintln!(
            "\x1b[33mWarning\x1b[0m: {}",
            message
        );
    }
}
```

### Comparison: errors vs warnings

| Aspect | Error | Warning |
|--------|-------|---------|
| Stops execution? | Yes (in strict mode) | Never |
| Catchable? | Yes (try/catch) | No |
| Sets `$error`? | Yes | No |
| Accessible later? | Via `$error` | Via `$warnings` list |
| Color | Red | Yellow |
| Use case | Things that broke | Things that smell wrong |

---

## Concept 9: Comparison with Bash Error Handling

Let's see the same tasks in bash vs james-shell, side by side.

### Task: Deploy with rollback on failure

**Bash:**

```bash
#!/bin/bash
set -e

deploy() {
    local env=$1

    # Problem: set -e doesn't work reliably in functions called
    # from conditional contexts. Also, cleanup on failure is manual.

    cd "/opt/$env" || { echo "Failed to cd"; return 1; }

    cp -r current/ rollback/ || { echo "Failed to backup"; return 1; }

    tar xzf /tmp/release.tar.gz -C current/ || {
        echo "Failed to extract — rolling back"
        rm -rf current/
        mv rollback/ current/
        return 1
    }

    systemctl restart app || {
        echo "Failed to restart — rolling back"
        rm -rf current/
        mv rollback/ current/
        systemctl restart app  # what if THIS fails?
        return 1
    }

    rm -rf rollback/
    echo "Deployed successfully"
}

# set -e is disabled inside this condition, silently!
if deploy "production"; then
    echo "Success"
else
    echo "Failed with code $?"
    # Good luck figuring out WHERE it failed
fi
```

**james-shell:**

```
#!/usr/bin/env jsh

def deploy(env: string) {
    cd $"/opt/($env)"

    cp -r current/ rollback/

    try {
        tar xzf /tmp/release.tar.gz -C current/
        systemctl restart app
    } catch err {
        warn "Deploy failed: $err.message — rolling back"
        rm -rf current/
        mv rollback/ current/
        (systemctl restart app)?    # propagate if rollback fails too
        throw $err                  # re-throw the original error
    }

    rm -rf rollback/
    echo "Deployed to $env successfully"
}

try {
    deploy "production"
} catch err {
    echo "Deployment failed:"
    echo "  $err.message"
    echo "  at $err.stack"
    exit $err.code
}
```

The james-shell version is:
- **Shorter** — no repetitive `|| { ...; return 1; }` on every line
- **Clearer** — the try/catch structure makes the intent obvious
- **Safer** — strict mode means we don't accidentally skip a failing command
- **More informative** — the error object tells us exactly what happened and where

---

## Key Rust Concepts Used

| Concept | Where it appears |
|---------|-----------------|
| **Custom error types** | `ShellError` with `Display` and `std::error::Error` impls |
| **Builder pattern** | `ShellError::new().with_code().with_frame().with_meta()` |
| **Enum variants** | `ErrorCategory` for classifying errors |
| **`From` trait conversions** | `impl From<std::io::Error> for ShellError` |
| **`Result<T, E>` propagation** | The `?` operator in both Rust code and shell scripts |
| **Box for recursive types** | `cause: Option<Box<ShellError>>` |
| **HashMap for metadata** | Structured key-value metadata on errors |
| **Pattern matching** | Dispatch on error categories for suggestions |
| **Trait objects** | `dyn std::error::Error` for the error source chain |

---

## Milestone

After completing this module, your shell should behave like this:

```
jsh> ls /nonexistent
Error [CommandFailed]: ls: /nonexistent: No such file or directory
  Exit code: 2

jsh> echo $error.category
CommandFailed

jsh> echo $error.code
2

jsh> try { ls /nonexistent } catch { echo "caught it!" }
caught it!

jsh> try { ls /nonexistent } catch err { echo $err.message }
ls: /nonexistent: No such file or directory

jsh> def risky() { throw "something went wrong" }
jsh> risky
Error [UserError]: something went wrong
  Exit code: 1
  Stack trace:
    at <interactive>:1 in risky()

jsh> try { risky } catch err { echo "Error in $err.stack.0.function: $err.message" }
Error in risky: something went wrong

jsh> lss
Error [CommandNotFound]: command not found: lss
  Suggestion: Did you mean `ls`?

jsh> # Strict mode: errors stop execution
jsh> { ls /nonexistent; echo "unreachable" }
Error [CommandFailed]: ls: /nonexistent: No such file or directory
  Exit code: 2

jsh> # Lenient mode: errors are warnings
jsh> lenient { ls /nonexistent; echo "still runs" }
Warning: ls: /nonexistent: No such file or directory (error suppressed)
still runs
```

---

## What's next?

We now have errors that are structured, catchable, and informative. But our scripting language itself is still limited — bash-style syntax with its arcane quoting rules and lack of real data types. In **Module 19: Modern Scripting Language**, we give james-shell named function parameters, type annotations, closures, pattern matching, and string interpolation — making it a scripting language you actually *want* to write code in.
