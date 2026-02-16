# Module 2: Command Parsing & Tokenization

## What are we building?

Right now our shell just echoes input back. A real shell needs to **understand** what the user typed. That means turning a raw string like:

```
echo "hello   world" foo\ bar 'single quotes'
```

Into structured data:

```rust
Command {
    program: "echo",
    args: ["hello   world", "foo bar", "single quotes"],
}
```

This is called **parsing**, and it happens in two stages: **tokenization** (lexing) and **parsing**.

---

## Concept 1: Tokenization (Lexing)

Tokenization means breaking a raw string into meaningful chunks called **tokens**. It's the first step in understanding any structured input — compilers, shells, JSON parsers, and config file readers all do this.

### The naive approach: split on whitespace

```rust
let tokens: Vec<&str> = input.split_whitespace().collect();
// "echo hello world" → ["echo", "hello", "world"]  ✓
// "echo "hello world"" → ["echo", "\"hello", "world\""]  ✗ WRONG
```

This breaks immediately with quoted strings. `"hello world"` should be ONE argument, not two.

### The real approach: a state machine

A state machine is a pattern where your code is always in one of several **states**, and each character of input causes a **transition** between states.

For a shell tokenizer, the states are:

```
┌──────────┐     regular char     ┌──────────┐
│  Normal   │ ──────────────────→ │  InWord  │
│ (between  │ ←────────────────── │          │
│  tokens)  │    whitespace       └──────────┘
└──────────┘
      │  "                             │  "
      ▼                                ▼
┌──────────┐                    ┌──────────┐
│ InDouble │ ←─── any char ───→ │ InDouble │
│  Quote   │ ──── " found ────→ │  (end)   │
└──────────┘                    └──────────┘
```

States for our tokenizer:
- **Normal** — between tokens, whitespace is skipped
- **InWord** — building an unquoted token, whitespace ends it
- **InDoubleQuote** — inside `"..."`, whitespace is kept, `"` ends the state
- **InSingleQuote** — inside `'...'`, everything is literal until closing `'`
- **Escaped** — the next character is taken literally (after `\`)

### Why enums are perfect for this

Rust enums map directly to state machines:

```rust
enum LexerState {
    Normal,
    InWord,
    InDoubleQuote,
    InSingleQuote,
    Escaped,
}
```

Then you match on `(state, current_char)` to decide what to do:

```rust
match (state, ch) {
    (Normal, '"')  => state = InDoubleQuote,
    (Normal, '\'') => state = InSingleQuote,
    (Normal, ' ')  => { /* skip whitespace */ },
    (Normal, c)    => { token.push(c); state = InWord; },
    // ... etc
}
```

---

## Concept 2: The `Command` struct

After tokenization, we parse the tokens into a command structure:

```rust
#[derive(Debug)]
pub struct Command {
    pub program: String,       // The program to run (first token)
    pub args: Vec<String>,     // Arguments (remaining tokens)
}
```

For now, this is simple — the first token is the program, the rest are args. Later modules will add fields for:
- Redirections (`> file`, `< file`)
- Background flag (`&`)
- Pipe connections (`|`)

Using `#[derive(Debug)]` lets us print the struct with `{:?}` for debugging — very useful during development.

---

## Concept 3: Escape characters

The backslash `\` is the escape character. It means "treat the next character literally, even if it's special."

```
echo hello\ world     →  one argument: "hello world"
echo "hello\"world"   →  one argument: hello"world
echo \\               →  one argument: \
```

In the state machine, when we see `\`, we save the current state, switch to `Escaped`, and on the next character we push it directly into the token and return to the saved state.

---

## Concept 4: Single vs Double quotes

They look similar but behave differently:

| Feature | Single quotes `'...'` | Double quotes `"..."` |
|---------|----------------------|----------------------|
| Spaces preserved | Yes | Yes |
| Escape sequences (`\n`) | No — everything is literal | Yes — `\` works inside |
| Variable expansion (`$VAR`) | No — literal `$VAR` | Yes (Module 5) |

For Module 2, we won't have variable expansion yet, so they're almost the same. But we build them as separate states now so we're ready for Module 5.

---

## Key Rust concepts used

- **Enums** — representing lexer states
- **`match` expressions** — the core of the state machine
- **`Peekable<Chars>`** — iterating over characters with the ability to look ahead
- **`Vec<String>`** — collecting tokens
- **`impl` blocks** — adding methods to our structs
- **Modules (`mod parser`)** — splitting code into separate files

### Rust modules primer

When your code grows, you split it into modules:

```rust
// In src/main.rs:
mod parser;  // This tells Rust to look for src/parser.rs

use parser::Command;
```

```rust
// In src/parser.rs:
pub struct Command { ... }
pub fn parse(input: &str) -> Option<Command> { ... }
```

The `pub` keyword makes items visible outside the module. Without it, they're private.

---

## Milestone

```
jsh> echo hello world
Command { program: "echo", args: ["hello", "world"] }
jsh> echo "hello   world" foo\ bar 'single quotes'
Command { program: "echo", args: ["hello   world", "foo bar", "single quotes"] }
jsh>
jsh> [empty input is ignored]
```

---

## What's next?

Module 3 takes our parsed `Command` struct and actually **executes** it — running real programs on your system.
