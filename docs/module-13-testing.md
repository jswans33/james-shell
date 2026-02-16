# Module 13: Testing & Robustness

## What are we building?

A shell sits between the user and their operating system. If it crashes, garbles
output, or silently drops data, the consequences range from annoying to
catastrophic. In this module we build a comprehensive testing strategy for
james-shell:

1. **Unit tests** for parsers, expanders, and individual builtins.
2. **Integration tests** that run the shell as a subprocess and verify its
   behaviour end-to-end.
3. **A testing strategy** that explains what to test at each architectural layer.
4. **Fuzz testing** to find crashes and panics with random input.
5. **Edge case handling** for empty input, massive input, binary data, and deep
   nesting.
6. **Memory safety verification** leveraging Rust's guarantees and auditing
   any `unsafe` code.
7. **Benchmarking** with the `criterion` crate.
8. **CI/CD** with GitHub Actions.
9. **A custom test harness** that runs `.jsh` script files and checks output.

---

## Concept 1: Unit Testing Parsers and Expanders

### Rust's Built-in Test Framework

Rust has first-class support for testing. You annotate test functions with
`#[test]` and conditionally compile test modules with `#[cfg(test)]`. Tests
live right next to the code they exercise.

```rust
// In src/lexer.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_simple_command() {
        let tokens = lex("echo hello world");
        assert_eq!(tokens, vec![
            Token::Word("echo".to_string()),
            Token::Word("hello".to_string()),
            Token::Word("world".to_string()),
        ]);
    }

    #[test]
    fn lex_pipe() {
        let tokens = lex("ls | grep foo");
        assert_eq!(tokens, vec![
            Token::Word("ls".to_string()),
            Token::Pipe,
            Token::Word("grep".to_string()),
            Token::Word("foo".to_string()),
        ]);
    }

    #[test]
    fn lex_and_or() {
        let tokens = lex("cmd1 && cmd2 || cmd3");
        assert_eq!(tokens, vec![
            Token::Word("cmd1".to_string()),
            Token::And,
            Token::Word("cmd2".to_string()),
            Token::Or,
            Token::Word("cmd3".to_string()),
        ]);
    }

    #[test]
    fn lex_quoted_string() {
        let tokens = lex(r#"echo "hello world""#);
        assert_eq!(tokens, vec![
            Token::Word("echo".to_string()),
            Token::Word("hello world".to_string()),
        ]);
    }

    #[test]
    fn lex_empty_input() {
        let tokens = lex("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn lex_only_whitespace() {
        let tokens = lex("   \t  \n  ");
        assert!(tokens.is_empty());
    }
}
```

### Testing the Parser

Parser tests verify that a token stream produces the expected AST. We use
pattern matching to check the structure without comparing every field.

```rust
// In src/parser.rs

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(input: &str) -> Vec<AstNode> {
        let tokens = lex(input);
        let mut parser = Parser::new(tokens);
        parser.parse_program().expect("parse failed")
    }

    #[test]
    fn parse_simple_command() {
        let ast = parse_str("echo hello");
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            AstNode::SimpleCommand(cmd) => {
                assert_eq!(cmd.argv, vec!["echo", "hello"]);
            }
            other => panic!("expected SimpleCommand, got {:?}", other),
        }
    }

    #[test]
    fn parse_pipeline() {
        let ast = parse_str("ls | grep foo | wc -l");
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            AstNode::Pipeline(cmds) => {
                assert_eq!(cmds.len(), 3);
            }
            other => panic!("expected Pipeline, got {:?}", other),
        }
    }

    #[test]
    fn parse_if_else() {
        let ast = parse_str("if test -f foo { echo yes } else { echo no }");
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            AstNode::If(block) => {
                assert!(block.else_body.is_some());
                assert!(block.elif_branches.is_empty());
            }
            other => panic!("expected If, got {:?}", other),
        }
    }

    #[test]
    fn parse_for_loop() {
        let ast = parse_str("for x in a b c { echo $x }");
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            AstNode::For(fl) => {
                assert_eq!(fl.variable, "x");
                assert_eq!(fl.items, vec!["a", "b", "c"]);
            }
            other => panic!("expected For, got {:?}", other),
        }
    }

    #[test]
    fn parse_function_def() {
        let ast = parse_str("fn greet(name) { echo hello $name }");
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            AstNode::FunctionDef(fd) => {
                assert_eq!(fd.name, "greet");
                assert_eq!(fd.params, vec!["name"]);
            }
            other => panic!("expected FunctionDef, got {:?}", other),
        }
    }

    #[test]
    fn parse_nested_if_in_for() {
        let input = r#"
            for f in a b c {
                if test $f = b {
                    echo "found b"
                }
            }
        "#;
        let ast = parse_str(input);
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            AstNode::For(fl) => {
                assert_eq!(fl.body.len(), 1);
                assert!(matches!(&fl.body[0], AstNode::If(_)));
            }
            other => panic!("expected For, got {:?}", other),
        }
    }

    #[test]
    fn parse_error_unclosed_brace() {
        let tokens = lex("if test -f foo { echo yes");
        let mut parser = Parser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }
}
```

### Testing the Expander

Variable expansion, command substitution, and arithmetic expansion each need
isolated tests.

```rust
// In src/expand.rs

#[cfg(test)]
mod tests {
    use super::*;

    fn make_env() -> ShellEnv {
        let mut env = ShellEnv::new();
        env.set_var("HOME", "/home/james");
        env.set_var("USER", "james");
        env.set_var("count", "42");
        env
    }

    #[test]
    fn expand_simple_variable() {
        let env = make_env();
        assert_eq!(expand_word("$USER", &env), "james");
    }

    #[test]
    fn expand_variable_in_string() {
        let env = make_env();
        assert_eq!(
            expand_word("Hello, $USER!", &env),
            "Hello, james!"
        );
    }

    #[test]
    fn expand_braced_variable() {
        let env = make_env();
        assert_eq!(
            expand_word("${USER}_home", &env),
            "james_home"
        );
    }

    #[test]
    fn expand_undefined_variable() {
        let env = make_env();
        assert_eq!(expand_word("$UNDEFINED", &env), "");
    }

    #[test]
    fn expand_arithmetic() {
        let env = make_env();
        assert_eq!(expand_word("$((1 + 2))", &env), "3");
    }

    #[test]
    fn expand_arithmetic_with_variable() {
        let env = make_env();
        assert_eq!(expand_word("$((count * 2))", &env), "84");
    }

    #[test]
    fn expand_nested_arithmetic() {
        let env = make_env();
        assert_eq!(expand_word("$((2 * (3 + 4)))", &env), "14");
    }

    #[test]
    fn expand_tilde() {
        let env = make_env();
        assert_eq!(expand_word("~/docs", &env), "/home/james/docs");
    }
}
```

### Testing Arithmetic Evaluation Directly

```rust
// In src/arith.rs

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(input: &str) -> i64 {
        let env = ShellEnv::new();
        let mut parser = ArithParser::new(input);
        let expr = parser.parse_expr().expect("parse error");
        eval_arith(&expr, &env).expect("eval error")
    }

    #[test]
    fn basic_addition() { assert_eq!(eval("1 + 2"), 3); }

    #[test]
    fn precedence() { assert_eq!(eval("2 + 3 * 4"), 14); }

    #[test]
    fn parentheses() { assert_eq!(eval("(2 + 3) * 4"), 20); }

    #[test]
    fn unary_minus() { assert_eq!(eval("-5 + 3"), -2); }

    #[test]
    fn modulo() { assert_eq!(eval("17 % 5"), 2); }

    #[test]
    fn comparison_true() { assert_eq!(eval("3 < 5"), 1); }

    #[test]
    fn comparison_false() { assert_eq!(eval("5 < 3"), 0); }

    #[test]
    fn bitwise_and() { assert_eq!(eval("0xFF & 0x0F"), 15); }

    #[test]
    fn division_by_zero() {
        let env = ShellEnv::new();
        let mut parser = ArithParser::new("1 / 0");
        let expr = parser.parse_expr().unwrap();
        assert!(eval_arith(&expr, &env).is_err());
    }

    #[test]
    fn deeply_nested_parens() {
        assert_eq!(eval("((((1 + 2))))"), 3);
    }
}
```

### Running Tests

```
$ cargo test
   Compiling james-shell v0.1.0
    Finished test target(s)
     Running unittests src/main.rs

running 28 tests
test lexer::tests::lex_simple_command ... ok
test lexer::tests::lex_pipe ... ok
test lexer::tests::lex_and_or ... ok
test lexer::tests::lex_quoted_string ... ok
test lexer::tests::lex_empty_input ... ok
test lexer::tests::lex_only_whitespace ... ok
test parser::tests::parse_simple_command ... ok
test parser::tests::parse_pipeline ... ok
test parser::tests::parse_if_else ... ok
test parser::tests::parse_for_loop ... ok
test parser::tests::parse_function_def ... ok
test parser::tests::parse_nested_if_in_for ... ok
test parser::tests::parse_error_unclosed_brace ... ok
test expand::tests::expand_simple_variable ... ok
test expand::tests::expand_variable_in_string ... ok
test expand::tests::expand_braced_variable ... ok
test expand::tests::expand_undefined_variable ... ok
test expand::tests::expand_arithmetic ... ok
test expand::tests::expand_arithmetic_with_variable ... ok
test expand::tests::expand_nested_arithmetic ... ok
test expand::tests::expand_tilde ... ok
test arith::tests::basic_addition ... ok
test arith::tests::precedence ... ok
test arith::tests::parentheses ... ok
test arith::tests::unary_minus ... ok
test arith::tests::modulo ... ok
test arith::tests::comparison_true ... ok
test arith::tests::division_by_zero ... ok

test result: ok. 28 passed; 0 failed; 0 ignored
```

---

## Concept 2: Integration Testing with `assert_cmd` and `predicates`

### What Are Integration Tests?

Integration tests treat the shell as a **black box**. They launch the `jsh`
binary, feed it input, and check the output and exit code. This tests the full
stack: lexer, parser, expander, evaluator, builtins, and I/O.

### Setup

Add these to `Cargo.toml`:

```toml
[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

Integration test files live in `tests/` at the project root (not inside `src/`).

### File Structure

```
james-shell/
  src/
    main.rs
    lexer.rs
    parser.rs
    ...
  tests/
    integration.rs
    scripts/
      hello.jsh
      loop_test.jsh
```

### Writing Integration Tests

```rust
// tests/integration.rs

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper to run jsh with the given input on stdin.
fn jsh() -> Command {
    Command::cargo_bin("jsh").expect("binary not found")
}

#[test]
fn echo_hello() {
    jsh()
        .write_stdin("echo hello\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn exit_code_zero() {
    jsh()
        .write_stdin("true\n")
        .assert()
        .success();
}

#[test]
fn exit_code_nonzero() {
    jsh()
        .write_stdin("false\n")
        .assert()
        .code(1);
}

#[test]
fn pipe_two_commands() {
    jsh()
        .write_stdin("echo hello world | wc -w\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("2"));
}

#[test]
fn variable_expansion() {
    jsh()
        .write_stdin("let x = 42\necho $x\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("42"));
}

#[test]
fn conditional_and() {
    jsh()
        .write_stdin("true && echo yes\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("yes"));
}

#[test]
fn conditional_or() {
    jsh()
        .write_stdin("false || echo fallback\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("fallback"));
}

#[test]
fn if_else() {
    jsh()
        .write_stdin("if false { echo no } else { echo yes }\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("yes"));
}

#[test]
fn for_loop() {
    jsh()
        .write_stdin("for i in a b c { echo $i }\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("a\nb\nc"));
}

#[test]
fn script_file_execution() {
    // Create a temporary script file.
    let mut script = NamedTempFile::new().unwrap();
    writeln!(script, "echo script-output").unwrap();
    let path = script.path().to_string_lossy().to_string();

    jsh()
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains("script-output"));
}

#[test]
fn arithmetic_expansion() {
    jsh()
        .write_stdin("echo $((3 + 4 * 2))\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("11"));
}

#[test]
fn command_substitution() {
    jsh()
        .write_stdin("echo hello-$(echo world)\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello-world"));
}

#[test]
fn function_definition_and_call() {
    jsh()
        .write_stdin("fn greet(name) { echo hello $name }\ngreet world\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello world"));
}

#[test]
fn stderr_not_captured_by_pipe() {
    // Commands in a pipeline should only pipe stdout, not stderr.
    jsh()
        .write_stdin("echo error >&2 | cat\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("error"));
}
```

### The `predicates` Crate

The `predicates` crate provides composable assertions:

```rust
use predicates::prelude::*;

// String predicates
predicate::str::contains("hello")          // stdout contains "hello"
predicate::str::starts_with("Error")       // starts with "Error"
predicate::str::is_match("^\\d+$").unwrap() // matches a regex

// Logical combinators
predicate::str::contains("a").and(predicate::str::contains("b"))
predicate::str::contains("x").or(predicate::str::contains("y"))
predicate::str::contains("secret").not()   // must NOT contain "secret"
```

### Running Integration Tests

```
$ cargo test --test integration
   Compiling james-shell v0.1.0
    Finished test target(s)
     Running tests/integration.rs

running 14 tests
test echo_hello ... ok
test exit_code_zero ... ok
test exit_code_nonzero ... ok
test pipe_two_commands ... ok
test variable_expansion ... ok
test conditional_and ... ok
test conditional_or ... ok
test if_else ... ok
test for_loop ... ok
test script_file_execution ... ok
test arithmetic_expansion ... ok
test command_substitution ... ok
test function_definition_and_call ... ok
test stderr_not_captured_by_pipe ... ok

test result: ok. 14 passed; 0 failed; 0 ignored
```

---

## Concept 3: Testing Strategy -- What to Test at Each Layer

### The Testing Pyramid

```
                    /\
                   /  \
                  / E2E \          Few: slow, fragile, but realistic
                 /--------\
                /Integration\      Moderate: run jsh as a subprocess
               /--------------\
              /   Unit Tests    \  Many: fast, focused, deterministic
             /____________________\
```

### Layer-by-Layer Guide

| Layer | What to test | Example |
|-------|-------------|---------|
| **Lexer** | Token output for each syntax construct; edge cases like empty strings, unterminated quotes, unusual characters | `lex("''")` produces `Token::Word("")` |
| **Parser** | AST shape for each grammar rule; error recovery on malformed input | `parse("if {}")` returns `Err(...)` |
| **Expander** | Variable expansion, tilde, glob, command substitution, arithmetic | `expand("$((1+2))")` returns `"3"` |
| **Builtins** | Each builtin in isolation with various argument patterns | `builtin_cd(&["/tmp"], &mut env)` changes `env.cwd` |
| **Evaluator** | Control flow: if/else branches taken, loop iterations, function calls | `eval("if true { echo yes }")` outputs `"yes"` |
| **Pipeline** | Multi-process pipes, exit code propagation, pipefail | `echo hi \| cat` outputs `"hi"` |
| **Job control** | Background jobs, fg/bg, signal handling | `sleep 10 &` creates a background job |
| **Integration** | Full command strings through the binary | `jsh -c "echo hello"` outputs `"hello"` |
| **Scripts** | Complete `.jsh` files run end-to-end | A deployment script produces expected output |

### What NOT to Unit Test

- **External command behaviour**: Do not test that `ls` lists files. That is
  the operating system's responsibility. Test that your shell *invokes* `ls`
  correctly.
- **Terminal rendering details**: Prompt colour codes depend on the terminal
  emulator. Test the logic that generates the escape sequences, not how they
  render.

### Test Isolation

Each test should create its own `ShellEnv` from scratch. Never share mutable
state between tests.

```rust
#[test]
fn test_something() {
    let mut env = ShellEnv::new();  // Fresh environment
    // ... test logic ...
}
```

If a test needs filesystem state, use `tempfile::TempDir`:

```rust
use tempfile::TempDir;

#[test]
fn test_glob_expansion() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();
    std::fs::write(dir.path().join("c.rs"), "").unwrap();

    let mut env = ShellEnv::new();
    env.set_cwd(dir.path());

    let expanded = expand_glob("*.txt", &env);
    assert_eq!(expanded.len(), 2);
    assert!(expanded.contains(&"a.txt".to_string()));
    assert!(expanded.contains(&"b.txt".to_string()));
}
```

---

## Concept 4: Fuzz Testing with `cargo-fuzz`

### Why Fuzz?

A shell's input is arbitrary user text. Fuzzing generates random (and
semi-random) inputs and feeds them to our code, looking for panics, infinite
loops, or memory safety violations. It is one of the most effective ways to
find bugs in parsers.

### Setup

```
$ cargo install cargo-fuzz
$ cargo fuzz init
```

This creates a `fuzz/` directory:

```
fuzz/
  Cargo.toml
  fuzz_targets/
    fuzz_target_1.rs
```

### Fuzz Target: Lexer

```rust
// fuzz/fuzz_targets/fuzz_lexer.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use james_shell::lexer::lex;

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 strings.
    if let Ok(input) = std::str::from_utf8(data) {
        // The lexer should never panic, regardless of input.
        let _ = lex(input);
    }
});
```

### Fuzz Target: Parser

```rust
// fuzz/fuzz_targets/fuzz_parser.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use james_shell::lexer::lex;
use james_shell::parser::Parser;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let tokens = lex(input);
        let mut parser = Parser::new(tokens);
        // The parser should return Ok or Err, but never panic.
        let _ = parser.parse_program();
    }
});
```

### Fuzz Target: Arithmetic Evaluator

```rust
// fuzz/fuzz_targets/fuzz_arith.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use james_shell::arith::{ArithParser, eval_arith};
use james_shell::env::ShellEnv;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let mut parser = ArithParser::new(input);
        if let Ok(expr) = parser.parse_expr() {
            let env = ShellEnv::new();
            // Evaluation may fail (e.g., division by zero) but must not panic.
            let _ = eval_arith(&expr, &env);
        }
    }
});
```

### Fuzz Target: Full Pipeline (Lex + Parse + Eval)

```rust
// fuzz/fuzz_targets/fuzz_eval.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use james_shell::lexer::lex;
use james_shell::parser::Parser;
use james_shell::eval::eval_nodes;
use james_shell::env::ShellEnv;
use std::time::Duration;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input length to prevent resource exhaustion.
        if input.len() > 1024 {
            return;
        }

        let tokens = lex(input);
        let mut parser = Parser::new(tokens);
        if let Ok(ast) = parser.parse_program() {
            let mut env = ShellEnv::new();
            // Set a timeout to prevent infinite loops.
            env.set_execution_timeout(Duration::from_millis(100));
            let _ = eval_nodes(&ast, &mut env);
        }
    }
});
```

### Running the Fuzzer

```
$ cargo fuzz run fuzz_lexer -- -max_total_time=300
```

This runs for 5 minutes. If a crash is found, the input is saved in
`fuzz/artifacts/fuzz_lexer/` and you can reproduce it:

```
$ cargo fuzz run fuzz_lexer fuzz/artifacts/fuzz_lexer/crash-abc123
```

### What Fuzzing Typically Finds

| Bug type | Example |
|----------|---------|
| **Index out of bounds** | Accessing `chars[i]` without checking length |
| **Infinite loops** | Parser stuck on unexpected token, never advancing |
| **Stack overflow** | Deeply nested `$(((((...)))))`  |
| **Unwrap panics** | `.unwrap()` on a `None` or `Err` from malformed input |
| **Integer overflow** | Arithmetic on very large numbers |

---

## Concept 5: Edge Cases

### Catalogue of Edge Cases

Every robust shell must handle these inputs without crashing:

| Category | Input | Expected behaviour |
|----------|-------|--------------------|
| **Empty** | `""` | No-op, exit 0 |
| **Whitespace only** | `"   \t\n  "` | No-op, exit 0 |
| **Very long line** | 1 MB of `a`s | Execute or report error, do not OOM |
| **Binary data** | Bytes `0x00-0xFF` in a pipe | Pass through without corruption |
| **Null bytes** | `echo "a\x00b"` | Handle or reject gracefully |
| **Deeply nested** | 1000 levels of `$($($(..)))` | Stack depth limit, not stack overflow |
| **Unterminated quote** | `echo "hello` | Report error, prompt for continuation |
| **Unterminated subshell** | `echo $(echo` | Report error |
| **Huge argument count** | `echo {1..100000}` | Work or report resource limit |
| **Special filenames** | `touch "file with spaces"` | Handle correctly |
| **Unicode** | `echo "hello"` (emoji/CJK) | Pass through correctly |
| **Backslash at EOF** | `echo hello\` | Continuation line or error |

### Defensive Coding Patterns

**Depth Limiting:**

```rust
const MAX_NESTING_DEPTH: usize = 256;

pub fn eval_nodes_with_depth(
    nodes: &[AstNode],
    env: &mut ShellEnv,
    depth: usize,
) -> i32 {
    if depth > MAX_NESTING_DEPTH {
        eprintln!("jsh: maximum nesting depth exceeded");
        return 1;
    }

    let mut last_status = 0;
    for node in nodes {
        last_status = eval_node_with_depth(node, env, depth + 1);
        if env.control_flow.is_some() {
            break;
        }
    }
    last_status
}
```

**Input Length Limiting:**

```rust
const MAX_LINE_LENGTH: usize = 1_048_576; // 1 MB

pub fn read_line(reader: &mut impl BufRead) -> Result<String, ShellError> {
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line)?;

    if line.len() > MAX_LINE_LENGTH {
        return Err(ShellError::InputTooLong(line.len()));
    }

    Ok(line)
}
```

**Timeout for Loops:**

```rust
use std::time::{Duration, Instant};

pub fn eval_while_safe(
    wl: &WhileLoop,
    env: &mut ShellEnv,
) -> i32 {
    let start = Instant::now();
    let timeout = env.execution_timeout.unwrap_or(Duration::from_secs(300));
    let mut iterations = 0u64;
    let mut last_status = 0;

    loop {
        // Timeout guard.
        if start.elapsed() > timeout {
            eprintln!("jsh: while loop timed out after {:?}", timeout);
            break;
        }

        // Iteration guard.
        iterations += 1;
        if iterations > 10_000_000 {
            eprintln!("jsh: while loop exceeded maximum iterations");
            break;
        }

        let cond = eval_nodes(&wl.condition, env);
        if cond != 0 { break; }

        last_status = eval_nodes(&wl.body, env);
        match env.take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => continue,
            _ => {}
        }
    }

    last_status
}
```

### Testing Edge Cases

```rust
#[cfg(test)]
mod edge_case_tests {
    use super::*;

    #[test]
    fn empty_input_does_not_crash() {
        let mut env = ShellEnv::new();
        assert_eq!(run_input("", &mut env), 0);
    }

    #[test]
    fn whitespace_only_does_not_crash() {
        let mut env = ShellEnv::new();
        assert_eq!(run_input("   \t\n  ", &mut env), 0);
    }

    #[test]
    fn deeply_nested_subshell_hits_limit() {
        let mut env = ShellEnv::new();
        // Build 300 levels of nesting: $($($(...)))
        let input = "$(".repeat(300) + "echo hi" + &")".repeat(300);
        let status = run_input(&input, &mut env);
        // Should not panic; should return non-zero.
        assert_ne!(status, 0);
    }

    #[test]
    fn very_long_word_does_not_oom() {
        let mut env = ShellEnv::new();
        let long_word = "a".repeat(100_000);
        let input = format!("echo {}", long_word);
        // Should not panic or OOM.
        let _ = run_input(&input, &mut env);
    }

    #[test]
    fn unterminated_quote_returns_error() {
        let tokens = lex("echo \"hello");
        // Lexer should handle this gracefully.
        // Depending on design: error token or implicit close.
        assert!(!tokens.is_empty());
    }

    #[test]
    fn unicode_passthrough() {
        let mut env = ShellEnv::new();
        let output = capture_run("echo \"hello\"", &mut env);
        assert!(output.contains("hello"));
    }

    #[test]
    fn null_byte_in_input() {
        let input = "echo hello\x00world";
        let tokens = lex(input);
        // Should not panic. The null may be stripped or treated as a separator.
        assert!(!tokens.is_empty());
    }
}
```

---

## Concept 6: Memory Safety Verification

### Rust's Guarantees

Rust prevents entire classes of bugs at compile time:

| Bug class | How Rust prevents it |
|-----------|---------------------|
| **Buffer overflow** | Bounds-checked indexing; slices know their length |
| **Use after free** | Ownership system; values dropped exactly once |
| **Double free** | Ownership system; only one owner at a time |
| **Null pointer deref** | No null pointers; `Option<T>` instead |
| **Data races** | `Send`/`Sync` traits; borrow checker for references |
| **Dangling references** | Lifetime system prevents references outliving data |

### Auditing `unsafe` Code

If you use `unsafe` anywhere (perhaps for FFI to `libc` on Unix), you should:

1. **Minimise it**: Wrap `unsafe` in a safe API. The unsafe block should be as
   small as possible.

2. **Document the invariants**: Explain *why* the unsafe code is sound.

3. **Test the boundaries**: Write tests that exercise the edge cases of the
   safe wrapper.

```rust
/// Set the process group ID. This is safe because:
/// - `pid` and `pgid` are valid process IDs from the OS.
/// - The call cannot cause memory unsafety; it only affects process state.
/// - Errors are propagated via Result.
#[cfg(unix)]
pub fn set_process_group(pid: i32, pgid: i32) -> Result<(), nix::Error> {
    // SAFETY: setpgid is a POSIX function that does not access memory
    // through the provided arguments. It only modifies kernel state.
    nix::unistd::setpgid(
        nix::unistd::Pid::from_raw(pid),
        nix::unistd::Pid::from_raw(pgid),
    )
}
```

### Using `cargo clippy` and `cargo audit`

```
$ cargo clippy -- -D warnings
# Catches common mistakes, style issues, and potential bugs.

$ cargo install cargo-audit
$ cargo audit
# Checks dependencies for known security vulnerabilities.
```

### Miri for Unsafe Code Verification

If you have `unsafe` blocks, Rust's Miri interpreter can detect undefined
behaviour at runtime:

```
$ rustup component add miri
$ cargo miri test
```

Miri catches:

- Out-of-bounds memory access
- Use of uninitialised memory
- Violations of Stacked Borrows (aliasing rules)
- Memory leaks (optional)

---

## Concept 7: Benchmarking with `criterion`

### Why Benchmark a Shell?

Interactive latency matters. If the prompt takes 50ms to render (because prompt
computation is slow), the shell feels sluggish. Benchmarking ensures that:

- Lexing and parsing are fast even for long scripts.
- Variable expansion does not degrade with many variables.
- Pipeline setup overhead is minimal.

### Setup

```toml
# Cargo.toml

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "shell_bench"
harness = false
```

### Benchmark File

```rust
// benches/shell_bench.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use james_shell::lexer::lex;
use james_shell::parser::Parser;
use james_shell::expand::expand_word;
use james_shell::arith::{ArithParser, eval_arith};
use james_shell::env::ShellEnv;

fn bench_lexer(c: &mut Criterion) {
    let input = "echo hello | grep world && ls -la > output.txt 2>&1 ; cat file";

    c.bench_function("lex_typical_command", |b| {
        b.iter(|| lex(black_box(input)))
    });

    let long_input = "echo ".to_string() + &"word ".repeat(10_000);
    c.bench_function("lex_10k_words", |b| {
        b.iter(|| lex(black_box(&long_input)))
    });
}

fn bench_parser(c: &mut Criterion) {
    let input = "if test -f Cargo.toml { echo yes } else { echo no }";
    let tokens = lex(input);

    c.bench_function("parse_if_else", |b| {
        b.iter(|| {
            let mut parser = Parser::new(black_box(tokens.clone()));
            parser.parse_program().unwrap()
        })
    });
}

fn bench_expand(c: &mut Criterion) {
    let mut env = ShellEnv::new();
    for i in 0..1000 {
        env.set_var(&format!("VAR_{}", i), &format!("value_{}", i));
    }

    c.bench_function("expand_variable_lookup", |b| {
        b.iter(|| expand_word(black_box("$VAR_500"), &env))
    });

    c.bench_function("expand_many_variables", |b| {
        let input = (0..100)
            .map(|i| format!("$VAR_{}", i))
            .collect::<Vec<_>>()
            .join(" ");
        b.iter(|| expand_word(black_box(&input), &env))
    });
}

fn bench_arithmetic(c: &mut Criterion) {
    let env = ShellEnv::new();

    c.bench_function("arith_simple", |b| {
        b.iter(|| {
            let mut p = ArithParser::new(black_box("1 + 2 * 3"));
            let expr = p.parse_expr().unwrap();
            eval_arith(&expr, &env).unwrap()
        })
    });

    c.bench_function("arith_complex", |b| {
        b.iter(|| {
            let mut p = ArithParser::new(
                black_box("((1 + 2) * (3 - 4)) / (5 + 6) + 7 * 8 - 9")
            );
            let expr = p.parse_expr().unwrap();
            eval_arith(&expr, &env).unwrap()
        })
    });
}

criterion_group!(
    benches,
    bench_lexer,
    bench_parser,
    bench_expand,
    bench_arithmetic,
);
criterion_main!(benches);
```

### Running Benchmarks

```
$ cargo bench
   Compiling james-shell v0.1.0
    Finished bench target(s)
     Running benches/shell_bench.rs

lex_typical_command     time:   [845 ns 852 ns 860 ns]
lex_10k_words           time:   [1.23 ms 1.25 ms 1.27 ms]
parse_if_else           time:   [423 ns 428 ns 434 ns]
expand_variable_lookup  time:   [45 ns 46 ns 47 ns]
expand_many_variables   time:   [4.12 us 4.18 us 4.25 us]
arith_simple            time:   [112 ns 114 ns 116 ns]
arith_complex           time:   [287 ns 291 ns 296 ns]
```

Criterion also generates HTML reports in `target/criterion/` with charts showing
performance over time. This is invaluable for detecting regressions.

### Performance Budgets

Set expectations for interactive responsiveness:

| Operation | Budget |
|-----------|--------|
| Lex + parse a typical command | < 10 us |
| Expand variables (typical prompt) | < 50 us |
| Full prompt render | < 1 ms |
| Pipeline setup (2-3 processes) | < 5 ms |
| Source a 100-line `.jshrc` | < 50 ms |

---

## Concept 8: CI/CD with GitHub Actions

### Workflow File

```yaml
# .github/workflows/ci.yml

name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
        rust: [stable, nightly]
    runs-on: ${{ matrix.os }}

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.rust }}
          components: clippy, rustfmt

      - name: Cache cargo registry and build
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ matrix.rust }}-${{ hashFiles('**/Cargo.lock') }}

      - name: Check formatting
        run: cargo fmt -- --check

      - name: Run clippy
        run: cargo clippy -- -D warnings

      - name: Run unit tests
        run: cargo test --lib

      - name: Run integration tests
        run: cargo test --test integration

      - name: Run doc tests
        run: cargo test --doc

  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Run benchmarks (no regression check, just ensure they compile)
        run: cargo bench --no-run

  security:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Install cargo-audit
        run: cargo install cargo-audit

      - name: Audit dependencies
        run: cargo audit

  fuzz:
    runs-on: ubuntu-latest
    # Only run on main branch merges, not on every PR (fuzzing is slow).
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly

      - name: Install cargo-fuzz
        run: cargo install cargo-fuzz

      - name: Fuzz lexer (5 minutes)
        run: cargo fuzz run fuzz_lexer -- -max_total_time=300
        continue-on-error: true

      - name: Fuzz parser (5 minutes)
        run: cargo fuzz run fuzz_parser -- -max_total_time=300
        continue-on-error: true

      - name: Upload crash artifacts
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: fuzz-crashes
          path: fuzz/artifacts/
```

### What This CI Pipeline Covers

| Job | What it checks |
|-----|---------------|
| **test** | Formatting, lints, unit tests, integration tests on 3 OSes x 2 Rust versions |
| **bench** | Benchmarks compile (no regression tracking in this basic setup) |
| **security** | No known vulnerabilities in dependencies |
| **fuzz** | 5-minute fuzz sessions for lexer and parser (main branch only) |

### Badge

Add a status badge to your README:

```markdown
[![CI](https://github.com/yourname/james-shell/actions/workflows/ci.yml/badge.svg)](https://github.com/yourname/james-shell/actions/workflows/ci.yml)
```

---

## Concept 9: Custom Test Harness for Shell Scripts

### The Problem

Integration tests with `assert_cmd` are great for individual commands. But
testing complex multi-line scripts (with expected output, expected exit codes,
and expected stderr) requires a more structured approach.

### Test File Format

We define a simple format for test cases. Each `.test` file contains input,
expected output, and metadata:

```
# tests/scripts/for_loop.test

## Description: For loop iterates over items

## Input:
for x in alpha beta gamma {
    echo "item: $x"
}

## Expected stdout:
item: alpha
item: beta
item: gamma

## Expected exit code: 0
```

### Test Runner

```rust
// tests/harness.rs

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
struct TestCase {
    name: String,
    description: String,
    input: String,
    expected_stdout: String,
    expected_stderr: Option<String>,
    expected_exit_code: i32,
}

fn parse_test_file(path: &Path) -> TestCase {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));

    let mut description = String::new();
    let mut input = String::new();
    let mut expected_stdout = String::new();
    let mut expected_stderr = None;
    let mut expected_exit_code = 0;
    let mut current_section = "";

    for line in content.lines() {
        if line.starts_with("## Description:") {
            description = line.trim_start_matches("## Description:").trim().to_string();
            current_section = "";
        } else if line == "## Input:" {
            current_section = "input";
        } else if line == "## Expected stdout:" {
            current_section = "stdout";
        } else if line == "## Expected stderr:" {
            current_section = "stderr";
            expected_stderr = Some(String::new());
        } else if line.starts_with("## Expected exit code:") {
            let code_str = line.trim_start_matches("## Expected exit code:").trim();
            expected_exit_code = code_str.parse().unwrap_or(0);
            current_section = "";
        } else if line.starts_with('#') {
            // Comment line, skip.
        } else {
            match current_section {
                "input" => {
                    if !input.is_empty() { input.push('\n'); }
                    input.push_str(line);
                }
                "stdout" => {
                    if !expected_stdout.is_empty() { expected_stdout.push('\n'); }
                    expected_stdout.push_str(line);
                }
                "stderr" => {
                    if let Some(ref mut s) = expected_stderr {
                        if !s.is_empty() { s.push('\n'); }
                        s.push_str(line);
                    }
                }
                _ => {}
            }
        }
    }

    TestCase {
        name: path.file_stem().unwrap().to_string_lossy().to_string(),
        description,
        input,
        expected_stdout,
        expected_stderr,
        expected_exit_code,
    }
}

fn run_test_case(tc: &TestCase) -> Result<(), String> {
    let bin = env!("CARGO_BIN_EXE_jsh");

    let output = Command::new(bin)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(tc.input.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| format!("failed to run jsh: {}", e))?;

    let actual_stdout = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    let actual_stderr = String::from_utf8_lossy(&output.stderr)
        .trim_end()
        .to_string();
    let actual_exit_code = output.status.code().unwrap_or(-1);

    let mut errors = Vec::new();

    // Check exit code.
    if actual_exit_code != tc.expected_exit_code {
        errors.push(format!(
            "exit code: expected {}, got {}",
            tc.expected_exit_code, actual_exit_code
        ));
    }

    // Check stdout.
    let expected_stdout = tc.expected_stdout.trim_end();
    if actual_stdout != expected_stdout {
        errors.push(format!(
            "stdout mismatch:\n  expected: {:?}\n  actual:   {:?}",
            expected_stdout, actual_stdout
        ));
    }

    // Check stderr (if specified).
    if let Some(ref expected_stderr) = tc.expected_stderr {
        let expected = expected_stderr.trim_end();
        if !actual_stderr.contains(expected) {
            errors.push(format!(
                "stderr mismatch:\n  expected to contain: {:?}\n  actual: {:?}",
                expected, actual_stderr
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

/// Discover and run all .test files in a directory.
pub fn run_test_suite(dir: &Path) -> (usize, usize, Vec<String>) {
    let mut passed = 0;
    let mut failed = 0;
    let mut failures = Vec::new();

    let mut test_files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("cannot read test directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|e| e == "test").unwrap_or(false))
        .collect();

    test_files.sort();

    for path in &test_files {
        let tc = parse_test_file(path);
        print!("  {} ({}) ... ", tc.name, tc.description);

        match run_test_case(&tc) {
            Ok(()) => {
                println!("ok");
                passed += 1;
            }
            Err(msg) => {
                println!("FAILED");
                failures.push(format!("  {} FAILED:\n{}", tc.name, msg));
                failed += 1;
            }
        }
    }

    (passed, failed, failures)
}
```

### Invoking the Harness from Cargo Tests

```rust
// tests/script_tests.rs

use std::path::Path;

mod harness;

#[test]
fn run_all_script_tests() {
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scripts");

    if !test_dir.exists() {
        eprintln!("No test scripts directory found at {:?}, skipping", test_dir);
        return;
    }

    let (passed, failed, failures) = harness::run_test_suite(&test_dir);

    println!("\n{} passed, {} failed", passed, failed);

    if !failures.is_empty() {
        println!("\nFailures:");
        for f in &failures {
            println!("{}", f);
        }
        panic!("{} script tests failed", failed);
    }
}
```

### Example Test Files

**tests/scripts/echo_basic.test:**

```
## Description: Echo prints its arguments

## Input:
echo hello world

## Expected stdout:
hello world

## Expected exit code: 0
```

**tests/scripts/variable.test:**

```
## Description: Variable assignment and expansion

## Input:
let name = james
echo "hello $name"

## Expected stdout:
hello james

## Expected exit code: 0
```

**tests/scripts/pipeline.test:**

```
## Description: Pipeline connects stdout to stdin

## Input:
echo -e "banana\napple\ncherry" | sort

## Expected stdout:
apple
banana
cherry

## Expected exit code: 0
```

**tests/scripts/if_else.test:**

```
## Description: If-else takes the correct branch

## Input:
if test 1 -eq 2 {
    echo "wrong"
} else {
    echo "correct"
}

## Expected stdout:
correct

## Expected exit code: 0
```

**tests/scripts/error_handling.test:**

```
## Description: Nonexistent command returns error

## Input:
nonexistent_command_12345

## Expected stdout:

## Expected stderr:
not found

## Expected exit code: 127
```

### Running the Harness

```
$ cargo test run_all_script_tests -- --nocapture
   Compiling james-shell v0.1.0
    Finished test target(s)
     Running tests/script_tests.rs

  echo_basic (Echo prints its arguments) ... ok
  variable (Variable assignment and expansion) ... ok
  pipeline (Pipeline connects stdout to stdin) ... ok
  if_else (If-else takes the correct branch) ... ok
  error_handling (Nonexistent command returns error) ... ok

5 passed, 0 failed
```

---

## Key Rust Concepts Used

| Concept | Where it appears |
|---------|-----------------|
| **`#[cfg(test)]`** | Conditionally compiling test modules so they do not affect the release binary |
| **`#[test]` attribute** | Marking functions as tests for `cargo test` |
| **`assert_eq!`, `assert!`, `assert_ne!`** | Standard assertion macros in unit tests |
| **`dev-dependencies`** | `assert_cmd`, `predicates`, `tempfile`, `criterion` only included in test/bench builds |
| **`Command` (std::process)** | Spawning the shell binary for integration tests |
| **`tempfile` crate** | Creating isolated filesystem state for tests |
| **`criterion` crate** | Statistical benchmarking with warmup and iteration |
| **`black_box`** | Preventing the compiler from optimising away benchmarked code |
| **Custom test harness** | The `.test` file parser and runner |
| **Pattern matching on strings** | Parsing test file sections |
| **`Result<T, E>` propagation** | Error handling in test helpers |
| **Closures in benchmarks** | `b.iter(\|\| ...)` in criterion benchmarks |
| **Feature flags** | `cargo fuzz` uses nightly features |

---

## Milestone

After implementing this module, your CI pipeline and local development workflow
should look like this:

```
$ cargo fmt -- --check
# No formatting issues.

$ cargo clippy -- -D warnings
# No warnings.

$ cargo test
running 28 tests              (unit tests)
test result: ok. 28 passed; 0 failed

running 14 tests              (integration tests)
test result: ok. 14 passed; 0 failed

running 1 test                (script harness)
  echo_basic ... ok
  variable ... ok
  pipeline ... ok
  if_else ... ok
  for_loop ... ok
  error_handling ... ok
  function_call ... ok
  arithmetic ... ok
  command_substitution ... ok
  subshell ... ok
10 passed, 0 failed
test result: ok. 1 passed; 0 failed

$ cargo bench
lex_typical_command     time:   [845 ns 852 ns 860 ns]
parse_if_else           time:   [423 ns 428 ns 434 ns]
expand_variable_lookup  time:   [45 ns 46 ns 47 ns]
arith_simple            time:   [112 ns 114 ns 116 ns]

$ cargo fuzz run fuzz_lexer -- -max_total_time=60
# ... 60 seconds of fuzzing, no crashes found.

$ gh pr create --title "Add testing infrastructure"
# CI runs: all green across Linux, Windows, macOS.
```

Your GitHub Actions dashboard shows:

```
CI                                    main    2m 34s    passed
  test (ubuntu-latest, stable)        ..................  passed
  test (ubuntu-latest, nightly)       ..................  passed
  test (windows-latest, stable)       ..................  passed
  test (windows-latest, nightly)      ..................  passed
  test (macos-latest, stable)         ..................  passed
  test (macos-latest, nightly)        ..................  passed
  bench                               ..................  passed
  security                            ..................  passed
  fuzz                                ..................  passed
```

---

## What's Next?

With a solid testing foundation, you are ready to take james-shell further.
Potential directions include:

- **Module 14: Plugin System** -- Load extensions as dynamic libraries (`.so` /
  `.dll`) or WebAssembly modules.
- **Module 15: Line Editor** -- Build a custom line editor with syntax
  highlighting, multi-line editing, and fuzzy history search.
- **Module 16: Completion Engine** -- Tab completion for commands, file paths,
  git branches, and custom completers.

Each of these can be built on the tested, robust foundation you have established
in this module. Ship with confidence.
