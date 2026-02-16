# Module 0: Foundations & Prerequisites

This module covers the Rust fundamentals you need before building a shell. If you already know another programming language, great -- this will map those concepts into Rust's world. If any section feels unfamiliar by the end, spend time with it before moving to Module 1. A shaky foundation makes everything harder later.

The goal: after this module, you can comfortably write a CLI tool that reads files, parses arguments, handles errors gracefully, and uses Rust's type system instead of fighting it.

---

## 1. Ownership & Borrowing

This is the single most important concept in Rust. Every other language lets you ignore memory management (garbage collector) or forces you to manage it manually (C/C++). Rust does something different: the compiler tracks who "owns" each piece of data and enforces strict rules at compile time. No garbage collector. No manual free. No use-after-free bugs.

### What ownership is

Every value in Rust has exactly one owner -- the variable that holds it. When that owner goes out of scope, the value is dropped (its memory is freed).

```rust
fn main() {
    let name = String::from("james-shell"); // `name` owns this String
    println!("{name}");
} // `name` goes out of scope here. The String's memory is freed.
```

There is no garbage collector running in the background. There is no `free()` call you need to remember. The compiler inserts the cleanup code for you, based on scope.

### Move semantics vs Copy

Here is where most newcomers get their first compiler error. When you assign a `String` to another variable, the original is **moved**, not copied:

```rust
fn main() {
    let a = String::from("hello");
    let b = a; // `a` is MOVED into `b`. `a` is now invalid.

    // println!("{a}"); // COMPILE ERROR: value used after move
    println!("{b}");    // This works fine. `b` owns the data now.
}
```

Why? A `String` is heap-allocated. If Rust copied it silently, you would get two owners pointing at the same memory, and both would try to free it when they go out of scope. That is a double-free bug. Rust prevents it by making the move explicit.

But simple types like integers, booleans, and floats implement the `Copy` trait. They live entirely on the stack and are cheap to duplicate:

```rust
fn main() {
    let x: i32 = 42;
    let y = x; // `x` is COPIED, not moved. Both are valid.

    println!("x = {x}, y = {y}"); // Both work fine.
}
```

The rule: types that implement `Copy` are duplicated on assignment. Types that do not (like `String`, `Vec`, `HashMap`) are moved.

### References and borrowing

Moving ownership everywhere is impractical. Most of the time you want to *lend* a value to a function without giving it away. That is what references are for.

```rust
fn print_length(s: &String) {
    // `s` is a reference to a String. It borrows the data, does not own it.
    println!("Length: {}", s.len());
} // `s` goes out of scope, but since it doesn't own the data, nothing is freed.

fn main() {
    let name = String::from("james-shell");
    print_length(&name); // Lend `name` to the function.
    println!("{name}");  // Still valid! We only lent it, didn't give it away.
}
```

The `&` creates a reference. The function borrows the value but cannot modify it.

### Mutable references

If you need to modify borrowed data, use `&mut`:

```rust
fn add_exclamation(s: &mut String) {
    s.push('!');
}

fn main() {
    let mut greeting = String::from("hello");
    add_exclamation(&mut greeting);
    println!("{greeting}"); // "hello!"
}
```

Note: the variable itself must be declared `mut`, and the reference must be `&mut`.

### The borrowing rules

Rust enforces two rules at compile time:

1. **You can have many immutable references (`&T`) at the same time, OR one mutable reference (`&mut T`) -- never both.**
2. **References must always be valid (no dangling pointers).**

```rust
fn main() {
    let mut data = String::from("hello");

    let r1 = &data;     // OK: first immutable borrow
    let r2 = &data;     // OK: second immutable borrow (many readers allowed)
    println!("{r1} {r2}");

    let r3 = &mut data; // OK: mutable borrow starts AFTER r1 and r2 are done
    r3.push_str(" world");
    println!("{r3}");
}
```

This would fail:

```rust
fn main() {
    let mut data = String::from("hello");

    let r1 = &data;        // Immutable borrow starts
    let r2 = &mut data;    // COMPILE ERROR: cannot borrow as mutable while
                           // also borrowed as immutable
    println!("{r1}");
}
```

Why does this rule exist? It prevents data races at compile time. If one part of your code is reading data while another part is modifying it, you get bugs. Rust makes this impossible.

### Common ownership errors and how to fix them

**Error: "value used after move"**

```rust
fn take_ownership(s: String) {
    println!("{s}");
}

fn main() {
    let name = String::from("shell");
    take_ownership(name);
    // println!("{name}"); // ERROR: `name` was moved into the function
}
```

Fix 1 -- pass a reference instead:

```rust
fn borrow_it(s: &String) {
    println!("{s}");
}

fn main() {
    let name = String::from("shell");
    borrow_it(&name);
    println!("{name}"); // Works! We only lent it.
}
```

Fix 2 -- clone the value (makes a full copy):

```rust
fn take_ownership(s: String) {
    println!("{s}");
}

fn main() {
    let name = String::from("shell");
    take_ownership(name.clone()); // Give the function a copy
    println!("{name}");           // Original is still valid
}
```

### When to use .clone() vs borrowing

Use borrowing (`&`) when the function only needs to read the data. This is the common case and costs nothing at runtime.

Use `.clone()` when you genuinely need two independent copies of the data -- for example, storing a copy in a struct while the original continues to be used elsewhere. Cloning heap-allocated types like `String` and `Vec` involves a memory allocation, so do not clone out of laziness. But do not bend your code into pretzels to avoid a clone either. If cloning makes the code clear and the data is small, just clone it.

A good rule of thumb for our shell project: if a function needs to *store* a string (like putting a command name into a struct), it should take ownership (`String`). If it only needs to *look at* a string (like parsing or printing), it should borrow (`&str`).

### Exercise 1.1

Predict whether each snippet compiles. Then test it with `cargo run`:

```rust
// Snippet A
let a = String::from("hello");
let b = a;
println!("{a}");

// Snippet B
let a = String::from("hello");
let b = &a;
println!("{a} {b}");

// Snippet C
let mut a = String::from("hello");
let b = &a;
a.push_str(" world");
println!("{b}");

// Snippet D
let x = 5;
let y = x;
println!("{x} {y}");
```

---

## 2. Error Handling

Shells interact with the file system, run external programs, parse untrusted user input, and deal with missing environment variables. Things go wrong constantly. Rust's error handling is built into the type system -- you cannot ignore errors without making a deliberate choice.

### Result<T, E> and Option<T>

These are Rust's two core types for representing "something might fail" or "something might not exist."

```rust
// Result: an operation that can succeed or fail
enum Result<T, E> {
    Ok(T),   // Success, carrying a value of type T
    Err(E),  // Failure, carrying an error of type E
}

// Option: a value that might or might not exist
enum Option<T> {
    Some(T), // The value exists
    None,    // No value
}
```

They are just enums. There is no magic. They are not exceptions, not null pointers, not special compiler features. They are enums that you pattern match on.

```rust
use std::fs;

fn main() {
    // fs::read_to_string returns Result<String, io::Error>
    let result = fs::read_to_string("Cargo.toml");

    match result {
        Ok(contents) => println!("File has {} bytes", contents.len()),
        Err(error) => println!("Failed to read file: {error}"),
    }
}
```

```rust
fn find_user(name: &str) -> Option<String> {
    if name == "james" {
        Some(String::from("James Swan"))
    } else {
        None
    }
}

fn main() {
    match find_user("james") {
        Some(full_name) => println!("Found: {full_name}"),
        None => println!("User not found"),
    }
}
```

### The ? operator

Writing `match` every time gets tedious. The `?` operator is syntactic sugar: if the `Result` is `Ok`, unwrap the value. If it is `Err`, return the error from the current function immediately.

```rust
use std::fs;
use std::io;

fn read_config() -> Result<String, io::Error> {
    let contents = fs::read_to_string("config.toml")?; // If error, return it
    Ok(contents)
}

// The ? above is equivalent to:
fn read_config_verbose() -> Result<String, io::Error> {
    let contents = match fs::read_to_string("config.toml") {
        Ok(c) => c,
        Err(e) => return Err(e),
    };
    Ok(contents)
}
```

The `?` operator only works inside functions that return `Result` (or `Option`). It propagates errors upward to the caller, who then decides how to handle them. This is how you build layered error handling: low-level functions propagate with `?`, and the top-level function (like `main`) decides what to show the user.

```rust
use std::io::{self, Write};

fn main() -> Result<(), io::Error> {
    // Now main returns a Result, so we can use ? here
    let mut stdout = io::stdout();
    stdout.write_all(b"jsh> ")?;
    stdout.flush()?;
    Ok(())
}
```

### .unwrap() vs .expect() vs proper matching

```rust
// .unwrap() -- crash if it's an Err/None. No context.
let contents = fs::read_to_string("file.txt").unwrap();

// .expect() -- crash with a message. Better for debugging.
let contents = fs::read_to_string("file.txt")
    .expect("Failed to read file.txt");

// Pattern matching -- handle the error properly.
let contents = match fs::read_to_string("file.txt") {
    Ok(c) => c,
    Err(e) => {
        eprintln!("Error reading file: {e}");
        return;
    }
};
```

Rule of thumb for the shell project:
- **Use `?`** in functions that propagate errors up (most functions).
- **Use `.expect()`** only during initial setup where failure means the program cannot continue (e.g., setting up signal handlers).
- **Use `match`** when you want to handle the error right there (e.g., "command not found" should print a message, not crash).
- **Never use `.unwrap()` in production code.** It gives no context when it crashes.

### When to panic vs when to return errors

`panic!()` crashes the program immediately. It is for bugs, not expected failures.

- **Panic** when something is logically impossible: an index that should always be valid, an invariant that is violated. These indicate a programming error.
- **Return `Err`** for anything that can legitimately happen at runtime: file not found, permission denied, invalid user input, network timeout.

In a shell, almost everything should return errors, not panic. The user typed a bad command? Print an error, show the prompt again. A file does not exist? Tell them. The shell should be hard to crash.

### Custom error types with enums

As your shell grows, you will have many kinds of errors. Define an enum:

```rust
#[derive(Debug)]
enum ShellError {
    CommandNotFound(String),
    ParseError(String),
    IoError(std::io::Error),
    PermissionDenied(String),
}

// Convert io::Error into ShellError automatically
impl From<std::io::Error> for ShellError {
    fn from(error: std::io::Error) -> Self {
        ShellError::IoError(error)
    }
}

fn run_command(input: &str) -> Result<(), ShellError> {
    if input.is_empty() {
        return Err(ShellError::ParseError("Empty input".to_string()));
    }
    // ... now the ? operator on io::Error works because of the From impl
    Ok(())
}
```

### Combinators on Result and Option

Instead of matching every time, you can chain operations:

```rust
use std::env;

// .map() -- transform the Ok/Some value
let port: Option<u16> = env::var("PORT")
    .ok()                          // Result -> Option (discard the error)
    .map(|s| s.parse::<u16>())     // Option<Result<u16, _>>
    .and_then(|r| r.ok());         // Flatten: Option<u16>

// .unwrap_or_else() -- provide a fallback on error
let home = env::var("HOME")
    .unwrap_or_else(|_| String::from("/tmp"));

// .and_then() -- chain operations that might fail (flatMap)
let config: Option<String> = env::var("SHELL_CONFIG")
    .ok()
    .and_then(|path| std::fs::read_to_string(path).ok());

// .map_err() -- transform the error
let contents = std::fs::read_to_string("config.toml")
    .map_err(|e| format!("Config error: {e}"))?;
```

These combinators are especially useful when parsing shell input, where you often have chains of "try this, if it works transform it, otherwise fall back."

### Exercise 2.1

Write a function with this signature:

```rust
fn read_first_line(path: &str) -> Result<String, std::io::Error>
```

It should open a file, read the first line, and return it (without the trailing newline). Use `?` to propagate errors. Test it with a file that exists and one that does not.

---

## 3. Pattern Matching

Pattern matching in Rust is not just a fancy `switch` statement. It is exhaustive (the compiler ensures you handle every case), it can destructure data, and it is used everywhere -- from error handling to parsing shell commands.

### match expressions

```rust
fn describe_exit_code(code: i32) -> &'static str {
    match code {
        0 => "success",
        1 => "general error",
        2 => "misuse of shell builtin",
        126 => "command not executable",
        127 => "command not found",
        128..=255 => "killed by signal",
        _ => "unknown",
    }
}
```

The compiler requires that `match` is **exhaustive** -- every possible value must be covered. The `_` wildcard matches anything not already matched. If you forget a case, the compiler tells you.

### if let and while let

When you only care about one variant and want to ignore the rest, `if let` is cleaner than a full `match`:

```rust
let maybe_home = std::env::var("HOME").ok(); // Option<String>

// Instead of:
match maybe_home {
    Some(home) => println!("Home is {home}"),
    None => {} // Do nothing
}

// Use if let:
if let Some(home) = std::env::var("HOME").ok() {
    println!("Home is {home}");
}
```

`while let` is the loop version -- keep going as long as the pattern matches:

```rust
let mut stack = vec![1, 2, 3, 4, 5];

// Pop values off the stack until it's empty
while let Some(top) = stack.pop() {
    println!("Got: {top}");
}
// Prints: 5, 4, 3, 2, 1
```

This pattern is useful when reading lines from stdin -- keep reading while there is input.

### Destructuring structs, enums, and tuples

```rust
// Destructuring a tuple
let (command, args) = ("ls", vec!["-la", "/tmp"]);
println!("Running {command} with {args:?}");

// Destructuring a struct
struct Command {
    program: String,
    args: Vec<String>,
}

let cmd = Command {
    program: String::from("echo"),
    args: vec![String::from("hello")],
};

let Command { program, args } = cmd;
println!("Program: {program}, Args: {args:?}");

// Destructuring an enum
enum Token {
    Word(String),
    Pipe,
    Redirect { fd: i32, target: String },
}

fn describe_token(token: &Token) {
    match token {
        Token::Word(w) => println!("Word: {w}"),
        Token::Pipe => println!("Pipe operator"),
        Token::Redirect { fd, target } => {
            println!("Redirect fd {fd} to {target}")
        }
    }
}
```

### Match guards

Add extra conditions to a match arm with `if`:

```rust
fn classify_char(c: char) -> &'static str {
    match c {
        ' ' | '\t' | '\n' => "whitespace",
        '"' | '\'' => "quote",
        '|' => "pipe",
        '>' | '<' => "redirect",
        '\\' => "escape",
        c if c.is_alphanumeric() => "word character",
        _ => "special",
    }
}
```

Match guards are essential when parsing shell input -- you will use them to classify characters in your tokenizer.

### Nested patterns

Patterns can be nested to match complex structures:

```rust
enum Redirect {
    File(String),
    Fd(i32),
}

enum CommandPart {
    Simple { program: String, args: Vec<String> },
    Pipeline(Vec<String>),
    WithRedirect { program: String, output: Redirect },
}

fn describe(part: &CommandPart) {
    match part {
        CommandPart::Simple { program, args } if args.is_empty() => {
            println!("{program} (no arguments)");
        }
        CommandPart::Simple { program, args } => {
            println!("{program} with {} args", args.len());
        }
        CommandPart::Pipeline(commands) => {
            println!("Pipeline of {} commands", commands.len());
        }
        CommandPart::WithRedirect {
            program,
            output: Redirect::File(path),
        } => {
            println!("{program} > {path}");
        }
        CommandPart::WithRedirect {
            program,
            output: Redirect::Fd(fd),
        } => {
            println!("{program} >&{fd}");
        }
    }
}
```

### Exercise 3.1

Write a function that takes an `Option<i32>` and returns a string:
- `Some(0)` -> "success"
- `Some(n)` where n is positive -> "error code: {n}"
- `Some(n)` where n is negative -> "invalid: {n}"
- `None` -> "no exit code"

Use `match` with guards.

---

## 4. Enums & Structs

Enums and structs are how you model data in Rust. In our shell, a `Command` struct holds parsed command data, and an enum like `Token` represents the different pieces of parsed input.

### Enums with data

Rust enums can carry data in each variant, unlike C-style enums:

```rust
// Each variant can hold different data
enum Token {
    Word(String),                          // A single string
    Pipe,                                  // No data
    Redirect { fd: i32, file: String },    // Named fields (like a struct)
    Background,                            // No data
    Semicolon,                             // No data
}

// Using the enum
let tokens = vec![
    Token::Word(String::from("ls")),
    Token::Pipe,
    Token::Word(String::from("grep")),
    Token::Word(String::from("foo")),
    Token::Redirect { fd: 1, file: String::from("output.txt") },
];
```

This is sometimes called a "tagged union" or "sum type." It is one of Rust's most powerful features. Instead of using inheritance hierarchies (like in Java or Python), you model variants as enum cases.

### Structs and impl blocks

```rust
struct Command {
    program: String,
    args: Vec<String>,
    background: bool,
}

impl Command {
    // Associated function (like a static method). Called with Command::new()
    fn new(program: String) -> Self {
        Command {
            program,
            args: Vec::new(),
            background: false,
        }
    }

    // Method (takes &self). Called with cmd.arg_count()
    fn arg_count(&self) -> usize {
        self.args.len()
    }

    // Mutable method (takes &mut self). Called with cmd.add_arg(...)
    fn add_arg(&mut self, arg: String) {
        self.args.push(arg);
    }

    // Method that takes ownership (takes self). Consumes the struct.
    fn into_parts(self) -> (String, Vec<String>) {
        (self.program, self.args)
    }
}

fn main() {
    let mut cmd = Command::new(String::from("echo"));
    cmd.add_arg(String::from("hello"));
    cmd.add_arg(String::from("world"));
    println!("{} has {} args", cmd.program, cmd.arg_count());
}
```

Notice the three flavors of `self`:
- `&self` -- borrow the struct immutably (read-only access)
- `&mut self` -- borrow the struct mutably (can modify it)
- `self` -- take ownership (the struct is consumed, cannot be used after)

### Derive macros

The `#[derive(...)]` attribute auto-generates trait implementations:

```rust
#[derive(Debug, Clone, PartialEq)]
struct Command {
    program: String,
    args: Vec<String>,
}

fn main() {
    let cmd = Command {
        program: String::from("ls"),
        args: vec![String::from("-la")],
    };

    // Debug: enables {:?} formatting
    println!("{cmd:?}");
    // Output: Command { program: "ls", args: ["-la"] }

    // Clone: enables .clone() to make a deep copy
    let cmd2 = cmd.clone();

    // PartialEq: enables == comparison
    assert_eq!(cmd, cmd2);
}
```

What each derive does:
- **`Debug`** -- enables `{:?}` and `{:#?}` (pretty-print) formatting. Essential for development.
- **`Clone`** -- enables `.clone()` to make a deep copy. Derive it when you need to duplicate your type.
- **`PartialEq`** -- enables `==` and `!=` comparisons. Essential for testing with `assert_eq!`.
- **`Default`** -- enables `Type::default()` to create a value with all fields set to their defaults (0, empty string, false, etc.).

### When to use enum vs struct

- Use a **struct** when you have a thing with multiple fields that are all present at the same time (a parsed command has a program *and* arguments *and* redirections).
- Use an **enum** when you have a thing that can be one of several different kinds (a token is *either* a word *or* a pipe *or* a redirect).

### Option and Result are just enums

There is nothing special about them. They are defined in the standard library as regular enums:

```rust
// This is literally how Option is defined:
enum Option<T> {
    Some(T),
    None,
}

// And Result:
enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

The `?` operator and methods like `.map()` are implemented as normal methods on these enums. Understanding this demystifies a lot of Rust.

### Exercise 4.1

Define an enum `Builtin` with variants: `Cd(String)`, `Exit(Option<i32>)`, `Pwd`, `Echo(Vec<String>)`. Write a function `execute_builtin(cmd: &Builtin)` that pattern-matches on it and prints what it would do (e.g., "Changing directory to /tmp").

---

## 5. Strings

Strings will trip you up more than any other topic in Rust. There are two string types, and understanding when to use each is critical.

### String vs &str

```rust
let owned: String = String::from("hello");  // Heap-allocated, growable, owned
let borrowed: &str = "hello";               // Reference to string data, not owned
```

Think of it like `Vec<u8>` vs `&[u8]`. `String` owns its data and can grow. `&str` is a borrowed view into string data that already exists somewhere.

| | `String` | `&str` |
|---|---|---|
| Ownership | Owns the data | Borrows from someone else |
| Mutability | Can be modified (push, insert) | Read-only |
| Storage | Heap-allocated | Points to data anywhere (heap, stack, binary) |
| Size | Known at runtime, can grow | Known at compile time (for literals) or runtime |
| Cost to create | Allocates memory | Free (just a pointer + length) |

**When to use which in our shell:**
- **`String`** when you need to store and own the data: fields in structs, elements in vectors, return values from parsing.
- **`&str`** when you just need to read or inspect a string: function parameters that only look at the data, match arms, comparisons.

```rust
// Function that only reads: take &str
fn is_builtin(command: &str) -> bool {
    matches!(command, "cd" | "exit" | "pwd" | "echo" | "export")
}

// Struct that stores data: use String
struct ParsedCommand {
    program: String,
    args: Vec<String>,
}

// Common pattern: accept &str, return String
fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}{}", &path[1..])
    } else {
        path.to_string()
    }
}
```

### Why you cannot index a String

This does not compile:

```rust
let s = String::from("hello");
// let c = s[0]; // COMPILE ERROR: String cannot be indexed by integer
```

Why? Rust strings are UTF-8 encoded. Characters can be 1 to 4 bytes long. `s[0]` is ambiguous: do you want the first *byte*, the first *character*, or the first *grapheme cluster*? Rust makes you be explicit.

```rust
let greeting = String::from("hello");

// Iterate over characters (Unicode scalar values)
for c in greeting.chars() {
    print!("{c} "); // h e l l o
}

// Iterate over bytes
for b in greeting.bytes() {
    print!("{b} "); // 104 101 108 108 111
}

// Get the first character explicitly
let first: Option<char> = greeting.chars().next(); // Some('h')
```

For our shell, we mostly deal with ASCII input (commands, paths, arguments), so this rarely causes problems in practice. But it is good to know why `s[0]` does not work.

### Useful string methods

```rust
let input = String::from("  echo hello world  ");

// Trimming whitespace
let trimmed: &str = input.trim(); // "echo hello world"

// Splitting
let parts: Vec<&str> = trimmed.split_whitespace().collect();
// ["echo", "hello", "world"]

// Checking prefixes/suffixes
trimmed.starts_with("echo"); // true
trimmed.ends_with("world");  // true

// Finding substrings
trimmed.contains("hello"); // true
trimmed.find("hello");     // Some(5) -- byte index

// Converting between String and &str
let owned: String = trimmed.to_string(); // &str -> String (allocates)
let borrowed: &str = owned.as_str();     // String -> &str (free)

// Building strings
let msg = format!("Running: {} with args {:?}", "ls", vec!["-la"]);
```

### String slicing

You can slice a string with byte ranges:

```rust
let s = String::from("hello world");
let hello: &str = &s[0..5];   // "hello"
let world: &str = &s[6..11];  // "world"
```

**Warning:** slicing on a non-UTF-8-character boundary panics at runtime:

```rust
let s = String::from("cafe\u{0301}"); // "caf??" with combining accent
// &s[0..5] would panic if it cuts a multi-byte character in half
```

For our shell, byte-based slicing is usually fine since we deal with ASCII commands. But always be aware of this when working with user input that might contain Unicode.

### String formatting

```rust
let name = "james-shell";
let version = 1;

// println! prints to stdout with a newline
println!("Welcome to {name} v{version}");

// eprintln! prints to stderr with a newline (for error messages)
eprintln!("Error: command not found");

// format! returns a String (no printing)
let prompt = format!("{name}> ");

// print! prints to stdout without a newline (needs flush for interactive use)
print!("jsh> ");

// Debug formatting with {:?}
let args = vec!["ls", "-la"];
println!("Args: {args:?}");     // Args: ["ls", "-la"]
println!("Args: {args:#?}");    // Pretty-printed, multi-line
```

### Exercise 5.1

Write a function `split_first_word(input: &str) -> (&str, &str)` that splits a string into the first word and the rest. Leading whitespace should be stripped. For example:
- `"  echo hello world"` -> `("echo", "hello world")`
- `"ls"` -> `("ls", "")`
- `""` -> `("", "")`

Use `str` methods like `.trim()`, `.find()`, and slicing. No `split_whitespace()` allowed.

---

## 6. Collections

Vectors, hash maps, and iterators are the workhorses of any shell implementation. You will use them to store token lists, argument vectors, environment variables, and more.

### Vec<T>

A growable array. This is the collection you will use most.

```rust
fn main() {
    // Creating vectors
    let mut args: Vec<String> = Vec::new();
    let numbers = vec![1, 2, 3, 4, 5]; // vec! macro for inline creation

    // Adding elements
    args.push(String::from("-la"));
    args.push(String::from("/tmp"));

    // Accessing elements
    let first: &String = &args[0];        // Panics if out of bounds
    let safe: Option<&String> = args.get(0); // Returns None if out of bounds

    // Length and emptiness
    println!("Length: {}", args.len());
    println!("Empty: {}", args.is_empty());

    // Iterating
    for arg in &args {
        println!("Arg: {arg}");
    }

    // Removing elements
    let last: Option<String> = args.pop(); // Removes and returns last element

    // Slicing -- borrow a portion
    let slice: &[String] = &args[0..1];

    // Collect from an iterator
    let words: Vec<&str> = "echo hello world".split_whitespace().collect();
}
```

### HashMap<K, V>

A key-value store. Perfect for environment variables, aliases, and function definitions.

```rust
use std::collections::HashMap;

fn main() {
    let mut env: HashMap<String, String> = HashMap::new();

    // Insert
    env.insert(String::from("HOME"), String::from("/home/james"));
    env.insert(String::from("PATH"), String::from("/usr/bin:/bin"));

    // Get (returns Option<&V>)
    if let Some(home) = env.get("HOME") {
        println!("Home: {home}");
    }

    // Check existence
    if env.contains_key("PATH") {
        println!("PATH is set");
    }

    // Remove
    env.remove("PATH");

    // The entry API -- insert only if key doesn't exist
    env.entry(String::from("SHELL"))
        .or_insert(String::from("/bin/jsh"));

    // Iterate over key-value pairs
    for (key, value) in &env {
        println!("{key}={value}");
    }
}
```

The **entry API** is particularly useful for things like counting word frequency:

```rust
use std::collections::HashMap;

fn word_frequency(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        let count = counts.entry(word).or_insert(0);
        *count += 1;
    }
    counts
}
```

### Iterators

Iterators are Rust's approach to processing sequences of values. They are lazy (nothing happens until you consume them) and composable (you chain operations together).

```rust
fn main() {
    let numbers = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

    // .map() -- transform each element
    let doubled: Vec<i32> = numbers.iter().map(|n| n * 2).collect();
    // [2, 4, 6, 8, 10, 12, 14, 16, 18, 20]

    // .filter() -- keep only elements that match a condition
    let evens: Vec<&i32> = numbers.iter().filter(|n| *n % 2 == 0).collect();
    // [2, 4, 6, 8, 10]

    // .enumerate() -- add an index to each element
    for (i, n) in numbers.iter().enumerate() {
        println!("[{i}] = {n}");
    }

    // .find() -- return the first match
    let first_even: Option<&i32> = numbers.iter().find(|n| *n % 2 == 0);

    // .any() and .all() -- boolean checks
    let has_zero = numbers.iter().any(|n| *n == 0);     // false
    let all_positive = numbers.iter().all(|n| *n > 0);  // true

    // .count()
    let even_count = numbers.iter().filter(|n| *n % 2 == 0).count(); // 5

    // .flat_map() -- map and flatten (useful for one-to-many transforms)
    let words = vec!["hello world", "foo bar"];
    let all_words: Vec<&str> = words.iter().flat_map(|s| s.split_whitespace()).collect();
    // ["hello", "world", "foo", "bar"]
}
```

### Chaining iterators

The real power is in chaining:

```rust
fn parse_path_dirs(path_var: &str) -> Vec<String> {
    path_var
        .split(':')                          // Split PATH by ':'
        .filter(|dir| !dir.is_empty())       // Skip empty entries (from "::")
        .map(|dir| dir.to_string())          // Convert &str to String
        .collect()                           // Collect into Vec<String>
}

fn main() {
    let path = "/usr/bin:/bin::/usr/local/bin:";
    let dirs = parse_path_dirs(path);
    println!("{dirs:?}");
    // ["/usr/bin", "/bin", "/usr/local/bin"]
}
```

Here is a more realistic example -- finding an executable in PATH:

```rust
use std::path::Path;

fn find_in_path(command: &str, path_var: &str) -> Option<String> {
    path_var
        .split(':')
        .map(|dir| format!("{dir}/{command}"))
        .find(|full_path| Path::new(full_path).exists())
}
```

### .iter() vs .into_iter() vs .iter_mut()

This is a common source of confusion:

```rust
let names = vec![String::from("ls"), String::from("grep"), String::from("cat")];

// .iter() -- borrows each element as &T. The original Vec is still usable.
for name in names.iter() {
    // `name` is &String here
    println!("{name}");
}
println!("Still have {} names", names.len()); // names still exists

// .into_iter() -- takes ownership of each element. The Vec is consumed.
for name in names.into_iter() {
    // `name` is String here (owned)
    println!("{name}");
}
// println!("{}", names.len()); // COMPILE ERROR: names was moved

// .iter_mut() -- borrows each element as &mut T. Can modify in place.
let mut numbers = vec![1, 2, 3];
for n in numbers.iter_mut() {
    *n *= 2;
}
// numbers is now [2, 4, 6]
```

Tip: `for item in &vec` is shorthand for `for item in vec.iter()`, and `for item in &mut vec` is shorthand for `for item in vec.iter_mut()`.

### Exercise 6.1

Write a function that takes a `Vec<String>` of shell command strings (like `vec!["ls -la", "echo hello", "ls foo", "echo world", "cat file"]`) and returns a `HashMap<String, usize>` counting how many times each command *name* (the first word) appears. For example, the input above should return `{"ls": 2, "echo": 2, "cat": 1}`.

---

## 7. Traits

Traits are Rust's version of interfaces. They define shared behavior that different types can implement. You will encounter traits constantly -- every time you call `.to_string()`, use `println!`, or write to stdout, you are using traits.

### Defining and implementing traits

```rust
trait Executable {
    fn execute(&self) -> Result<i32, String>;
    fn name(&self) -> &str;

    // Default implementation -- types can override this or use it as-is
    fn describe(&self) -> String {
        format!("Command: {}", self.name())
    }
}

struct ExternalCommand {
    program: String,
    args: Vec<String>,
}

impl Executable for ExternalCommand {
    fn execute(&self) -> Result<i32, String> {
        // In reality, this would use std::process::Command
        println!("Running: {} {:?}", self.program, self.args);
        Ok(0)
    }

    fn name(&self) -> &str {
        &self.program
    }
    // describe() uses the default implementation
}

struct BuiltinCd {
    target: String,
}

impl Executable for BuiltinCd {
    fn execute(&self) -> Result<i32, String> {
        println!("Changing to: {}", self.target);
        Ok(0)
    }

    fn name(&self) -> &str {
        "cd"
    }

    // Override the default
    fn describe(&self) -> String {
        format!("Builtin: cd {}", self.target)
    }
}
```

### The Display and Debug traits

These control how your types are printed:

```rust
use std::fmt;

#[derive(Debug)] // Auto-generates Debug: {:?} formatting
struct Command {
    program: String,
    args: Vec<String>,
}

// Display must be implemented manually -- it controls {} formatting
impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.program)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

fn main() {
    let cmd = Command {
        program: String::from("echo"),
        args: vec![String::from("hello"), String::from("world")],
    };

    println!("{cmd}");   // Uses Display: "echo hello world"
    println!("{cmd:?}"); // Uses Debug: Command { program: "echo", args: ["hello", "world"] }
}
```

`Debug` is for developers (detailed, shows structure). `Display` is for users (clean, readable). Always derive `Debug`. Implement `Display` when your type has a natural human-readable representation.

### The Write trait

This one matters immediately for Module 1. To call `.flush()` on stdout, you need to import the `Write` trait:

```rust
use std::io::{self, Write};

fn print_prompt() -> io::Result<()> {
    let mut stdout = io::stdout();
    write!(stdout, "jsh> ")?;   // write! macro requires the Write trait
    stdout.flush()?;            // .flush() is defined on the Write trait
    Ok(())
}
```

Why does importing a trait matter? In Rust, you can only call a trait's methods if the trait is in scope. `stdout()` returns a type that *implements* `Write`, but the compiler needs to know about `Write` to let you call `.flush()`. This is a common "gotcha" for beginners.

### Trait bounds on generics

You can write functions that accept any type implementing a specific trait:

```rust
use std::fmt::Display;

// This function accepts anything that can be displayed
fn print_error(error: &impl Display) {
    eprintln!("Error: {error}");
}

// Equivalent longer syntax with explicit generic:
fn print_error_verbose<E: Display>(error: &E) {
    eprintln!("Error: {error}");
}

// Multiple bounds:
fn log_and_clone<T: Display + Clone>(item: &T) {
    println!("Logging: {item}");
    let _copy = item.clone();
}

// Where clause for complex bounds:
fn process<T>(item: T)
where
    T: Display + Clone + Default,
{
    println!("{item}");
}
```

### Common standard library traits

| Trait | What it does | How you get it |
|-------|-------------|----------------|
| `Debug` | `{:?}` formatting | `#[derive(Debug)]` |
| `Display` | `{}` formatting | Implement manually |
| `Clone` | `.clone()` deep copy | `#[derive(Clone)]` |
| `Copy` | Implicit copy on assignment | `#[derive(Copy, Clone)]` (only for simple stack types) |
| `Default` | `Type::default()` | `#[derive(Default)]` |
| `PartialEq` | `==` and `!=` | `#[derive(PartialEq)]` |
| `From<T>` / `Into<T>` | Type conversion | Implement `From`, get `Into` free |
| `Write` | Write bytes to a sink | Implemented by stdout, File, etc. |

The `From`/`Into` pair is especially useful for error handling:

```rust
#[derive(Debug)]
enum ShellError {
    Io(std::io::Error),
    Parse(String),
}

impl From<std::io::Error> for ShellError {
    fn from(error: std::io::Error) -> Self {
        ShellError::Io(error)
    }
}

// Now the ? operator on io::Error automatically converts to ShellError
fn read_rc_file() -> Result<String, ShellError> {
    let contents = std::fs::read_to_string("~/.jshrc")?; // io::Error -> ShellError
    Ok(contents)
}
```

### Exercise 7.1

Define a trait `Describable` with a method `fn describe(&self) -> String`. Implement it for two types: a `FileRedirect` struct (with fields `fd: i32` and `path: String`) and a `PipeRedirect` struct (with fields `from_fd: i32` and `to_fd: i32`). Write a function that takes a `&impl Describable` and prints the description.

---

## 8. Modules & Project Structure

As our shell grows from a single `main.rs` to thousands of lines, we need to organize code into modules. Rust's module system is explicit -- nothing is visible unless you say so.

### File structure

Rust maps modules to files. Here is how our shell project will be organized:

```
james-shell/
  Cargo.toml
  src/
    main.rs         -- entry point, REPL loop
    parser.rs       -- tokenizer and command parser
    executor.rs     -- running commands (external + builtin)
    builtins.rs     -- cd, exit, pwd, etc.
    environment.rs  -- environment variable management
    redirect.rs     -- I/O redirection
    pipeline.rs     -- pipe handling
```

### mod declarations

In `main.rs`, you declare modules:

```rust
// src/main.rs
mod parser;      // Tells Rust to look for src/parser.rs
mod executor;    // Tells Rust to look for src/executor.rs
mod builtins;    // Tells Rust to look for src/builtins.rs

fn main() {
    let input = "echo hello world";
    let command = parser::parse(input);      // Use the module
    executor::run(&command);
}
```

In `src/parser.rs`:

```rust
// src/parser.rs

pub struct Command {
    pub program: String,
    pub args: Vec<String>,
}

pub fn parse(input: &str) -> Command {
    let parts: Vec<&str> = input.split_whitespace().collect();
    Command {
        program: parts[0].to_string(),
        args: parts[1..].iter().map(|s| s.to_string()).collect(),
    }
}
```

### pub visibility

By default, everything in Rust is private. Use `pub` to make items visible outside their module:

```rust
pub struct Command {        // Struct is public
    pub program: String,    // Field is public
    pub args: Vec<String>,  // Field is public
    exit_code: i32,         // Field is PRIVATE -- only code in this module can access it
}

pub fn parse(input: &str) -> Command {  // Function is public
    // ...
    todo!()
}

fn tokenize(input: &str) -> Vec<String> {  // Function is PRIVATE -- internal helper
    // ...
    todo!()
}
```

Think of `pub` as "part of the module's API." Internal helpers stay private.

### use statements

`use` brings items into scope so you do not have to write the full path:

```rust
// Without use:
let cmd = parser::Command::new(parser::parse_program(input));

// With use:
use crate::parser::{Command, parse_program};
let cmd = Command::new(parse_program(input));

// Glob import (use sparingly):
use crate::parser::*;

// Renaming to avoid conflicts:
use std::io::Result as IoResult;
use crate::parser::Result as ParseResult;
```

The `crate::` prefix means "start from the root of this project." You can also use `super::` to go up one module level.

### Cargo.toml basics

```toml
[package]
name = "james-shell"
version = "0.1.0"
edition = "2024"       # Rust edition -- determines which language features are available

[dependencies]
ctrlc = "3"            # Cross-platform Ctrl-C handling
# nix = "0.29"         # Unix system calls (fork, exec, pipe)  -- added later
# rustyline = "14"     # Line editing and history              -- added later
# glob = "0.3"         # Wildcard expansion                   -- added later
```

Key fields:
- **name** -- the package name (also the binary name by default)
- **version** -- follows semantic versioning (major.minor.patch)
- **edition** -- the Rust edition; newer editions enable newer syntax
- **dependencies** -- external crates from crates.io

### Cargo commands

```bash
cargo build             # Compile the project (debug mode)
cargo build --release   # Compile with optimizations (slow compile, fast binary)
cargo run               # Build and run
cargo run -- arg1 arg2  # Build and run, passing arguments to your program
cargo test              # Run all tests
cargo clippy            # Run the linter (catches common mistakes)
cargo fmt               # Auto-format all code
cargo doc --open        # Generate and open documentation
cargo check             # Type-check without full compilation (fast feedback)
```

During development, the loop is: write code, `cargo check` (fast type checking), fix errors, `cargo run` (test it), `cargo clippy` (catch style issues), `cargo test` (run tests).

### Exercise 8.1

Create a new Rust project with `cargo new practice-shell`. Add a `src/greeter.rs` module that exports a `pub fn greet(name: &str) -> String` function. Import and use it from `main.rs`. Then add the `rand` crate to `Cargo.toml` and use it to pick a random greeting.

---

## 9. Standard Library: io, fs, and process

These three modules from Rust's standard library are the foundation of our shell. They handle input/output, file system access, and running external programs.

### std::io -- Reading and Writing

```rust
use std::io::{self, BufRead, Write};

fn repl_basics() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    // Writing to stdout (with explicit flush for prompts)
    write!(stdout, "jsh> ")?;
    stdout.flush()?;

    // Reading one line from stdin
    let mut input = String::new();
    let bytes = stdin.read_line(&mut input)?;

    if bytes == 0 {
        println!("EOF reached");
    } else {
        println!("You typed: {}", input.trim());
    }

    // Writing to stderr (for error messages)
    eprintln!("This goes to stderr");

    // Reading lines with a buffered reader (useful for reading from files/pipes)
    let reader = io::BufReader::new(io::stdin());
    for line in reader.lines() {
        let line = line?;
        println!("Line: {line}");
    }

    Ok(())
}
```

Key types:
- `io::stdin()` -- handle to standard input
- `io::stdout()` -- handle to standard output
- `io::stderr()` -- handle to standard error
- `io::BufReader` -- wraps any reader with buffering (efficient for line-by-line reading)
- `io::Result<T>` -- shorthand for `Result<T, io::Error>`

### std::fs -- File System Operations

```rust
use std::fs;
use std::io::Write;

fn file_operations() -> std::io::Result<()> {
    // Read an entire file into a String
    let contents = fs::read_to_string("Cargo.toml")?;
    println!("File contents:\n{contents}");

    // Write a string to a file (creates or overwrites)
    fs::write("output.txt", "hello from james-shell\n")?;

    // Append to a file
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open("output.txt")?;
    writeln!(file, "another line")?;

    // Check if a file/directory exists
    if fs::metadata("Cargo.toml").is_ok() {
        println!("Cargo.toml exists");
    }

    // Read directory contents
    for entry in fs::read_dir("src")? {
        let entry = entry?;
        let path = entry.path();
        let file_type = if path.is_dir() { "DIR" } else { "FILE" };
        println!("{file_type}: {}", path.display());
    }

    // Create a directory
    fs::create_dir_all("tmp/nested/dirs")?;

    // Remove a file
    fs::remove_file("output.txt")?;

    Ok(())
}
```

These are the operations our shell will perform behind the scenes for builtins like `cd`, `pwd`, and for I/O redirection.

### std::process -- Running External Commands

This is how our shell will execute programs like `ls`, `grep`, and `cat`:

```rust
use std::process::{Command, Stdio};

fn process_examples() -> std::io::Result<()> {
    // Run a command and wait for it to finish
    let status = Command::new("echo")
        .arg("hello")
        .arg("world")
        .status()?;

    println!("Exit code: {}", status.code().unwrap_or(-1));

    // Capture stdout as a String
    let output = Command::new("ls")
        .arg("-la")
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("stdout:\n{stdout}");
    println!("stderr:\n{stderr}");
    println!("success: {}", output.status.success());

    // Redirect stdin/stdout/stderr
    let child = Command::new("grep")
        .arg("hello")
        .stdin(Stdio::piped())      // We will write to this process's stdin
        .stdout(Stdio::piped())     // Capture stdout
        .stderr(Stdio::null())      // Discard stderr
        .spawn()?;                  // Start the process (don't wait)

    // spawn() returns a Child, which we can interact with
    // ... we'll use this heavily in Module 7 (Pipes)

    Ok(())
}
```

Key types and methods:
- `Command::new("program")` -- builds a command to run
- `.arg("argument")` -- adds a single argument
- `.args(&["arg1", "arg2"])` -- adds multiple arguments
- `.env("KEY", "VALUE")` -- sets an environment variable
- `.current_dir("/some/path")` -- sets the working directory
- `.status()` -- runs the command, waits, returns the exit status
- `.output()` -- runs the command, waits, captures all output
- `.spawn()` -- starts the command and returns immediately (gives you a `Child` handle)
- `Stdio::piped()` -- connect to a pipe (for reading/writing from the parent)
- `Stdio::null()` -- discard (like redirecting to `/dev/null`)
- `Stdio::inherit()` -- use the parent process's stdio (the default)

### What these modules provide for our shell

| Module | Shell Feature |
|--------|--------------|
| `std::io` | REPL loop (reading input, writing prompts), I/O redirection |
| `std::fs` | File redirection (`>`, `<`), checking if files exist, `cd` path validation |
| `std::process` | Running external commands (`ls`, `grep`, etc.), pipes, capturing output |

### Exercise 9.1

Write a program that:
1. Reads a file path from command line arguments (`std::env::args`)
2. Opens and reads that file
3. Prints each line with a line number prefix
4. If no argument is given, prints a usage message to stderr
5. If the file does not exist, prints a helpful error message

This is essentially a minimal `cat -n` clone.

---

## 10. Practice Exercises

These exercises combine multiple concepts from this module. Complete them before moving to Module 1 to verify your readiness.

### Exercise 10.1: Word Frequency Counter

Write a program that reads a file and counts the frequency of each word. Print the results sorted by frequency (most common first).

Requirements:
- Accept a filename as a command line argument
- Handle errors with `Result` and `?` (file not found, read errors)
- Use `HashMap` for counting
- Use iterators to process and sort
- Print to stdout in the format: `count word` (one per line)

Example:

```
$ echo "the cat sat on the mat the cat" > test.txt
$ cargo run -- test.txt
3 the
2 cat
1 sat
1 on
1 mat
```

Hints:
- `std::env::args().nth(1)` gets the first command line argument
- `split_whitespace()` splits a string into words
- `HashMap::entry().or_insert(0)` handles the counting pattern
- Collect the HashMap into a `Vec` and use `.sort_by()` to sort by count

### Exercise 10.2: CLI Argument Processor

Write a program that mimics a simplified `echo` command with flags:

```
$ cargo run -- -n hello world        # Print without trailing newline
hello world$
$ cargo run -- -e "hello\tworld"     # Interpret escape sequences
hello	world
$ cargo run -- hello world           # Default: print with newline
hello world
```

Requirements:
- Parse command line arguments manually (no clap or structopt)
- Support `-n` (no trailing newline) and `-e` (interpret `\n`, `\t`, `\\` escapes)
- Arguments after the flags are joined with spaces and printed
- If no arguments, print nothing (just exit)

Hints:
- Use `std::env::args().skip(1)` to skip the program name
- Collect args into a `Vec<String>`, then iterate to separate flags from words
- Use `match` on the escape characters
- Use `print!` instead of `println!` when `-n` is specified

### Exercise 10.3: External Command Runner

Write a program that runs an external command and captures its output:

```
$ cargo run -- ls -la /tmp
Exit code: 0
Stdout (14 lines):
total 48
drwxrwxrwt 12 root root 4096 ...
...

$ cargo run -- nonexistent-program
Error: command not found: nonexistent-program
```

Requirements:
- Take the command and its arguments from CLI args
- Use `std::process::Command` to run it
- Capture stdout, stderr, and the exit code
- Print the exit code, the number of stdout lines, then the stdout content
- If the command fails to start (not found), print a helpful error to stderr
- Handle the case where no arguments are given

Hints:
- `Command::new(&args[0]).args(&args[1..]).output()` runs and captures
- `String::from_utf8_lossy()` converts bytes to a string
- The `.output()` call returns `io::Result<Output>` -- match on the error to distinguish "command not found" from other failures

---

## Self-Assessment Checklist

Before starting Module 1, you should be able to honestly check off every item below. If any feel shaky, revisit that section or work through additional examples.

### Ownership & Borrowing
- [ ] I can explain what happens to memory when a variable goes out of scope
- [ ] I can predict when a move vs copy occurs
- [ ] I know the difference between `&T` and `&mut T` and when to use each
- [ ] I can fix "value used after move" errors without just adding `.clone()` everywhere
- [ ] I understand why Rust only allows one mutable reference at a time

### Error Handling
- [ ] I can use `Result<T, E>` and the `?` operator to propagate errors
- [ ] I know when to use `.unwrap()`, `.expect()`, and `match` on a Result
- [ ] I can define a custom error enum with variants for different failure modes
- [ ] I can chain `.map()`, `.and_then()`, and `.unwrap_or_else()` on Result/Option

### Pattern Matching
- [ ] I can write exhaustive `match` expressions on enums
- [ ] I can use `if let` for single-variant checks
- [ ] I can destructure structs, enums, and tuples in match arms
- [ ] I can use match guards for additional conditions

### Enums & Structs
- [ ] I can define enums with data-carrying variants
- [ ] I can write `impl` blocks with methods that take `&self`, `&mut self`, or `self`
- [ ] I know what `#[derive(Debug, Clone, PartialEq)]` does and when to use each
- [ ] I understand that Option and Result are regular enums, not compiler magic

### Strings
- [ ] I can explain the difference between `String` and `&str` and when to use each
- [ ] I know why `string[0]` does not compile in Rust
- [ ] I can use `.chars()`, `.split_whitespace()`, `.trim()`, `.starts_with()`, and other common string methods
- [ ] I can use `format!()` to build strings and `println!()` / `eprintln!()` for output

### Collections
- [ ] I can use `Vec<T>` (push, pop, iterate, index, slice)
- [ ] I can use `HashMap<K, V>` (insert, get, entry API, iterate)
- [ ] I can chain iterator methods (`.map()`, `.filter()`, `.collect()`)
- [ ] I know the difference between `.iter()`, `.into_iter()`, and `.iter_mut()`

### Traits
- [ ] I can define a trait and implement it for a type
- [ ] I understand why `use std::io::Write` is needed to call `.flush()`
- [ ] I can implement `Display` for a custom type
- [ ] I know the purpose of `From`/`Into` for error type conversion

### Modules & Cargo
- [ ] I can split code into multiple files using `mod` and `pub`
- [ ] I can use `cargo build`, `cargo run`, `cargo test`, and `cargo clippy`
- [ ] I know how to add dependencies to `Cargo.toml`
- [ ] I can use `use` statements to bring items into scope

### Standard Library
- [ ] I can read from stdin and write to stdout/stderr
- [ ] I can read and write files with `std::fs`
- [ ] I can run external commands with `std::process::Command` and capture their output
- [ ] I know what `flush()` does and why it matters for interactive prompts

---

## What's Next?

If you checked everything above, you are ready for Module 1: The REPL Loop. That module will have you build the core loop of the shell -- printing a prompt, reading input, and handling EOF and Ctrl-C. Every concept from this module will be used immediately.

If some boxes are unchecked, here are targeted resources:

- **Ownership still confusing?** Read Chapter 4 of *The Rust Programming Language* ("Understanding Ownership"). Do the Rustlings `move_semantics` exercises.
- **Error handling unclear?** Read Chapter 9 of *The Rust Programming Language*. Write small programs that open files and handle missing files gracefully.
- **Pattern matching / enums?** Read Chapter 6 ("Enums and Pattern Matching"). Write an enum with 4-5 variants and match on all of them.
- **Strings?** Read Chapter 8.2. Write a program that takes user input and manipulates it in various ways.
- **Traits?** Read Chapter 10.2. Implement `Display` for a custom struct.
- **General practice?** Work through the [Rustlings exercises](https://github.com/rust-lang/rustlings) -- they cover all of these topics in small, focused drills.
