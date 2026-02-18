# Module 5: Environment Variables & Expansion

## What are we building?

Right now, if a user types `echo $HOME`, our shell passes the literal string `$HOME` as an argument to `echo`. A real shell **expands** that into something like `/home/jswan` *before* the command ever runs. After this module, your shell will support variable expansion (`$VAR`, `${VAR}`), tilde expansion (`~`), glob/wildcard expansion (`*.rs`), and understand the critical difference between single and double quotes during expansion.

Expansion is a **pipeline** -- a series of transformations applied to the user's input between parsing and execution. Getting the order right is what separates a working shell from a buggy one.

```
Raw input: echo ~/*.rs "$HOME" '$HOME'

     |  1. Tilde expansion        ~  -->  /home/jswan
     |  2. Variable expansion      $HOME  -->  /home/jswan (but NOT inside single quotes)
     |  3. Glob expansion          /home/jswan/*.rs  -->  /home/jswan/main.rs lib.rs ...
     |  4. Word splitting          (split unquoted results on whitespace)
     v

Final args: ["echo", "/home/jswan/main.rs", "/home/jswan/lib.rs", "/home/jswan", "$HOME"]
```

---

## Concept 1: The Expansion Pipeline

Shells apply expansions in a specific, well-defined order. Getting this wrong causes subtle bugs that are maddening to debug. Here is the order bash uses, and the order we will implement:

```
 Input tokens (from the parser, Module 2)
    |
    v
 1. Tilde Expansion         ~  -->  /home/jswan
    |
    v
 2. Variable Expansion       $VAR, ${VAR}, $?, $$, etc.
    |
    v
 3. Glob/Pathname Expansion  *.rs  -->  list of matching files
    |
    v
 4. Word Splitting           split unquoted expanded text on $IFS
    |
    v
 Final argument list (passed to the executor)
```

### Why does order matter?

Consider `export DIR=~` followed by `ls $DIR/*.rs`. If we expand variables before tildes, `$DIR` becomes `~` (the literal character), and then tilde expansion turns it into `/home/jswan`. But if we did it the other way, `$DIR` would already be `/home/jswan` from the `export`, and tilde expansion would have nothing to do. Both might work in this case, but edge cases diverge quickly. Following the standard order keeps behavior predictable.

### Where does expansion fit in the architecture?

We add a new module to our pipeline:

```
main.rs  -->  parser.rs  -->  expander.rs  -->  executor.rs
  REPL         tokenize        expand vars       run command
  loop         & parse         globs, tildes
```

```rust
// In src/expander.rs
pub fn expand(tokens: Vec<String>, last_exit_code: i32) -> Vec<String> {
    let mut result = Vec::new();
    for token in tokens {
        let expanded = expand_tilde(&token);
        let expanded = expand_variables(&expanded, last_exit_code);
        let globbed = expand_globs(&expanded);
        result.extend(globbed);
    }
    result
}
```

Each step is a function that takes a string and returns a (possibly different) string. Glob expansion is special because it can turn one token into *many* tokens (one per matching file).

---

## Concept 2: Variable Expansion (`$VAR` and `${VAR}`)

Variable expansion replaces references to environment variables with their values.

### Two syntaxes

| Syntax | Example | Use case |
|--------|---------|----------|
| `$VAR` | `$HOME` | Simple -- name is delimited by non-alphanumeric characters |
| `${VAR}` | `${HOME}` | Explicit boundaries -- needed when the name is ambiguous |

The difference matters when a variable is adjacent to other text:

```
echo $HOMEdir       # Looks for variable "HOMEdir" -- probably undefined!
echo ${HOME}dir     # Expands $HOME, then appends "dir" --> /home/jswandir
```

### Parsing variable names

A variable name is a sequence of alphanumeric characters and underscores, starting with a letter or underscore. We scan forward from the `$` to find where the name ends:

```rust
fn extract_var_name(input: &str) -> (&str, &str) {
    // input starts AFTER the '$'
    // Returns (variable_name, rest_of_string)
    if input.starts_with('{') {
        // ${VAR} syntax -- find the closing brace
        if let Some(end) = input.find('}') {
            return (&input[1..end], &input[end + 1..]);
        }
        // No closing brace -- treat as literal
        return ("", input);
    }

    // $VAR syntax -- name is [a-zA-Z_][a-zA-Z0-9_]*
    let end = input
        .char_indices()
        .find(|(i, c)| {
            if *i == 0 {
                !c.is_ascii_alphabetic() && *c != '_'
            } else {
                !c.is_ascii_alphanumeric() && *c != '_'
            }
        })
        .map(|(i, _)| i)
        .unwrap_or(input.len());

    (&input[..end], &input[end..])
}
```

### The expansion function

```rust
fn expand_variables(input: &str, last_exit_code: i32) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Collect the rest of the string from this position
            let remaining: String = chars.clone().collect();

            if remaining.is_empty() {
                // Trailing $ with nothing after it -- keep it literal
                result.push('$');
                continue;
            }

            let (name, rest) = extract_var_name(&remaining);

            if name.is_empty() {
                result.push('$');
                continue;
            }

            // Look up the variable
            let value = match name {
                "?" => last_exit_code.to_string(),
                "$" => std::process::id().to_string(),
                "0" => "jsh".to_string(),
                _ => std::env::var(name).unwrap_or_default(),
            };

            result.push_str(&value);

            // Advance the chars iterator past the variable name we consumed
            let consumed = remaining.len() - rest.len();
            for _ in 0..consumed {
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }

    result
}
```

---

## Concept 3: Special Variables

Shells define several special variables that are not environment variables -- they are maintained by the shell itself:

| Variable | Meaning | Example value |
|----------|---------|---------------|
| `$?` | Exit code of the last command | `0`, `1`, `127` |
| `$$` | PID of the shell process | `12345` |
| `$0` | Name of the shell | `jsh` |
| `$HOME` | Home directory (environment variable) | `/home/jswan` |
| `$PWD` | Current working directory (environment variable) | `/home/jswan/projects` |
| `$PATH` | Executable search path (environment variable) | `/usr/bin:/bin` |
| `$USER` | Current username (environment variable) | `jswan` |

The first three (`$?`, `$$`, `$0`) are **shell-managed** -- they live in the shell's state, not in the environment. The rest are regular environment variables that happen to be conventionally set.

### Storing shell state

We need our `Shell` struct to track these:

```rust
pub struct Shell {
    pub last_exit_code: i32,   // $?
    // $$ is std::process::id() -- always available
    // $0 is hardcoded to "jsh"
}
```

When calling the expander, pass this state along:

```rust
let expanded_args = expander::expand(cmd.args, self.last_exit_code);
```

---

## Concept 4: Tilde Expansion

The `~` character at the start of a word expands to the user's home directory:

| Input | Expands to |
|-------|-----------|
| `~` | `/home/jswan` |
| `~/projects` | `/home/jswan/projects` |
| `~jswan` | `/home/jswan` (lookup another user -- optional) |
| `foo~bar` | `foo~bar` (no expansion -- `~` not at word start) |

### Important rules

1. Tilde expansion **only happens at the start of a word** (or after `=` in an assignment)
2. `~` inside quotes is **not expanded** -- `"~"` stays as literal `~`
3. On Windows, the home directory comes from `USERPROFILE` instead of `HOME`

### Implementation

```rust
fn expand_tilde(token: &str) -> String {
    if !token.starts_with('~') {
        return token.to_string();
    }

    let home = get_home_dir();

    if token == "~" {
        return home;
    }

    if token.starts_with("~/") || token.starts_with("~\\") {
        return format!("{}{}", home, &token[1..]);
    }

    // ~username expansion (optional, Unix-only)
    // For now, just return the token unchanged if it's ~someuser
    token.to_string()
}

fn get_home_dir() -> String {
    // Try HOME first (Unix convention), then USERPROFILE (Windows)
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "~".to_string())
}
```

### Cross-platform note

On Windows, home directories live at `C:\Users\jswan`. The `USERPROFILE` environment variable points there. On Unix, `HOME` is `/home/jswan`. Our `get_home_dir()` function tries both, making this work cross-platform without conditional compilation.

---

## Concept 5: Glob/Wildcard Expansion

Glob expansion turns patterns like `*.rs` into a list of matching filenames. This is one of the shell's most useful features -- it means programs don't need to understand wildcards themselves.

### Glob patterns

| Pattern | Matches | Example |
|---------|---------|---------|
| `*` | Any sequence of characters (not `/`) | `*.rs` matches `main.rs`, `lib.rs` |
| `?` | Any single character | `?.rs` matches `a.rs` but not `ab.rs` |
| `[abc]` | Any one of the listed characters | `[ml]*.rs` matches `main.rs`, `lib.rs` |
| `[a-z]` | Any character in the range | `[a-c]*.rs` matches `a.rs`, `b_file.rs` |
| `**` | Any number of directories (recursive) | `**/*.rs` matches `src/main.rs`, `src/parser/mod.rs` |

### Using the `glob` crate

Implementing glob matching from scratch is educational but tedious. The `glob` crate handles it correctly, including cross-platform path separators:

```toml
# Cargo.toml
[dependencies]
glob = "0.3"
```

```rust
use glob::glob;

fn expand_globs(token: &str) -> Vec<String> {
    // Only try glob expansion if the token contains glob characters
    if !contains_glob_chars(token) {
        return vec![token.to_string()];
    }

    match glob(token) {
        Ok(paths) => {
            let matches: Vec<String> = paths
                .filter_map(|entry| entry.ok())
                .map(|path| path.to_string_lossy().into_owned())
                .collect();

            if matches.is_empty() {
                // No matches -- bash keeps the pattern literal
                // zsh would report an error
                // We follow bash behavior
                vec![token.to_string()]
            } else {
                matches
            }
        }
        Err(_) => vec![token.to_string()],
    }
}

fn contains_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}
```

### Key behavior: no matches

When a glob pattern matches nothing, shells differ:

- **bash:** Keeps the literal pattern (`echo *.xyz` prints `*.xyz` if no `.xyz` files exist)
- **zsh:** Reports an error ("no matches found")
- **Our shell:** We follow bash behavior -- it is more forgiving for interactive use

### Glob expansion creates multiple tokens

This is what makes glob expansion different from other expansions. A single input token like `*.rs` can expand into many output tokens:

```
Input tokens:  ["echo", "*.rs"]
After glob:    ["echo", "Cargo.toml", "main.rs", "lib.rs"]
                         ^-- one token became three!
```

This is why our `expand_globs` function returns `Vec<String>` -- and why the overall expander uses `result.extend(globbed)` instead of `result.push(expanded)`.

---

## Concept 6: Single Quotes vs Double Quotes During Expansion

This is where the quoting rules from Module 2 become critical. The parser already tracks whether text was quoted, but now the *kind* of quoting affects whether expansion happens.

| Context | Variable expansion? | Tilde expansion? | Glob expansion? | Escape sequences? |
|---------|:------------------:|:----------------:|:---------------:|:-----------------:|
| Unquoted | Yes | Yes | Yes | Yes |
| Double quotes `"..."` | Yes | No | No | Yes (`\"`, `\\`, `\$`, `` \` ``) |
| Single quotes `'...'` | No | No | No | No (everything literal) |

### Why this matters

```bash
FILE="hello world.txt"

echo $FILE        # After expansion + word splitting: echo hello world.txt
                  # (two arguments: "hello" and "world.txt")

echo "$FILE"      # Quoted: echo "hello world.txt"
                  # (one argument: "hello world.txt")

echo '$FILE'      # Single-quoted: echo '$FILE'
                  # (one argument: literal "$FILE")
```

### Tracking quote context through the expansion pipeline

The parser from Module 2 needs to annotate tokens with their quote context. We can do this with an enum:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum QuoteContext {
    Unquoted,
    DoubleQuoted,
    SingleQuoted,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub text: String,
    pub quote_context: QuoteContext,
}
```

However, a single "word" in shell can contain mixed quoting:

```bash
echo he"ll"o'world'    # One argument: "helloworld"
```

A more precise approach is to represent each word as a sequence of **segments**, each with its own quote context:

```rust
#[derive(Debug, Clone)]
pub enum WordSegment {
    Unquoted(String),
    DoubleQuoted(String),
    SingleQuoted(String),
}

// A single "word" (argument) is made of segments
pub type Word = Vec<WordSegment>;
```

Then the expander processes each segment according to its quote type:

```rust
fn expand_word(segments: &[WordSegment], last_exit_code: i32) -> Vec<String> {
    let mut combined = String::new();
    let mut has_glob = false;

    for segment in segments {
        match segment {
            WordSegment::SingleQuoted(text) => {
                // No expansion at all -- literal text
                combined.push_str(text);
            }
            WordSegment::DoubleQuoted(text) => {
                // Variable expansion only, no glob, no word split
                let expanded = expand_variables(text, last_exit_code);
                combined.push_str(&expanded);
            }
            WordSegment::Unquoted(text) => {
                // Full expansion: tilde, variables, globs
                let expanded = expand_tilde(text);
                let expanded = expand_variables(&expanded, last_exit_code);
                combined.push_str(&expanded);
                has_glob = has_glob || contains_glob_chars(&expanded);
            }
        }
    }

    if has_glob {
        expand_globs(&combined)
    } else {
        vec![combined]
    }
}
```

---

## Concept 7: Word Splitting After Expansion

After variable expansion, unquoted results are split on whitespace (or more precisely, on the characters in the `IFS` variable, which defaults to space, tab, and newline).

### Why word splitting exists

Consider:

```bash
FILES="main.rs lib.rs"
ls $FILES
```

After variable expansion, this becomes `ls main.rs lib.rs` -- but that is a single token `"main.rs lib.rs"`. Word splitting breaks it into two separate arguments so `ls` sees two files.

### When word splitting does NOT happen

- **Inside double quotes:** `"$FILES"` stays as one argument `"main.rs lib.rs"`
- **Inside single quotes:** Not applicable (no expansion happens)
- **On literal text:** Only text produced by expansion gets split

### Implementation

```rust
fn word_split(text: &str) -> Vec<String> {
    // IFS defaults to space, tab, newline
    // For now, we just split on whitespace (same effect)
    text.split_whitespace()
        .map(|s| s.to_string())
        .collect()
}
```

In practice, word splitting interacts with glob expansion and quoting in subtle ways. A full implementation tracks which parts of a string came from expansion (and should be split) versus which parts were literal (and should not). For our shell, we can start with the simpler approach and refine later.

---

## Concept 8: Putting the Expansion Pipeline Together

Here is the complete expansion module that processes a parsed command:

```rust
// src/expander.rs

use glob::glob;

/// Expand a list of words (from the parser) into final argument strings.
/// Each word is a Vec<WordSegment> representing mixed quoting.
pub fn expand_words(words: Vec<Word>, last_exit_code: i32) -> Vec<String> {
    let mut final_args = Vec::new();

    for word in words {
        let expanded = expand_word(&word, last_exit_code);
        final_args.extend(expanded);
    }

    final_args
}

fn expand_word(segments: &[WordSegment], last_exit_code: i32) -> Vec<String> {
    let mut combined = String::new();
    let mut is_globbable = false;
    let mut is_splittable = false;

    for segment in segments {
        match segment {
            WordSegment::SingleQuoted(text) => {
                combined.push_str(text);
                // No expansion, no splitting, no globbing
            }
            WordSegment::DoubleQuoted(text) => {
                let expanded = expand_variables(text, last_exit_code);
                combined.push_str(&expanded);
                // Variables expanded, but no globbing or word splitting
            }
            WordSegment::Unquoted(text) => {
                let expanded = expand_tilde(text);
                let expanded = expand_variables(&expanded, last_exit_code);
                combined.push_str(&expanded);
                is_globbable = is_globbable || contains_glob_chars(&expanded);
                is_splittable = true;
            }
        }
    }

    // Step 1: Word splitting (only on unquoted expansion results)
    let words = if is_splittable && combined.contains(char::is_whitespace) {
        word_split(&combined)
    } else {
        vec![combined]
    };

    // Step 2: Glob expansion (only on unquoted text)
    if is_globbable {
        words.into_iter()
            .flat_map(|w| expand_globs(&w))
            .collect()
    } else {
        words
    }
}
```

### Integrating into the shell loop

The main REPL loop now looks like:

```rust
loop {
    let input = read_input()?;
    let words = parser::parse(&input);            // Module 2: tokenize & parse
    let args = expander::expand_words(            // Module 5: expand
        words,
        shell.last_exit_code,
    );
    if args.is_empty() {
        continue;
    }
    shell.execute(&args);                         // Module 3+4: run
}
```

### The full picture as a diagram

```
  "echo ~/*.rs $HOME '$HOME'"
          |
          v
  +-----------------+
  |   Tokenizer     |  Module 2: handles quotes, escapes
  +-----------------+
          |
  [Word(Unquoted("echo")),
   Word(Unquoted("~/*.rs")),
   Word(DoubleQuoted("$HOME")),      <-- if double-quoted
   Word(SingleQuoted("$HOME"))]
          |
          v
  +-----------------+
  |  Tilde Expand   |  ~ --> /home/jswan (unquoted only)
  +-----------------+
          |
  +-----------------+
  | Variable Expand |  $HOME --> /home/jswan (unquoted + double-quoted)
  +-----------------+
          |
  +-----------------+
  |  Glob Expand    |  /home/jswan/*.rs --> [main.rs, lib.rs] (unquoted only)
  +-----------------+
          |
  +-----------------+
  |  Word Split     |  split on IFS (unquoted only)
  +-----------------+
          |
          v
  ["echo", "/home/jswan/main.rs", "/home/jswan/lib.rs",
   "/home/jswan", "$HOME"]
```

Notice: the double-quoted `$HOME` expanded to `/home/jswan`, but the single-quoted `$HOME` stayed as literal `$HOME`.

---

## Key Rust concepts used

- **Enums with data (`WordSegment`)** -- each variant holds a `String`, letting us tag text with its quote context
- **`Vec<Vec<...>>` flattening** -- glob expansion turns one word into many, requiring `flat_map` and `extend`
- **Iterator adapters** -- `filter_map`, `flat_map`, `map`, `collect` to chain transformations
- **`std::env::var()`** -- reading environment variables, returns `Result<String, VarError>`
- **The `glob` crate** -- cross-platform pathname pattern matching
- **`String::push_str` and `format!`** -- building strings incrementally
- **Pattern matching on `&str`** -- dispatching special variables (`$?`, `$$`, `$0`) vs environment lookups

---

## Milestone

After implementing expansion, your shell should handle these scenarios:

```
jsh> echo $HOME
/home/jswan

jsh> echo ${HOME}
/home/jswan

jsh> echo "$HOME"
/home/jswan

jsh> echo '$HOME'
$HOME

jsh> echo ~
/home/jswan

jsh> echo ~/projects
/home/jswan/projects

jsh> echo *.rs
main.rs lib.rs

jsh> echo "*.rs"
*.rs

jsh> echo $?
0

jsh> nonexistent
jsh: command not found: nonexistent
jsh> echo $?
127

jsh> echo $$
12345

jsh> echo $0
jsh

jsh> export GREETING=hello
jsh> echo $GREETING world
hello world

jsh> echo ${GREETING}_world
hello_world

jsh> echo $UNDEFINED

jsh>
```

### Behavior Notes
- Tilde expansion only applies to `~`, `~/`, or `~\\` (no `~user` support yet).
- Glob expansion only triggers when glob characters come from unquoted text.
- Word splitting happens only for unquoted expansions; quoted expansions stay as one arg.
- `$?`, `$$`, and `$0` are supported and update on each command.

On Windows, the behavior is identical except that `~` expands to `C:\Users\jswan` (or wherever `USERPROFILE` points) and glob patterns use backslash-style paths.

---

## What's next?

Module 6 adds **I/O redirection** -- sending command output to files (`>`, `>>`), reading input from files (`<`), and redirecting stderr (`2>`). That is when `ls > files.txt` starts working.
