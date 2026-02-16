# Recommended Resources for james-shell

A curated reading list organized by topic. Each resource includes a description
of why it matters for this project and which modules benefit most from it.

---

## Books

### The Rust Programming Language ("The Book")
- **Authors**: Steve Klabnik and Carol Nichols
- **Link**: [https://doc.rust-lang.org/book/](https://doc.rust-lang.org/book/)
- **Why**: The authoritative introduction to Rust; covers ownership, enums, pattern matching, error handling, and traits -- all foundational to every module in james-shell.
- **Relevant modules**: All (1-20). Especially 1-3 for getting started with Rust idioms.

### Programming Rust, 2nd Edition
- **Authors**: Jim Blandy, Jason Orendorff, and Leonora Tindall
- **Publisher**: O'Reilly Media
- **Why**: Goes deeper than The Book on systems-level topics like memory layout, concurrency, and unsafe code, which matter when we implement process management and signal handling.
- **Relevant modules**: 7 (pipes), 8 (job control), 9 (signals), 20 (plugins/dynamic loading).

### Advanced Programming in the UNIX Environment, 3rd Edition (APUE)
- **Authors**: W. Richard Stevens and Stephen A. Rago
- **Publisher**: Addison-Wesley
- **Why**: The definitive reference for Unix system calls: fork, exec, pipe, dup2, signal handling, terminal I/O, and process groups. Essential for understanding what the shell actually does at the OS level.
- **Relevant modules**: 3 (execution), 6 (redirection), 7 (pipes), 8 (job control), 9 (signals).

### The Linux Programming Interface
- **Author**: Michael Kerrisk
- **Publisher**: No Starch Press
- **Why**: A modern, comprehensive alternative to APUE with excellent coverage of process creation, IPC, signals, and terminal drivers. The chapters on process groups and sessions are invaluable for job control.
- **Relevant modules**: 3 (execution), 7 (pipes), 8 (job control), 9 (signals).

### Crafting Interpreters
- **Author**: Robert Nystrom
- **Link**: [https://craftinginterpreters.com/](https://craftinginterpreters.com/)
- **Why**: Teaches lexer and parser design from scratch using a clean, practical approach. The tree-walk interpreter maps almost directly onto how a shell evaluates an AST. The chapters on scanning, parsing expressions, and control flow are directly applicable.
- **Relevant modules**: 2 (parsing/AST), 5 (expansion), 11 (scripting/control flow), 19 (modern scripting).

### Writing An Interpreter In Go
- **Author**: Thorsten Ball
- **Publisher**: Self-published
- **Why**: A compact, hands-on guide to building a complete interpreter with REPL, lexer, parser, and evaluator. The step-by-step approach mirrors our modular curriculum.
- **Relevant modules**: 1 (REPL), 2 (parsing), 3 (execution), 11 (scripting).

---

## Online Resources

### Rust by Example
- **Link**: [https://doc.rust-lang.org/rust-by-example/](https://doc.rust-lang.org/rust-by-example/)
- **Why**: Learn Rust concepts through short, runnable examples. Great for quickly looking up how to use a specific feature (e.g., `match`, iterators, traits) while implementing a module.
- **Relevant modules**: All.

### The Rustonomicon (The Dark Arts of Unsafe Rust)
- **Link**: [https://doc.rust-lang.org/nomicon/](https://doc.rust-lang.org/nomicon/)
- **Why**: When we need raw pointers for signal handlers or FFI with platform APIs, this is the guide to doing it correctly. Should be consulted sparingly and only when safe abstractions are insufficient.
- **Relevant modules**: 9 (signals), 20 (plugins/FFI).

### Writing an OS in Rust
- **Author**: Philipp Oppermann
- **Link**: [https://os.phil-opp.com/](https://os.phil-opp.com/)
- **Why**: While we are not writing an OS, the systems programming concepts (memory management, interrupts, process isolation) build intuition for how shells interact with the operating system.
- **Relevant modules**: 8 (job control), 9 (signals), background systems knowledge.

### Build Your Own Shell (Codecrafters)
- **Link**: [https://app.codecrafters.io/courses/shell/overview](https://app.codecrafters.io/courses/shell/overview)
- **Why**: A guided challenge to build a POSIX shell step by step; useful as a sanity check for our own implementation order and to see how others approach the same problem.
- **Relevant modules**: 1-3 (REPL, parsing, execution).

### Tutorial - Write a Shell in C
- **Author**: Stephen Brennan
- **Link**: [https://brennan.io/2015/01/16/write-a-shell-in-c/](https://brennan.io/2015/01/16/write-a-shell-in-c/)
- **Why**: A concise walkthrough of a minimal shell in C (read, parse, fork/exec). Translating the concepts to Rust is a valuable exercise for Modules 1-3.
- **Relevant modules**: 1 (REPL loop), 2 (parsing), 3 (execution).

### The Architecture of Open Source Applications: The Bourne-Again Shell
- **Link**: [https://aosabook.org/en/v1/bash.html](https://aosabook.org/en/v1/bash.html)
- **Why**: An insider explanation of bash's architecture by its maintainer, Chet Ramey. Covers the interaction between the parser, word expansion, and command execution in detail.
- **Relevant modules**: 2 (parsing), 5 (expansion), 3 (execution), 11 (scripting).

### POSIX Shell Command Language Specification
- **Link**: [https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html](https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html)
- **Why**: The formal specification for shell syntax and semantics. Essential reference when deciding which behaviors to replicate for POSIX compatibility and which to intentionally deviate from.
- **Relevant modules**: 2 (parsing), 3 (execution), 5 (expansion), 6 (redirection), 11 (scripting).

### Error Handling in Rust (Burntsushi's blog post)
- **Author**: Andrew Gallant (BurntSushi)
- **Link**: [https://blog.burntsushi.net/rust-error-handling/](https://blog.burntsushi.net/rust-error-handling/)
- **Why**: The classic guide to Rust error handling patterns: custom error types, the `?` operator, `From` implementations, and `thiserror`/`anyhow`. Directly informs our `ShellError` design.
- **Relevant modules**: 18 (error handling), but relevant from Module 1 onward.

---

## Shell Source Code to Study

### nushell (Rust, structured data)
- **Repository**: [https://github.com/nushell/nushell](https://github.com/nushell/nushell)
- **Why**: The closest existing project to what james-shell aims to become. Nushell pioneered structured data pipelines in a shell. Studying its architecture reveals both brilliant ideas to adopt and complexity to avoid.
- **Key files to read**:
  - `crates/nu-parser/src/parser.rs` -- How nushell parses commands and expressions.
  - `crates/nu-protocol/src/value/mod.rs` -- The `Value` enum: nushell's core data type.
  - `crates/nu-protocol/src/pipeline_data.rs` -- How structured data flows through pipelines.
  - `crates/nu-engine/src/eval.rs` -- The evaluator that executes parsed AST nodes.
  - `crates/nu-command/src/filters/` -- Built-in filter commands (`where`, `select`, `sort-by`) that operate on structured data.
  - `crates/nu-plugin/` -- The plugin protocol and interface.
- **Relevant modules**: 14 (structured types), 15 (typed pipelines), 16 (data parsers), 20 (plugins).

### fish (C++, great UX)
- **Repository**: [https://github.com/fish-shell/fish-shell](https://github.com/fish-shell/fish-shell)
- **Why**: Fish is the gold standard for shell user experience: syntax highlighting, autosuggestions, and tab completions that work out of the box. Study it for UX inspiration, not for code to port.
- **What to learn from it**:
  - `src/highlight.cpp` -- How fish does live syntax highlighting as you type.
  - `src/complete.cpp` -- The completion engine: how it generates context-aware suggestions.
  - `src/reader.cpp` -- The interactive line reader with autosuggestions.
  - `src/parse_tree.cpp` -- Fish's approach to parsing (concrete syntax tree).
  - The overall philosophy: "every feature should work by default" without configuration.
- **Relevant modules**: 10 (line editing), 17 (completions), and general UX decisions.

### bash
- **Repository**: [https://git.savannah.gnu.org/cgit/bash.git](https://git.savannah.gnu.org/cgit/bash.git)
- **Why**: The most widely used shell. Understanding bash internals helps us know what behaviors users expect and why certain things are the way they are.
- **Key files for understanding traditional shell internals**:
  - `parse.y` -- The YACC grammar: shows how complex shell syntax really is.
  - `execute_cmd.c` -- Command execution: fork, exec, pipes, redirections.
  - `subst.c` -- Word expansion (variable substitution, globbing, tilde expansion). This is one of the most complex files in bash.
  - `jobs.c` -- Job control: process groups, foreground/background, wait.
  - `sig.c` -- Signal handling.
  - `redir.c` -- I/O redirection implementation.
- **Relevant modules**: 3 (execution), 5 (expansion), 6 (redirection), 7 (pipes), 8 (job control), 9 (signals).

### dash (Debian Almquist Shell)
- **Repository**: [https://git.kernel.org/pub/scm/utils/dash/dash.git](https://git.kernel.org/pub/scm/utils/dash/dash.git)
- **Why**: A minimal POSIX shell that is much easier to read than bash (~15k lines vs ~150k). If bash's source feels overwhelming, start with dash to understand the same concepts in a simpler codebase.
- **Key files**:
  - `src/parser.c` -- A hand-written recursive descent parser (no YACC).
  - `src/eval.c` -- Command evaluation.
  - `src/exec.c` -- Process execution.
  - `src/redir.c` -- Redirection.
  - `src/expand.c` -- Word expansion.
- **Relevant modules**: 2 (parsing), 3 (execution), 5 (expansion), 6 (redirection).

### ion (Rust shell, Redox OS)
- **Repository**: [https://gitlab.redox-os.org/redox-os/ion](https://gitlab.redox-os.org/redox-os/ion)
- **Why**: Another shell written in Rust, simpler than nushell. Useful for seeing how Rust idioms apply to shell internals (process spawning, signal handling, etc.) without the complexity of structured data.
- **Relevant modules**: 1-9 (basic shell infrastructure).

---

## Key Documentation (Crate Docs)

### std::process (Rust standard library)
- **Link**: [https://doc.rust-lang.org/std/process/index.html](https://doc.rust-lang.org/std/process/index.html)
- **Why**: The primary API for spawning and managing child processes. `Command`, `Child`, `Stdio`, and `ExitStatus` are used directly in the executor.
- **Relevant modules**: 3 (execution), 7 (pipes), 8 (job control).

### crossterm
- **Link**: [https://docs.rs/crossterm/latest/crossterm/](https://docs.rs/crossterm/latest/crossterm/)
- **Why**: Cross-platform terminal manipulation (raw mode, colors, cursor movement, event reading). Powers our line editor and syntax highlighting.
- **Relevant modules**: 10 (line editing), highlighter, and any terminal UI features.

### rustyline
- **Link**: [https://docs.rs/rustyline/latest/rustyline/](https://docs.rs/rustyline/latest/rustyline/)
- **Why**: A readline-like library for Rust. Provides line editing, history, and completion out of the box. We may use it directly in early modules and replace with a custom editor later.
- **Relevant modules**: 1 (REPL), 10 (line editing), 17 (completions).

### nix (Unix API bindings for Rust)
- **Link**: [https://docs.rs/nix/latest/nix/](https://docs.rs/nix/latest/nix/)
- **Why**: Safe Rust wrappers around Unix system calls (fork, exec, pipe, dup2, signal, tcsetpgrp). Avoids writing raw `unsafe` blocks for common syscalls.
- **Relevant modules**: 3 (execution), 7 (pipes), 8 (job control), 9 (signals).

### windows-rs (Windows API bindings for Rust)
- **Link**: [https://docs.rs/windows/latest/windows/](https://docs.rs/windows/latest/windows/)
- **Why**: Official Microsoft Rust bindings for Windows APIs. Needed for Windows-specific process management, console handling, and job objects.
- **Relevant modules**: 3 (execution on Windows), 8 (job control on Windows), 9 (signals/ctrl handler on Windows).

### serde and serde_json
- **Link**: [https://docs.rs/serde/latest/serde/](https://docs.rs/serde/latest/serde/) and [https://docs.rs/serde_json/latest/serde_json/](https://docs.rs/serde_json/latest/serde_json/)
- **Why**: Serialization/deserialization framework. Used to convert between `Value` and external formats (JSON, TOML, etc.) in Module 16.
- **Relevant modules**: 14 (structured types), 16 (data parsers), 20 (plugin protocol).

### glob
- **Link**: [https://docs.rs/glob/latest/glob/](https://docs.rs/glob/latest/glob/)
- **Why**: File path glob matching. Used in the expander for wildcard expansion (`*.rs`, `src/**/*.rs`).
- **Relevant modules**: 5 (expansion).

### clap
- **Link**: [https://docs.rs/clap/latest/clap/](https://docs.rs/clap/latest/clap/)
- **Why**: Command-line argument parsing for the shell binary itself (flags like `--config`, `-c "command"`, script file arguments).
- **Relevant modules**: 1 (main.rs argument handling).

### thiserror and miette
- **Link**: [https://docs.rs/thiserror/latest/thiserror/](https://docs.rs/thiserror/latest/thiserror/) and [https://docs.rs/miette/latest/miette/](https://docs.rs/miette/latest/miette/)
- **Why**: `thiserror` for deriving `Error` implementations on `ShellError`. `miette` for beautiful diagnostic output with source spans, arrows, and suggestions.
- **Relevant modules**: 18 (error handling), but used from Module 1 onward.

### libloading
- **Link**: [https://docs.rs/libloading/latest/libloading/](https://docs.rs/libloading/latest/libloading/)
- **Why**: Cross-platform dynamic library loading for native plugins (`.so` / `.dll` / `.dylib`).
- **Relevant modules**: 20 (plugins).

---

## Videos and Talks

### "Implementing a Language" - Jonathan Turner (nushell creator)
- **Link**: Search for Jonathan Turner's talks on nushell design and implementation.
- **Why**: First-hand explanation of the design decisions behind nushell's structured data model, which directly informs our Module 14-16 approach.
- **Relevant modules**: 14 (structured types), 15 (typed pipelines).

### "How Rust Makes Advanced Shell Scripting Possible" - various RustConf/RustFest talks
- **Why**: Talks about using Rust's type system for systems programming tasks like shell building. Look for talks that cover process management and error handling in Rust.
- **Relevant modules**: 3 (execution), 18 (error handling).

### "A Cartoon Intro to WebAssembly" - Lin Clark
- **Link**: [https://hacks.mozilla.org/2017/02/a-cartoon-intro-to-webassembly/](https://hacks.mozilla.org/2017/02/a-cartoon-intro-to-webassembly/)
- **Why**: If we pursue WASM-based plugins (an alternative to native dynamic loading), understanding WASM fundamentals helps. This is the most accessible introduction.
- **Relevant modules**: 20 (plugins, if using WASM sandboxing).

### "Building a Shell" - Josh Triplett (LPC / various Linux conferences)
- **Why**: Covers the practical challenges of implementing POSIX shell features correctly, including edge cases in parsing, expansion, and process management.
- **Relevant modules**: 2-9 (core shell infrastructure).

### "Terminals, Shells, and Command Lines" - various conference talks
- **Why**: Understanding the terminal emulator <-> shell <-> kernel relationship clarifies why certain things work the way they do (raw mode, job control, TIOCSPGRP, etc.).
- **Relevant modules**: 8 (job control), 9 (signals), 10 (line editing).

### RustConf and Rust Belt Rust recorded talks
- **Link**: Available on YouTube; search for talks on systems programming, error handling, and async Rust.
- **Why**: General Rust ecosystem knowledge and patterns that apply across the project.
- **Relevant modules**: All.

---

## Supplementary References

### Wikipedia: Pipeline (Unix)
- **Link**: [https://en.wikipedia.org/wiki/Pipeline_(Unix)](https://en.wikipedia.org/wiki/Pipeline_(Unix))
- **Why**: Quick overview of the history and mechanics of Unix pipes.
- **Relevant modules**: 7 (pipes).

### "The TTY Demystified" - Linus Akesson
- **Link**: [https://www.linusakesson.net/programming/tty/](https://www.linusakesson.net/programming/tty/)
- **Why**: The single best explanation of how terminals, TTYs, process groups, sessions, and job control fit together. Essential reading before implementing Modules 8-9.
- **Relevant modules**: 8 (job control), 9 (signals), 10 (line editing).

### "Anatomy of a Terminal Emulator" - poor.dev
- **Link**: [https://poor.dev/blog/terminal-anatomy/](https://poor.dev/blog/terminal-anatomy/)
- **Why**: Explains the relationship between terminal emulators, PTYs, and shells, which helps understand the environment our shell operates in.
- **Relevant modules**: 10 (line editing), general background.

### Bash Reference Manual (GNU)
- **Link**: [https://www.gnu.org/software/bash/manual/bash.html](https://www.gnu.org/software/bash/manual/bash.html)
- **Why**: The definitive documentation for bash behavior. When users report "bash does X, why doesn't james-shell?", this is where to look.
- **Relevant modules**: All (as a compatibility reference).

---

## Suggested Study Order

For the best learning experience, study these resources in parallel with the
modules:

| Modules | Priority Reading                                              |
|---------|---------------------------------------------------------------|
| 1-3     | The Rust Book, Crafting Interpreters (ch. 4-8), Brennan's shell tutorial |
| 4-5     | POSIX spec (word expansion), bash manual (builtins)           |
| 6-7     | APUE (ch. 3, 15), "The TTY Demystified"                      |
| 8-9     | APUE (ch. 9, 10), "The TTY Demystified", nix crate docs      |
| 10      | crossterm/rustyline docs, fish source (reader.cpp)            |
| 11      | Crafting Interpreters (ch. 9-13), POSIX spec (compound cmds)  |
| 12-13   | Bash reference manual, testing chapters in Programming Rust   |
| 14-16   | nushell source (Value, PipelineData), serde docs              |
| 17      | fish source (complete.cpp), rustyline Completer trait          |
| 18      | miette docs, BurntSushi error handling post                   |
| 19      | Crafting Interpreters (ch. 22-24), nushell closures            |
| 20      | libloading docs, nushell plugin protocol, WASM docs (optional)|
