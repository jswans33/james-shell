# Module 7: Pipes

## What are we building?

Pipes are the defining feature of Unix philosophy: small, focused programs that do one thing well, connected together to do complex things. The `|` operator takes the stdout of one command and feeds it directly into the stdin of the next.

After this module, your shell will support:

```
ls | grep .rs                     # two-command pipe
cat data.txt | sort | uniq -c     # three-command pipe chain
find . -name "*.rs" | head -5     # pipe with early termination
```

This is where our shell crosses the line from "toy that runs programs" to "tool that composes programs." Every real shell user depends on pipes daily.

---

## Concept 1: What Is a Pipe?

A pipe is a one-way data channel between two processes. One process writes to it, and another reads from it. The operating system handles the buffering.

```
  ┌───────────┐        pipe         ┌───────────┐
  │   ls      │ ─── stdout ──────→ │   grep    │
  │           │    (write end)      │           │ ─── stdout ──→ terminal
  │  fd 1 ──→ │ =================== │ ──→ fd 0  │
  └───────────┘                     └───────────┘
     writes                            reads
```

The pipe is a kernel-managed buffer (typically 64KB on Linux, varies on other systems). When the writer fills it, the writer blocks until the reader consumes some data. When the reader empties it, the reader blocks until the writer produces more. This backpressure mechanism means pipes work with datasets of any size -- the entire dataset never needs to be in memory at once.

### Key properties of pipes

| Property | Description |
|----------|-------------|
| **Unidirectional** | Data flows one way: writer to reader |
| **Buffered** | The OS maintains a buffer between writer and reader |
| **Blocking** | Writers block when the buffer is full, readers block when it is empty |
| **EOF on close** | When the writer closes its end, the reader gets EOF |
| **No seek** | You cannot rewind or jump around -- data is consumed in order |

### What happens physically

On Unix, `pipe()` creates two file descriptors: a read end and a write end. The parent shell creates the pipe, then sets up the children so that one child's stdout connects to the write end and the other child's stdin connects to the read end.

On Windows, the equivalent is `CreatePipe()`. The mechanism is different internally but the behavior is the same.

Rust's `std::process::Command` with `Stdio::piped()` handles both platforms transparently. We never call `pipe()` or `CreatePipe()` directly.

---

## Concept 2: Pipes with `std::process::Command`

Here is the core pattern for connecting two processes with a pipe in Rust:

```rust
use std::process::{Command, Stdio};

// Step 1: Spawn the first command with piped stdout
let mut child1 = Command::new("ls")
    .args(&["-la"])
    .stdout(Stdio::piped())     // Capture stdout into a pipe
    .spawn()
    .expect("failed to start ls");

// Step 2: Take the stdout pipe from child1
// .take() moves it out of the Option, leaving None behind
let child1_stdout = child1.stdout.take()
    .expect("failed to capture stdout");

// Step 3: Spawn the second command with stdin connected to child1's stdout
let child2 = Command::new("grep")
    .args(&[".rs"])
    .stdin(Stdio::from(child1_stdout))   // Connect pipe to stdin
    .spawn()
    .expect("failed to start grep");

// Step 4: Wait for both processes
let status2 = child2.wait_with_output().expect("failed to wait for grep");
let status1 = child1.wait().expect("failed to wait for ls");
```

### What `Stdio::piped()` does

When you set `.stdout(Stdio::piped())`, Rust:
1. Creates an OS pipe (a pair of file descriptors)
2. Attaches the write end to the child's stdout (fd 1)
3. Gives you the read end through `child.stdout` (as a `ChildStdout`)

The `ChildStdout` type implements `Read` and can be converted to `Stdio` with `Stdio::from()`, which is exactly what we need to feed it into the next command's stdin.

### What `Stdio::from(child_stdout)` does

It takes ownership of the `ChildStdout` (which is really just a file descriptor wrapper) and passes it to the new child as its stdin. The OS-level file descriptor is transferred -- no data copying happens.

---

## Concept 3: Pipe Chains (`cmd1 | cmd2 | cmd3`)

A pipeline can have any number of stages. The pattern extends naturally:

```
  cmd1  |  cmd2  |  cmd3
   stdout→stdin  stdout→stdin  stdout→terminal
```

Each `|` creates a pipe between adjacent commands. In a 3-command pipeline, there are 2 pipes:

```
  ┌──────┐  pipe1  ┌──────┐  pipe2  ┌──────┐
  │ cmd1 │ ──────→ │ cmd2 │ ──────→ │ cmd3 │ ──→ terminal
  └──────┘         └──────┘         └──────┘
```

### Implementation: a general pipeline function

```rust
use std::process::{Child, Command, Stdio};

/// Execute a pipeline of commands.
/// Each command is (program, args).
/// Returns the exit code of the last command in the pipeline.
pub fn execute_pipeline(
    commands: &[(String, Vec<String>)],
) -> i32 {
    if commands.is_empty() {
        return 0;
    }

    // Special case: single command (no pipes needed)
    if commands.len() == 1 {
        let (ref program, ref args) = commands[0];
        return execute_single(program, args);
    }

    let mut children: Vec<Child> = Vec::new();
    let mut previous_stdout: Option<std::process::ChildStdout> = None;

    for (i, (program, args)) in commands.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i == commands.len() - 1;

        let mut cmd = Command::new(program);
        cmd.args(args);

        // Connect stdin: first command inherits terminal,
        // others read from the previous command's stdout pipe
        if let Some(prev_stdout) = previous_stdout.take() {
            cmd.stdin(Stdio::from(prev_stdout));
        }
        // else: first command, stdin is inherited from the shell (terminal)

        // Connect stdout: last command inherits terminal,
        // others pipe their stdout to the next command
        if !is_last {
            cmd.stdout(Stdio::piped());
        }
        // else: last command, stdout goes to terminal (or any redirections)

        match cmd.spawn() {
            Ok(mut child) => {
                // Grab this child's stdout pipe for the next command
                if !is_last {
                    previous_stdout = child.stdout.take();
                }
                children.push(child);
            }
            Err(e) => {
                eprintln!("jsh: {}: {}", program, e);
                // Clean up any already-spawned children
                for mut child in children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return 127;
            }
        }
    }

    // Wait for ALL children to finish
    let mut last_exit_code = 0;
    for (i, mut child) in children.into_iter().enumerate() {
        match child.wait() {
            Ok(status) => {
                let code = status.code().unwrap_or(-1);
                if i == commands.len() - 1 {
                    last_exit_code = code;
                }
            }
            Err(e) => {
                eprintln!("jsh: error waiting for process: {}", e);
                last_exit_code = 1;
            }
        }
    }

    last_exit_code
}

fn execute_single(program: &str, args: &[String]) -> i32 {
    match Command::new(program).args(args).status() {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => {
            eprintln!("jsh: {}: {}", program, e);
            127
        }
    }
}
```

### Step-by-step walkthrough for `ls | grep .rs | head -3`

```
Iteration 0 (ls):
  - No previous_stdout → stdin = terminal (inherited)
  - Not last → stdout = Stdio::piped()
  - Spawn child0
  - Save child0.stdout as previous_stdout

Iteration 1 (grep .rs):
  - previous_stdout exists → stdin = Stdio::from(child0's stdout)
  - Not last → stdout = Stdio::piped()
  - Spawn child1
  - Save child1.stdout as previous_stdout

Iteration 2 (head -3):
  - previous_stdout exists → stdin = Stdio::from(child1's stdout)
  - IS last → stdout = terminal (inherited)
  - Spawn child2

Wait for child0, child1, child2.
Return child2's exit code.
```

---

## Concept 4: Why Waiting for All Processes Matters

A common mistake in shell implementations is to only wait for the last process. This causes two problems:

### Problem 1: Zombie processes

On Unix, when a process exits but its parent has not called `wait()` on it, it becomes a **zombie**. It consumes a PID and an entry in the process table. If you run enough pipelines without waiting, you can run out of PIDs.

```
  zombie: a process that has exited but whose parent
  hasn't acknowledged it with wait()

  $ ps aux | grep defunct
  jswan  12345  Z  ls <defunct>      <-- zombie!
```

### Problem 2: Broken pipe behavior

If the last command finishes early (like `head -3` in `cat bigfile | head -3`), the earlier commands receive a `SIGPIPE` signal when they try to write to the closed pipe. This is normal and expected -- the commands die gracefully. But the shell still needs to `wait()` for them to clean up properly.

### Our approach

We wait for every child in the pipeline:

```rust
for mut child in children {
    let _ = child.wait();
}
```

Rust's `Child` type also waits in its `Drop` implementation as of recent Rust versions, but relying on implicit drop ordering is fragile. Always wait explicitly.

---

## Concept 5: Pipeline Exit Status

In bash, the exit status of a pipeline is the exit status of the **last** command:

```bash
false | true
echo $?    # 0 (true succeeded)

true | false
echo $?    # 1 (false failed)
```

Some shells (bash with `set -o pipefail`, zsh) offer an option where the pipeline fails if *any* command fails:

```bash
set -o pipefail
true | false | true
echo $?    # 1 (false failed, even though the last command succeeded)
```

### Our implementation

We follow the standard convention: the exit code of the last command is the pipeline's exit code. We can add `pipefail` as an option later.

```rust
// In execute_pipeline, after waiting:
let mut last_exit_code = 0;
for (i, mut child) in children.into_iter().enumerate() {
    match child.wait() {
        Ok(status) => {
            if i == commands.len() - 1 {
                // Only the last command's exit code matters
                last_exit_code = status.code().unwrap_or(-1);
            }
        }
        Err(e) => {
            eprintln!("jsh: error waiting for process: {}", e);
        }
    }
}
```

### Future enhancement: `$PIPESTATUS`

Bash provides the `PIPESTATUS` array variable that records every command's exit status:

```bash
false | true | false
echo ${PIPESTATUS[@]}    # 1 0 1
```

To support this, we would save every exit code:

```rust
let mut pipe_status: Vec<i32> = Vec::new();

for mut child in children {
    match child.wait() {
        Ok(status) => pipe_status.push(status.code().unwrap_or(-1)),
        Err(_) => pipe_status.push(-1),
    }
}

// Store pipe_status in the Shell struct for $PIPESTATUS access
shell.pipe_status = pipe_status;
```

---

## Concept 6: Updating the Parser for Pipes

The parser needs to recognize `|` as a pipe separator and split the input into multiple commands.

### The `Pipeline` type

```rust
/// A pipeline is a sequence of commands connected by pipes.
/// `ls -la | grep .rs | sort` is a pipeline of three commands.
#[derive(Debug)]
pub struct Pipeline {
    pub commands: Vec<PipelineCommand>,
}

/// A single command within a pipeline.
#[derive(Debug)]
pub struct PipelineCommand {
    pub program: String,
    pub args: Vec<String>,
    pub redirections: Vec<Redirection>,
}
```

### Parsing pipes

After tokenization, we split the token list on `|`:

```rust
pub fn parse(input: &str) -> Option<Pipeline> {
    let tokens = tokenize(input);
    if tokens.is_empty() {
        return None;
    }

    // Split tokens on pipe characters
    let segments = split_on_pipes(&tokens);

    let mut commands = Vec::new();

    for segment in segments {
        if segment.is_empty() {
            eprintln!("jsh: syntax error near unexpected token '|'");
            return None;
        }

        // Parse each segment into a command with possible redirections
        let (args, redirections) = parse_redirections(&segment);

        if args.is_empty() {
            eprintln!("jsh: syntax error near unexpected token '|'");
            return None;
        }

        commands.push(PipelineCommand {
            program: args[0].clone(),
            args: args[1..].to_vec(),
            redirections,
        });
    }

    Some(Pipeline { commands })
}

fn split_on_pipes(tokens: &[String]) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = Vec::new();

    for token in tokens {
        if token == "|" {
            segments.push(current);
            current = Vec::new();
        } else {
            current.push(token.clone());
        }
    }

    segments.push(current); // Don't forget the last segment
    segments
}
```

### Edge cases in pipe parsing

The parser must handle (and reject) malformed pipelines:

```rust
// These are syntax errors:
"|"              // pipe with no left side
"ls |"           // pipe with no right side
"ls | | grep"    // empty segment in the middle
"ls || grep"     // this is OR logic (Module 11), not a double pipe

// These are valid:
"ls | grep .rs"           // two commands
"ls | grep .rs | sort"    // three commands
"ls -la | head -5"        // commands with arguments
```

### Tokenizer update for `|`

The tokenizer must emit `|` as its own token, even if there is no whitespace around it:

```rust
// In the tokenizer state machine:
match (state, ch) {
    (Normal, '|') | (InWord, '|') => {
        finalize_current_token();
        tokens.push("|".to_string());
        state = Normal;
    }
    // ... existing rules ...
}
```

---

## Concept 7: Pipes Combined with Redirections

Pipes and redirections can coexist. The rules are:

1. **Redirections override the pipe** for the specific fd they target
2. **The pipe only affects stdout and stdin** between adjacent commands
3. **Stderr passes through** unless explicitly redirected

### Examples

```bash
# Pipe stdout, but redirect stderr to a file
ls /nonexistent /tmp | grep tmp 2> errors.txt
# ls's stdout goes through the pipe to grep
# ls's stderr goes to errors.txt (not through the pipe)

# Redirect stdout of the last command
ls | sort > sorted.txt
# ls's stdout goes through the pipe to sort
# sort's stdout goes to sorted.txt (not the terminal)

# Redirect stdin of the first command
sort < data.txt | head -5
# sort reads from data.txt (not the terminal)
# sort's stdout goes through the pipe to head
```

### Implementation in the executor

When executing a pipeline, we apply redirections per-command:

```rust
pub fn execute_pipeline(pipeline: &Pipeline, shell: &mut Shell) -> i32 {
    let commands = &pipeline.commands;

    if commands.len() == 1 {
        return execute_single_command(&commands[0], shell);
    }

    let mut children: Vec<Child> = Vec::new();
    let mut previous_stdout: Option<std::process::ChildStdout> = None;

    for (i, pipe_cmd) in commands.iter().enumerate() {
        let is_last = i == commands.len() - 1;

        let mut cmd = Command::new(&pipe_cmd.program);
        cmd.args(&pipe_cmd.args);

        // Connect pipe stdin (unless this is the first command)
        if let Some(prev_stdout) = previous_stdout.take() {
            cmd.stdin(Stdio::from(prev_stdout));
        }

        // Connect pipe stdout (unless this is the last command)
        if !is_last {
            cmd.stdout(Stdio::piped());
        }

        // Apply per-command redirections (these OVERRIDE the pipe settings)
        // For example: `ls 2> err.txt | grep foo`
        //   - The pipe sets ls's stdout to piped (already done above)
        //   - The redirection sets ls's stderr to err.txt
        //   - stdin is inherited (first command) -- no override
        apply_redirections(&mut cmd, &pipe_cmd.redirections);

        match cmd.spawn() {
            Ok(mut child) => {
                if !is_last {
                    previous_stdout = child.stdout.take();
                }
                children.push(child);
            }
            Err(e) => {
                eprintln!("jsh: {}: {}", pipe_cmd.program, e);
                for mut c in children {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                return 127;
            }
        }
    }

    // Wait for all children
    let mut last_exit_code = 0;
    for (i, mut child) in children.into_iter().enumerate() {
        match child.wait() {
            Ok(status) => {
                if i == commands.len() - 1 {
                    last_exit_code = status.code().unwrap_or(-1);
                }
            }
            Err(e) => {
                eprintln!("jsh: error waiting for process: {}", e);
                last_exit_code = 1;
            }
        }
    }

    last_exit_code
}
```

### The order of application

Redirections are applied *after* the pipe setup. This means:

```bash
echo hello > file.txt | cat
```

This redirects echo's stdout to `file.txt`. The pipe to `cat` receives nothing because the redirection overrode it. This matches bash behavior -- `cat` gets empty input and produces no output.

---

## Concept 8: Cross-Platform Considerations

Our pipe implementation using `std::process::Command` is already cross-platform. Here are the differences under the hood and the edge cases to watch for:

### Platform differences Rust handles for us

| Concern | Unix | Windows | Rust handles it? |
|---------|------|---------|:----------------:|
| Creating a pipe | `pipe()` syscall | `CreatePipe()` | Yes |
| Setting child stdin/stdout | `dup2()` + `fork()`/`exec()` | `STARTUPINFO` in `CreateProcess()` | Yes |
| Waiting for a child | `waitpid()` | `WaitForSingleObject()` | Yes |
| Pipe buffer size | Typically 64KB | Typically 4KB | Yes (transparent) |
| Broken pipe signal | `SIGPIPE` kills writer | Write returns error | Yes (Rust catches both) |

### Edge cases to be aware of

**Line endings:** On Windows, some programs output `\r\n` instead of `\n`. When piping between programs, this is usually fine (each program handles it). But if your shell ever processes pipe data directly, normalize line endings.

**Binary data:** Pipes can carry binary data (not just text). Do not assume the data is UTF-8. When we need to process pipe output inside the shell (e.g., for command substitution in Module 11), use `Vec<u8>` or `OsString`, not `String`.

**Console programs vs GUI programs on Windows:** On Windows, `Stdio::piped()` may not work correctly with GUI applications. Stick to console programs for piping.

### A note about performance

The Rust `Command` API adds minimal overhead over the raw system calls. The pipe buffer is managed by the OS kernel, so data flows at memory speed between processes. The bottleneck is always the programs themselves, not the pipe.

---

## Key Rust concepts used

- **`std::process::Stdio::piped()`** -- creating a pipe for a child's stdin or stdout
- **`std::process::Stdio::from(child_stdout)`** -- converting a `ChildStdout` into `Stdio` for another child's stdin, transferring the file descriptor
- **`child.stdout.take()`** -- using `Option::take()` to move the `ChildStdout` out of the `Child` struct so ownership can be transferred
- **`Vec<Child>`** -- collecting all spawned children so we can wait for them all
- **`into_iter().enumerate()`** -- consuming the vector of children while tracking the index, so we can identify the last command
- **`Stdio::inherit()`** -- the default for first-command stdin and last-command stdout (terminal passthrough)
- **Ownership transfer** -- the `ChildStdout` is moved into `Stdio::from()`, then moved into the next `Command`. Rust's ownership model ensures only one process holds the read end of each pipe.
- **Error handling with `match`** -- each `spawn()` and `wait()` can fail, and we handle both cases

### Why ownership matters here

Consider what would happen without ownership semantics. If two processes both held the read end of a pipe, they would compete for data -- each read by one process would consume bytes that the other never sees. Rust prevents this at compile time. The `ChildStdout` moves from `child1` to `Stdio::from()` to `child2.stdin`. At no point can two things read from the same pipe end.

Similarly, when `ChildStdout` is dropped (because we transferred it with `Stdio::from`), the original child's `.stdout` field becomes `None`. Attempting to access it would require handling the `Option`, which Rust forces you to do.

---

## Milestone

After implementing pipes, your shell should handle these scenarios:

```
jsh> echo hello | cat
hello

jsh> echo "hello world" | tr ' ' '\n'
hello
world

jsh> ls | grep .rs
main.rs
lib.rs
parser.rs

jsh> ls | grep .rs | sort -r
parser.rs
main.rs
lib.rs

jsh> echo hello | cat | cat | cat | cat
hello

jsh> ls /nonexistent 2>&1 | grep "No such"
ls: cannot access '/nonexistent': No such file or directory

jsh> ls | sort > sorted.txt
jsh> cat sorted.txt
Cargo.lock
Cargo.toml
docs
src
target

jsh> echo $?
0

jsh> cat nonexistent_file | head -1
cat: nonexistent_file: No such file or directory
jsh> echo $?
0
```

Notice the last example: `cat` fails, but the pipeline exit code is 0 because `head` (the last command) succeeded. This is standard shell behavior.

On Windows, the same pipeline syntax works. Programs like `findstr` serve as the Windows equivalent of `grep`, but many users install Unix-style tools (via Git Bash, MSYS2, or Windows Subsystem for Linux). Our shell does not care which programs are used -- it just connects their I/O.

---

## What's next?

Module 8 adds **job control** -- running commands in the background with `&`, listing running jobs with `jobs`, and bringing them back to the foreground with `fg`. That is when `sleep 30 &` stops blocking your shell.
