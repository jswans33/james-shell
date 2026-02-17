# Module 21: Diagnostics & Logging

## What are we building?

A shell is notoriously hard to debug. When a script misbehaves, the user's only
tool in most shells is `set -x`, which dumps every command to stderr with zero
filtering or structure. That is fine for a five-line script but useless for
anything larger.

In this module we add a **feature-flagged diagnostics system** to james-shell
that gives developers and advanced users deep visibility into the shell's
internals:

1. **Log levels** -- ERROR, WARN, INFO, DEBUG, TRACE with runtime selection.
2. **Compile-time feature gates** -- zero-cost when diagnostics are compiled
   out.
3. **Subsystem spans** -- scoped logs for the lexer, parser, expander,
   executor, and builtins.
4. **Structured events** -- key-value pairs instead of ad-hoc print statements.
5. **Command audit trail** -- persistent, timestamped record of every command
   executed.
6. **Session logging** -- full capture of a shell session to a file.
7. **Log targets** -- stderr, file, or both, with rotation support.
8. **Integration with `set -x`** -- xtrace becomes one layer of a richer
   system.

The entire system is built on the Rust `tracing` crate ecosystem, which gives
us compile-time elision, zero-allocation fast paths, and composable
subscribers.

---

## Concept 1: Log Levels and When to Use Them

### The Five Levels

| Level | Purpose | Example |
|-------|---------|---------|
| `ERROR` | Something failed and the shell cannot recover normally. | `jsh: exec failed: No such file` |
| `WARN` | Something unexpected but recoverable. | `jsh: alias expansion depth limit reached` |
| `INFO` | High-level lifecycle events. | `session started`, `script loaded`, `job completed` |
| `DEBUG` | Detailed internal decisions. | `expanding variable $PATH`, `matched glob pattern *.rs` |
| `TRACE` | Extremely fine-grained step-by-step traces. | `lexer consumed byte 0x22 at pos 14`, `parser entering parse_pipeline` |

### Design Principle

In normal operation (no flags, no env vars), james-shell produces **zero**
diagnostic output. Users opt in explicitly. Even when compiled with
diagnostics, the hot path pays nothing unless a subscriber is active -- this is
a guarantee the `tracing` crate provides via its `callsite` mechanism.

---

## Concept 2: Feature Flags

### Cargo Feature Gates

We define two Cargo features that control diagnostics at compile time:

```toml
# Cargo.toml
[features]
default = []

# Include diagnostic instrumentation (tracing spans and events).
# When disabled, all tracing macros compile to nothing.
diagnostics = ["dep:tracing", "dep:tracing-subscriber"]

# Include the command audit trail subsystem.
# Requires `diagnostics`.
audit = ["diagnostics", "dep:chrono"]
```

### What Each Feature Enables

| Feature | What it adds | Binary size cost |
|---------|-------------|-----------------|
| *(none)* | No tracing code emitted at all. | Baseline |
| `diagnostics` | Spans, events, subscriber setup, `--log-level` flag. | ~200 KB |
| `audit` | Timestamped command log, `history --audit` builtin. | ~50 KB on top of `diagnostics` |

### Conditional Compilation Pattern

Throughout the codebase, we wrap instrumentation in feature gates:

```rust
#[cfg(feature = "diagnostics")]
use tracing::{debug, error, info, trace, warn, instrument, span, Level};

// For modules that just need events without spans:
#[cfg(feature = "diagnostics")]
use tracing::{debug, trace};
```

When `diagnostics` is not enabled, we provide no-op stubs so that call sites
do not need `#[cfg]` on every line:

```rust
// src/diagnostics/mod.rs

/// When the `diagnostics` feature is disabled, these macros expand to nothing.
#[cfg(not(feature = "diagnostics"))]
macro_rules! jsh_trace { ($($arg:tt)*) => {} }
#[cfg(not(feature = "diagnostics"))]
macro_rules! jsh_debug { ($($arg:tt)*) => {} }
#[cfg(not(feature = "diagnostics"))]
macro_rules! jsh_info  { ($($arg:tt)*) => {} }
#[cfg(not(feature = "diagnostics"))]
macro_rules! jsh_warn  { ($($arg:tt)*) => {} }
#[cfg(not(feature = "diagnostics"))]
macro_rules! jsh_error { ($($arg:tt)*) => {} }

/// When the `diagnostics` feature is enabled, these delegate to `tracing`.
#[cfg(feature = "diagnostics")]
macro_rules! jsh_trace { ($($arg:tt)*) => { tracing::trace!($($arg)*) } }
#[cfg(feature = "diagnostics")]
macro_rules! jsh_debug { ($($arg:tt)*) => { tracing::debug!($($arg)*) } }
#[cfg(feature = "diagnostics")]
macro_rules! jsh_info  { ($($arg:tt)*) => { tracing::info!($($arg)*) } }
#[cfg(feature = "diagnostics")]
macro_rules! jsh_warn  { ($($arg:tt)*) => { tracing::warn!($($arg)*) } }
#[cfg(feature = "diagnostics")]
macro_rules! jsh_error { ($($arg:tt)*) => { tracing::error!($($arg)*) } }
```

This means you can write:

```rust
jsh_debug!(variable = %name, value = %val, "expanded variable");
```

and it compiles to **nothing** in a release build without the feature flag.

---

## Concept 3: Runtime Configuration

### Environment Variable: `JSH_LOG`

The primary way to control log verbosity at runtime:

```
$ JSH_LOG=debug jsh
$ JSH_LOG=trace jsh ./script.jsh
$ JSH_LOG=warn jsh           # only warnings and errors
```

### CLI Flags

```
$ jsh --log-level debug
$ jsh --log-level trace --log-file /tmp/jsh.log
$ jsh -v                     # shorthand for --log-level info
$ jsh -vv                    # shorthand for --log-level debug
$ jsh -vvv                   # shorthand for --log-level trace
```

### Per-Subsystem Filtering

The `tracing-subscriber` crate's `EnvFilter` supports per-target filtering
via `JSH_LOG`:

```
$ JSH_LOG=jsh::parser=trace,jsh::executor=debug jsh
```

This is invaluable for debugging a specific subsystem without drowning in
output from every other part of the shell.

### Implementation: Subscriber Setup

```rust
#[cfg(feature = "diagnostics")]
pub fn init_diagnostics(opts: &CliOptions) {
    use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

    // Determine the log level from (in priority order):
    // 1. --log-level CLI flag
    // 2. JSH_LOG environment variable
    // 3. Default: off (no output)
    let filter = if let Some(ref level) = opts.log_level {
        EnvFilter::new(level)
    } else if let Ok(env_val) = std::env::var("JSH_LOG") {
        EnvFilter::new(env_val)
    } else {
        EnvFilter::new("off")
    };

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_thread_ids(false)
        .with_ansi(atty::is(atty::Stream::Stderr));

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer);

    // Optionally add a file layer.
    if let Some(ref log_path) = opts.log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("failed to open log file");

        let file_layer = fmt::layer()
            .with_writer(file)
            .with_ansi(false)
            .json();

        subscriber.with(file_layer).init();
    } else {
        subscriber.init();
    }
}

#[cfg(not(feature = "diagnostics"))]
pub fn init_diagnostics(_opts: &CliOptions) {
    // No-op when diagnostics are compiled out.
}
```

### CLI Options Struct

```rust
pub struct CliOptions {
    pub script_file: Option<String>,
    pub command_string: Option<String>,
    pub interactive: bool,

    // Diagnostics (only meaningful with `diagnostics` feature).
    pub log_level: Option<String>,
    pub log_file: Option<String>,
    pub verbose_count: u8,  // -v, -vv, -vvv
}

impl CliOptions {
    pub fn parse(args: &[String]) -> Self {
        let mut opts = CliOptions {
            script_file: None,
            command_string: None,
            interactive: true,
            log_level: None,
            log_file: None,
            verbose_count: 0,
        };

        let mut i = 1; // skip argv[0]
        while i < args.len() {
            match args[i].as_str() {
                "-c" => {
                    i += 1;
                    opts.command_string = args.get(i).cloned();
                    opts.interactive = false;
                }
                "--log-level" => {
                    i += 1;
                    opts.log_level = args.get(i).cloned();
                }
                "--log-file" => {
                    i += 1;
                    opts.log_file = args.get(i).cloned();
                }
                arg if arg.starts_with("-v") && arg.chars().skip(1).all(|c| c == 'v') => {
                    opts.verbose_count = (arg.len() - 1) as u8;
                    opts.log_level = Some(match opts.verbose_count {
                        1 => "info".to_string(),
                        2 => "debug".to_string(),
                        _ => "trace".to_string(),
                    });
                }
                arg if !arg.starts_with('-') && opts.script_file.is_none() => {
                    opts.script_file = Some(arg.to_string());
                    opts.interactive = false;
                }
                _ => {}
            }
            i += 1;
        }

        opts
    }
}
```

---

## Concept 4: Subsystem Spans

### What Are Spans?

A span represents a **period of time** during which something is happening.
Unlike point-in-time log events, spans have a beginning and an end. They nest
naturally, forming a tree that mirrors the shell's execution flow.

```
[REPL iter #42] ─┐
  [lex] ─┐       │
         └─      │
  [parse] ─┐     │
           └─    │
  [expand] ─┐    │
            └─   │
  [execute] ─┐   │
             └─  │
                 └─
```

### Instrumenting the Core Pipeline

Each major phase of command processing gets its own span:

```rust
use tracing::instrument;

/// Lex a raw input line into tokens.
#[cfg_attr(feature = "diagnostics", instrument(level = "debug", skip(input), fields(input_len = input.len())))]
pub fn lex(input: &str) -> Vec<Token> {
    jsh_trace!(input = %input, "lexing input");
    let mut tokens = Vec::new();
    let mut lexer = Lexer::new(input);

    while let Some(token) = lexer.next_token() {
        jsh_trace!(?token, pos = lexer.pos, "produced token");
        tokens.push(token);
    }

    jsh_debug!(token_count = tokens.len(), "lexing complete");
    tokens
}

/// Parse a token stream into an AST.
#[cfg_attr(feature = "diagnostics", instrument(level = "debug", skip(tokens), fields(token_count = tokens.len())))]
pub fn parse(tokens: &[Token]) -> Result<Vec<AstNode>, ParseError> {
    let mut parser = Parser::new(tokens);
    let ast = parser.parse_program()?;
    jsh_debug!(node_count = ast.len(), "parsing complete");
    Ok(ast)
}

/// Expand words (variables, globs, tildes, command substitution).
#[cfg_attr(feature = "diagnostics", instrument(level = "debug", skip(env)))]
pub fn expand_words(words: &[String], env: &ShellEnv) -> Vec<String> {
    words.iter().map(|w| {
        let expanded = expand_word(w, env);
        if &expanded != w {
            jsh_debug!(original = %w, expanded = %expanded, "word expanded");
        }
        expanded
    }).collect()
}

/// Execute an AST node.
#[cfg_attr(feature = "diagnostics", instrument(level = "debug", skip(env), fields(node_type = %node.type_name())))]
pub fn eval_node(node: &AstNode, env: &mut ShellEnv) -> i32 {
    let status = match node {
        AstNode::SimpleCommand(cmd) => {
            jsh_debug!(argv = ?cmd.argv, "executing simple command");
            execute_simple_command(cmd, env)
        }
        AstNode::Pipeline(cmds) => {
            jsh_debug!(stage_count = cmds.len(), "executing pipeline");
            execute_pipeline(cmds, env)
        }
        AstNode::If { condition, then_branch, else_branch } => {
            jsh_debug!("evaluating if-condition");
            eval_if(condition, then_branch, else_branch.as_deref(), env)
        }
        // ... other node types
        _ => { 0 }
    };

    jsh_debug!(exit_code = status, "node evaluation complete");
    status
}
```

### REPL Span

Each iteration of the REPL gets a top-level span so all events from a single
command are grouped:

```rust
pub fn run_repl(env: &mut ShellEnv) {
    let mut line_number: u64 = 0;

    loop {
        line_number += 1;
        let prompt = render_prompt(&env.prompt_template, env);
        print!("{}", prompt);
        flush_stdout();

        let input = match read_line() {
            Some(line) => line,
            None => break, // EOF
        };

        if input.trim().is_empty() {
            continue;
        }

        // Each REPL iteration gets its own span.
        #[cfg(feature = "diagnostics")]
        let _repl_span = tracing::info_span!(
            "repl",
            line = line_number,
            input = %input.trim(),
        ).entered();

        jsh_info!(line = line_number, "processing command");

        let tokens = lex(&input);
        let ast = match parse(&tokens) {
            Ok(ast) => ast,
            Err(e) => {
                jsh_warn!(error = %e, "parse error");
                eprintln!("jsh: {}", e);
                continue;
            }
        };

        let status = eval_nodes(&ast, env);
        jsh_info!(line = line_number, exit_code = status, "command complete");
    }
}
```

### Example Output

With `JSH_LOG=debug`:

```
2026-02-16T10:32:01.234Z DEBUG jsh::repl: processing command line=1
2026-02-16T10:32:01.234Z DEBUG jsh::lexer: lexing input input_len=18
2026-02-16T10:32:01.234Z TRACE jsh::lexer: produced token token=Word("ls") pos=2
2026-02-16T10:32:01.234Z TRACE jsh::lexer: produced token token=Pipe pos=4
2026-02-16T10:32:01.234Z TRACE jsh::lexer: produced token token=Word("grep") pos=9
2026-02-16T10:32:01.234Z TRACE jsh::lexer: produced token token=Word("foo") pos=13
2026-02-16T10:32:01.235Z DEBUG jsh::lexer: lexing complete token_count=4
2026-02-16T10:32:01.235Z DEBUG jsh::parser: parsing complete node_count=1
2026-02-16T10:32:01.235Z DEBUG jsh::executor: executing pipeline stage_count=2
2026-02-16T10:32:01.240Z DEBUG jsh::executor: node evaluation complete exit_code=0
2026-02-16T10:32:01.240Z  INFO jsh::repl: command complete line=1 exit_code=0
```

---

## Concept 5: Structured Events

### Beyond Printf Debugging

Traditional shells scatter `eprintln!("DEBUG: got here")` calls through their
code. This has problems:

- No filtering (all or nothing).
- No structured data (you parse strings by eye).
- Cannot be redirected to a file separately from command stderr.
- Cannot be disabled without editing source.

With `tracing`, every event carries **structured fields**:

```rust
// Bad: ad-hoc string formatting
eprintln!("DEBUG: expanding {} -> {}", name, value);

// Good: structured event with typed fields
jsh_debug!(
    variable = %name,
    value = %value,
    scope = %env.current_scope_name(),
    "variable expanded"
);
```

### Key Events to Instrument

Here is a checklist of the most valuable events to instrument across the shell:

**Lexer:**
```rust
jsh_trace!(byte = ch, pos = self.pos, "consumed byte");
jsh_trace!(?token, "produced token");
jsh_debug!(token_count = tokens.len(), "lexing complete");
```

**Parser:**
```rust
jsh_trace!(rule = "parse_pipeline", "entering parser rule");
jsh_debug!(?node, "parsed AST node");
jsh_warn!(error = %e, token = ?current, "parse error");
```

**Expander:**
```rust
jsh_debug!(variable = %name, value = %val, "variable expansion");
jsh_debug!(pattern = %glob, matches = ?results, "glob expansion");
jsh_debug!(original = %word, result = %expanded, "tilde expansion");
jsh_trace!(input = %cmd, output_len = out.len(), "command substitution");
```

**Executor:**
```rust
jsh_info!(command = %argv[0], pid = %child.id(), "spawned process");
jsh_debug!(command = %argv[0], exit_code = status, "process exited");
jsh_debug!(builtin = %name, exit_code = status, "builtin completed");
jsh_info!(job_id = id, pid = %pid, command = %cmd, "job backgrounded");
jsh_info!(job_id = id, exit_code = status, "job completed");
```

**Redirections:**
```rust
jsh_debug!(fd = src_fd, target = %path, mode = "write", "redirect applied");
jsh_debug!(fd = src_fd, dup_from = dest_fd, "fd duplication");
jsh_trace!(fd = src_fd, "redirect restored");
```

**Signals:**
```rust
jsh_info!(signal = "SIGINT", pid = %pid, "signal received");
jsh_debug!(signal = "SIGCHLD", pid = %pid, status = %code, "child reaped");
```

---

## Concept 6: Command Audit Trail

### What Is It?

The audit trail is a persistent, append-only log of every command executed in
the shell. Unlike history (which is for command recall), the audit trail
records metadata for observability and forensics.

### Audit Record

```rust
#[cfg(feature = "audit")]
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditRecord {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// The raw command line as entered.
    pub command: String,
    /// Working directory at time of execution.
    pub cwd: String,
    /// Exit code of the command.
    pub exit_code: i32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Session ID (random, assigned at shell startup).
    pub session_id: String,
    /// Monotonically increasing per-session.
    pub sequence: u64,
}
```

### Audit Logger

```rust
#[cfg(feature = "audit")]
pub struct AuditLogger {
    file: std::io::BufWriter<std::fs::File>,
    session_id: String,
    sequence: u64,
}

#[cfg(feature = "audit")]
impl AuditLogger {
    pub fn new() -> std::io::Result<Self> {
        let log_dir = dirs::data_local_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
            .join("jsh")
            .join("audit");

        std::fs::create_dir_all(&log_dir)?;

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let log_path = log_dir.join(format!("{}.jsonl", today));

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        Ok(Self {
            file: std::io::BufWriter::new(file),
            session_id: generate_session_id(),
            sequence: 0,
        })
    }

    pub fn log(
        &mut self,
        command: &str,
        cwd: &str,
        exit_code: i32,
        duration: std::time::Duration,
    ) {
        self.sequence += 1;

        let record = AuditRecord {
            timestamp: chrono::Local::now().to_rfc3339(),
            command: command.to_string(),
            cwd: cwd.to_string(),
            exit_code,
            duration_ms: duration.as_millis() as u64,
            session_id: self.session_id.clone(),
            sequence: self.sequence,
        };

        if let Ok(json) = serde_json::to_string(&record) {
            use std::io::Write;
            let _ = writeln!(self.file, "{}", json);
            let _ = self.file.flush();
        }
    }
}

fn generate_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{:x}", ts)
}
```

### Audit Log Format

Each line is a JSON object (JSONL), one per command:

```jsonl
{"timestamp":"2026-02-16T10:32:01-06:00","command":"ls -la","cwd":"/home/james","exit_code":0,"duration_ms":12,"session_id":"18d8f3a2b","sequence":1}
{"timestamp":"2026-02-16T10:32:05-06:00","command":"grep TODO src/*.rs","cwd":"/home/james/james-shell","exit_code":0,"duration_ms":45,"session_id":"18d8f3a2b","sequence":2}
{"timestamp":"2026-02-16T10:32:09-06:00","command":"cargo build","cwd":"/home/james/james-shell","exit_code":0,"duration_ms":8234,"session_id":"18d8f3a2b","sequence":3}
```

### Querying the Audit Trail

A builtin lets users search their audit log:

```rust
#[cfg(feature = "audit")]
pub fn builtin_audit(args: &[&str], _env: &mut ShellEnv) -> i32 {
    let subcommand = args.first().map(|s| *s).unwrap_or("help");

    match subcommand {
        "show" => {
            // Show recent audit entries.
            let count: usize = args.get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(20);
            show_recent_audit(count)
        }
        "search" => {
            // Search by command pattern.
            let pattern = args.get(1).unwrap_or(&"");
            search_audit(pattern)
        }
        "stats" => {
            // Show session statistics.
            print_audit_stats()
        }
        _ => {
            eprintln!("Usage: audit <show [N]|search PATTERN|stats>");
            1
        }
    }
}
```

Example usage:

```
jsh> audit show 5
 #  TIME         EXIT  DUR    COMMAND
 1  10:32:01     0     12ms   ls -la
 2  10:32:05     0     45ms   grep TODO src/*.rs
 3  10:32:09     0     8.2s   cargo build
 4  10:32:18     1     3ms    cat nonexistent.txt
 5  10:32:20     0     1ms    echo "done"

jsh> audit search "cargo"
 3  10:32:09     0     8.2s   cargo build

jsh> audit stats
Session 18d8f3a2b:
  Commands run:    142
  Failures:        7 (4.9%)
  Total duration:  2m 34s
  Most used:       git (23), cargo (18), ls (15)
```

---

## Concept 7: Session Logging

### What Is Session Logging?

Session logging captures **everything** that appears in the terminal -- input,
output, prompts, and error messages -- to a file. This is similar to the Unix
`script(1)` command but built into the shell.

### The `session-log` Builtin

```rust
pub fn builtin_session_log(args: &[&str], env: &mut ShellEnv) -> i32 {
    match args.first().map(|s| *s) {
        Some("start") => {
            let path = args.get(1).map(|s| s.to_string()).unwrap_or_else(|| {
                let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
                format!("jsh_session_{}.log", ts)
            });
            match start_session_log(&path, env) {
                Ok(()) => {
                    println!("Session logging started: {}", path);
                    0
                }
                Err(e) => {
                    eprintln!("session-log: {}", e);
                    1
                }
            }
        }
        Some("stop") => {
            stop_session_log(env);
            println!("Session logging stopped.");
            0
        }
        Some("status") => {
            match &env.session_log {
                Some(log) => println!("Logging to: {}", log.path),
                None => println!("No active session log."),
            }
            0
        }
        _ => {
            eprintln!("Usage: session-log <start [FILE]|stop|status>");
            1
        }
    }
}
```

### Session Log Writer

```rust
pub struct SessionLog {
    pub path: String,
    writer: std::io::BufWriter<std::fs::File>,
}

impl SessionLog {
    pub fn new(path: &str) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        let mut log = Self {
            path: path.to_string(),
            writer: std::io::BufWriter::new(file),
        };

        // Write a header.
        use std::io::Write;
        writeln!(
            log.writer,
            "# jsh session log started at {}",
            chrono::Local::now().to_rfc3339()
        )?;
        writeln!(log.writer, "# cwd: {}", std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
        )?;
        writeln!(log.writer, "#")?;

        Ok(log)
    }

    pub fn log_input(&mut self, prompt: &str, input: &str) {
        use std::io::Write;
        let _ = write!(self.writer, "{}{}\n", prompt, input);
        let _ = self.writer.flush();
    }

    pub fn log_output(&mut self, output: &str) {
        use std::io::Write;
        let _ = write!(self.writer, "{}", output);
        let _ = self.writer.flush();
    }
}
```

---

## Concept 8: Integration with `set -x`

### Xtrace as a Log Layer

Module 12 introduced `set -x` (xtrace) as a simple `eprintln!("+ ...")`.
With the diagnostics system, xtrace becomes one layer of a broader tracing
system rather than standalone code.

### Before (Module 12 approach)

```rust
if env.options.xtrace {
    eprintln!("+ {}", cmd.argv.join(" "));
}
```

### After (Diagnostics-aware approach)

```rust
if env.options.xtrace {
    // Still prints to stderr for compatibility.
    eprintln!("+ {}", cmd.argv.join(" "));
}

// Additionally emit a structured tracing event (only with `diagnostics` feature).
jsh_debug!(
    command = %cmd.argv.join(" "),
    argv = ?cmd.argv,
    xtrace = env.options.xtrace,
    "executing command"
);
```

This means:
- `set -x` works exactly as users expect, even without the `diagnostics` feature.
- When diagnostics are enabled, the same information is **also** available as a
  structured event, filterable, and routable to a file.

### Enriched Xtrace

With diagnostics enabled, we can optionally enrich xtrace output:

```rust
pub fn print_xtrace(cmd: &SimpleCommand, env: &ShellEnv) {
    if !env.options.xtrace {
        return;
    }

    let depth = env.subshell_depth;
    let prefix = "+".repeat(depth + 1); // ++ for subshells, +++ for nested, etc.

    let line = cmd.argv.join(" ");

    // Basic xtrace (always available).
    eprintln!("{} {}", prefix, line);

    // Enriched xtrace (diagnostics feature).
    #[cfg(feature = "diagnostics")]
    if env.options.xtrace_verbose {
        // Show expansions that occurred.
        if let Some(ref original) = cmd.original_text {
            if original != &line {
                eprintln!("{}   (before expansion: {})", prefix, original);
            }
        }
    }
}
```

### New Shell Option: `xtrace_verbose`

```rust
pub struct ShellOptions {
    pub errexit: bool,
    pub xtrace: bool,
    pub nounset: bool,
    pub pipefail: bool,

    // Extended options (require `diagnostics` feature at compile time).
    #[cfg(feature = "diagnostics")]
    pub xtrace_verbose: bool,
}
```

Toggled with:

```
jsh> set -o xtrace-verbose   # show pre-expansion text alongside xtrace
jsh> set +o xtrace-verbose
```

---

## Concept 9: Log Output Targets

### Multiple Simultaneous Outputs

The `tracing-subscriber` crate supports layered subscribers, meaning we can
send logs to multiple destinations at once:

```
┌─────────────┐
│ tracing      │
│ events       │──▶ stderr (coloured, human-readable)
│              │──▶ file   (JSON, machine-parseable)
│              │──▶ audit  (JSONL, append-only)
└─────────────┘
```

### File Logging with Rotation

For long-running sessions or scripts, log files can grow large. We support
basic date-based rotation:

```rust
#[cfg(feature = "diagnostics")]
pub fn create_file_layer(
    log_dir: &std::path::Path,
) -> impl tracing_subscriber::Layer<tracing_subscriber::Registry> {
    use tracing_subscriber::fmt;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_path = log_dir.join(format!("jsh_{}.log", today));

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("failed to open log file");

    fmt::layer()
        .with_writer(file)
        .with_ansi(false)
        .json()
}
```

### stderr Formatting

For interactive use, stderr output is compact and coloured:

```
  DEBUG jsh::lexer  lexing complete token_count=4
  DEBUG jsh::parser parsing complete node_count=1
   INFO jsh::exec   spawned process command="ls" pid=12345
```

For file output, we use JSON for easy parsing with `jq`:

```json
{"timestamp":"2026-02-16T10:32:01.234Z","level":"DEBUG","target":"jsh::lexer","fields":{"token_count":4},"message":"lexing complete"}
```

---

## Concept 10: Debug Builtins

### The `debug` Builtin

A set of subcommands for inspecting shell internals at runtime:

```rust
pub fn builtin_debug(args: &[&str], env: &mut ShellEnv) -> i32 {
    let subcommand = args.first().map(|s| *s).unwrap_or("help");

    match subcommand {
        "vars" => {
            // Dump all variables and their scopes.
            let mut sorted: Vec<_> = env.variables.iter().collect();
            sorted.sort_by_key(|(k, _)| k.clone());
            for (name, value) in sorted {
                println!("{} = {:?}", name, value);
            }
            0
        }
        "aliases" => {
            // Dump all aliases.
            let mut sorted: Vec<_> = env.aliases.iter().collect();
            sorted.sort_by_key(|(k, _)| k.clone());
            for (name, value) in sorted {
                println!("alias {} = {:?}", name, value);
            }
            0
        }
        "options" => {
            // Show current shell options.
            println!("errexit  (-e): {}", env.options.errexit);
            println!("xtrace   (-x): {}", env.options.xtrace);
            println!("nounset  (-u): {}", env.options.nounset);
            println!("pipefail    : {}", env.options.pipefail);
            #[cfg(feature = "diagnostics")]
            println!("xtrace-verbose: {}", env.options.xtrace_verbose);
            0
        }
        "fds" => {
            // Show open file descriptors (Unix only).
            #[cfg(unix)]
            {
                show_open_fds();
                0
            }
            #[cfg(not(unix))]
            {
                eprintln!("debug fds: not supported on this platform");
                1
            }
        }
        "ast" => {
            // Parse the remaining args as a command and print the AST.
            let input = args[1..].join(" ");
            let tokens = lex(&input);
            match parse(&tokens) {
                Ok(ast) => {
                    for node in &ast {
                        println!("{:#?}", node);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("parse error: {}", e);
                    1
                }
            }
        }
        "tokens" => {
            // Lex the remaining args and print the token stream.
            let input = args[1..].join(" ");
            let tokens = lex(&input);
            for (i, token) in tokens.iter().enumerate() {
                println!("[{}] {:?}", i, token);
            }
            0
        }
        _ => {
            eprintln!("Usage: debug <vars|aliases|options|fds|ast CMD|tokens CMD>");
            1
        }
    }
}
```

### Usage Examples

```
jsh> debug ast echo $HOME | grep -v foo
Pipeline {
    commands: [
        SimpleCommand { argv: ["echo", "$HOME"] },
        SimpleCommand { argv: ["grep", "-v", "foo"] },
    ],
}

jsh> debug tokens for x in 1 2 3 { echo $x }
[0] Word("for")
[1] Word("x")
[2] Word("in")
[3] Word("1")
[4] Word("2")
[5] Word("3")
[6] OpenBrace
[7] Word("echo")
[8] Word("$x")
[9] CloseBrace

jsh> debug options
errexit  (-e): false
xtrace   (-x): false
nounset  (-u): false
pipefail    : false

jsh> debug vars
HOME = "/home/james"
PATH = "/usr/local/bin:/usr/bin:/bin"
PWD = "/home/james/james-shell"
SHELL = "/usr/local/bin/jsh"
```

---

## Putting It All Together

### Activation Summary

| Method | What it enables | Persistence |
|--------|----------------|-------------|
| `JSH_LOG=debug` | Structured tracing to stderr | Per-invocation |
| `--log-level trace` | Same, via CLI flag | Per-invocation |
| `-v` / `-vv` / `-vvv` | Shorthand for info/debug/trace | Per-invocation |
| `--log-file path` | JSON logs to a file | Per-invocation |
| `JSH_LOG=jsh::parser=trace` | Per-subsystem filtering | Per-invocation |
| `set -x` | Traditional xtrace to stderr | Toggled at runtime |
| `set -o xtrace-verbose` | Enriched xtrace with expansions | Toggled at runtime |
| `session-log start` | Full terminal capture to file | Toggled at runtime |
| `audit show` / `audit search` | Query the command audit trail | Always on (with `audit` feature) |
| `debug vars` / `debug ast` | Inspect shell internals | Interactive |

### Build Configurations

```bash
# Production build: no diagnostics overhead.
cargo build --release

# Development build: full diagnostics available.
cargo build --features diagnostics

# Full observability: diagnostics + persistent audit trail.
cargo build --features audit

# Check that the shell compiles cleanly without diagnostics.
cargo check
cargo check --features diagnostics
cargo check --features audit
```

### Integration in `main()`

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opts = CliOptions::parse(&args);

    // Initialize diagnostics (no-op if feature is disabled).
    init_diagnostics(&opts);

    jsh_info!(version = env!("CARGO_PKG_VERSION"), "james-shell starting");

    let mut env = ShellEnv::new();

    // Initialize audit logger.
    #[cfg(feature = "audit")]
    {
        match AuditLogger::new() {
            Ok(logger) => env.audit_logger = Some(logger),
            Err(e) => jsh_warn!(error = %e, "failed to initialize audit logger"),
        }
    }

    load_startup_files(&mut env, opts.interactive);

    if let Some(ref cmd) = opts.command_string {
        let status = execute_string(cmd, &mut env);
        std::process::exit(status);
    }

    if let Some(ref path) = opts.script_file {
        let status = execute_file(path, &mut env);
        std::process::exit(status);
    }

    run_repl(&mut env);

    jsh_info!("james-shell exiting");
}
```

---

## Key Rust Concepts Used

| Concept | Where it appears |
|---------|-----------------|
| **Feature flags (`#[cfg(feature)]`)** | Compile-time gating of all diagnostics code |
| **Procedural macros** | `#[instrument]` attribute from `tracing` |
| **Zero-cost abstractions** | `tracing` callsite mechanism -- disabled spans have no runtime cost |
| **Declarative macros** | `jsh_trace!`, `jsh_debug!`, etc. wrappers |
| **Builder pattern** | `tracing_subscriber::fmt::layer().with_writer(...).json()` |
| **Layered composition** | Multiple subscriber layers (stderr + file + audit) |
| **Serde serialization** | `AuditRecord` derives `Serialize` for JSON output |
| **`BufWriter`** | Buffered I/O for log files and audit trail |
| **Append-only file I/O** | `OpenOptions::new().append(true)` for safe concurrent writes |
| **Conditional compilation** | `#[cfg_attr(feature = "diagnostics", instrument(...))]` |

---

## Milestone

After implementing this module, a debugging session looks like this:

```
$ JSH_LOG=debug jsh
2026-02-16T10:32:00.100Z  INFO jsh: james-shell starting version="0.1.0"

james@laptop:~$ ls | grep rs
2026-02-16T10:32:01.234Z  INFO jsh::repl: processing command line=1
2026-02-16T10:32:01.234Z DEBUG jsh::lexer: lexing complete token_count=4
2026-02-16T10:32:01.235Z DEBUG jsh::parser: parsing complete node_count=1
2026-02-16T10:32:01.235Z DEBUG jsh::executor: executing pipeline stage_count=2
2026-02-16T10:32:01.235Z  INFO jsh::executor: spawned process command="ls" pid=12345
2026-02-16T10:32:01.236Z  INFO jsh::executor: spawned process command="grep" pid=12346
main.rs
lib.rs
2026-02-16T10:32:01.240Z DEBUG jsh::executor: node evaluation complete exit_code=0
2026-02-16T10:32:01.240Z  INFO jsh::repl: command complete line=1 exit_code=0

james@laptop:~$ debug ast echo $HOME | wc -l
Pipeline {
    commands: [
        SimpleCommand { argv: ["echo", "$HOME"] },
        SimpleCommand { argv: ["wc", "-l"] },
    ],
}

james@laptop:~$ audit stats
Session 18d8f3a2b:
  Commands run:    2
  Failures:        0 (0.0%)
  Total duration:  0.25s
  Most used:       ls (1), echo (1)

james@laptop:~$ exit
2026-02-16T10:32:10.500Z  INFO jsh: james-shell exiting
```

And a clean production build has **zero** diagnostics overhead:

```
$ cargo build --release
$ ./target/release/jsh    # no tracing, no audit, minimal binary
```

---

## What's Next?

With diagnostics in place, you can now observe the shell's behaviour at every
level of detail. This is invaluable for the debugging workflows covered in
Module 18 (Error Handling) and for profiling performance bottlenecks as the
shell grows more complex.
