# Module 1: The REPL Loop

## What is a REPL?

REPL stands for **Read-Eval-Print Loop**. Every shell you've ever used (bash, PowerShell, cmd) is built on this pattern:

```
loop {
    1. Print a prompt      →  jsh>
    2. Read user input     →  "hello world"
    3. Evaluate/process it →  (later: run commands. for now: just echo it)
    4. Print the result    →  "You typed: hello world"
}
```

That's it. A shell is fundamentally just this loop with increasingly sophisticated behavior in step 3.

## Why start here?

Because even this simple loop teaches you real systems concepts.

---

## Concept 1: Buffered I/O

In Rust, `print!("jsh> ")` does **not** immediately show text on screen. Why? Because stdout is *buffered* — Rust collects output in memory and only actually writes it when:

- A newline (`\n`) appears (like `println!`)
- The buffer is full
- You explicitly call `flush()`

So if you write `print!("jsh> ")` and then wait for input, the user sees... nothing. The prompt is stuck in the buffer. You need `stdout().flush()` to force it out.

```rust
use std::io::{self, Write};

// This WON'T show the prompt before waiting for input:
print!("jsh> ");
// The text is sitting in a buffer, not on screen yet!

// You need this to force it out:
io::stdout().flush().unwrap();
// NOW the user sees "jsh> " and can type
```

### Why does buffering exist?

Writing to the terminal is *slow* compared to writing to memory. If every single character went straight to the screen, programs that print lots of output would crawl. Buffering collects many small writes into one big write — much faster.

The tradeoff: interactive prompts need an explicit `flush()`.

---

## Concept 2: Reading input with `read_line()`

`stdin().read_line(&mut buffer)` reads one line from the user (up to and including the newline character `\n`).

```rust
let mut input = String::new();
let bytes_read = io::stdin().read_line(&mut input)?;
```

Key things to know:

- **It returns `Result<usize>`** — the number of bytes read, wrapped in a Result for error handling
- **The newline is included** — if the user types "hello" and presses Enter, `input` contains `"hello\n"`. Use `.trim()` to strip it
- **`read_line` appends** — it doesn't clear the buffer first! If you reuse the same `String`, you must call `.clear()` each iteration

---

## Concept 3: EOF (End of File)

When `read_line()` returns `Ok(0)` (zero bytes read), that means **EOF** — the input stream is closed.

On a terminal:
- **Unix/Mac:** Ctrl-D sends EOF
- **Windows:** Ctrl-Z then Enter sends EOF

A good shell says "Goodbye!" and exits cleanly instead of crashing or looping forever.

```rust
let bytes_read = io::stdin().read_line(&mut input)?;
if bytes_read == 0 {
    // EOF — user pressed Ctrl-D (Unix) or Ctrl-Z+Enter (Windows)
    println!("\nGoodbye!");
    break;
}
```

### Why does Ctrl-D/Ctrl-Z mean EOF?

Terminals are pretending to be files. When a program reads from stdin, it's reading from a "file" that happens to be your keyboard. Ctrl-D/Ctrl-Z is the terminal's way of saying "this file is done — there's nothing more to read."

This matters later: when you pipe data into your shell (`echo "ls" | jsh`), EOF happens naturally when the pipe runs out of data.

---

## Concept 4: Ctrl-C (SIGINT)

When you press Ctrl-C, the OS sends a **signal** called SIGINT (Signal Interrupt) to the process. By default, this **kills the program**.

But a shell should NOT die from Ctrl-C. Think about bash — when you Ctrl-C, it cancels the current line and gives you a fresh prompt. The shell itself survives.

We use the `ctrlc` crate to intercept this signal cross-platform:

```rust
ctrlc::set_handler(|| {
    // This runs when Ctrl-C is pressed
    // For now: just print a newline so the prompt looks clean
    print!("\n");
})
.expect("Error setting Ctrl-C handler");
```

### What's a signal?

Signals are the OS's way of poking a running process to tell it something happened. Common signals:

| Signal | Trigger | Default Action | What shells do |
|--------|---------|---------------|----------------|
| SIGINT | Ctrl-C | Kill process | Cancel current line, new prompt |
| SIGTSTP | Ctrl-Z | Suspend process | Suspend foreground job (Module 8) |
| SIGTERM | `kill` command | Kill process | Clean shutdown |
| SIGCHLD | Child exits | Ignore | Reap background jobs (Module 8) |

We'll handle more signals in Module 9. For now, just Ctrl-C.

---

## The complete REPL structure

Putting it all together, here's what Module 1 looks like in pseudocode:

```
1. Set up Ctrl-C handler (so it doesn't kill us)
2. Loop forever:
   a. Print "jsh> "
   b. Flush stdout (so the prompt actually appears)
   c. Read a line from stdin
   d. If EOF (0 bytes): print "Goodbye!" and break
   e. Trim the newline off the input
   f. If input is empty (user just pressed Enter): continue
   g. Print "You typed: {input}"
3. Exit cleanly
```

---

## Key Rust concepts used

- **`use std::io::{self, Write}`** — importing the `Write` trait is required to call `.flush()` on stdout
- **`Result` and `?` operator** — `read_line` can fail (rare, but possible), we handle it with `?` or `.expect()`
- **`String::new()` and `.clear()`** — mutable string buffer for reading input
- **`loop` and `break`** — Rust's infinite loop with explicit exit
- **`.trim()`** — returns a `&str` with whitespace stripped from both ends

---

## Milestone

When you're done, your shell should handle these scenarios:

```
$ cargo run
jsh> hello world
You typed: hello world
jsh>
jsh> foo bar baz
You typed: foo bar baz
jsh> [Ctrl-C]
jsh> [Ctrl-D or Ctrl-Z+Enter]
Goodbye!
```

### Behavior Notes
- Prompt is `jsh> ` on every loop.
- `Ctrl-C` prints a newline and re-displays the prompt (no exit).
- EOF exits the shell (`Ctrl-D` on Unix, `Ctrl-Z` then Enter on Windows).

---

## What's next?

Module 2 will replace the "You typed:" echo with a real **parser** that breaks input into a structured command (program name + arguments). That's where things start feeling like a real shell.
