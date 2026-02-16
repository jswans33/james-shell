# James-Shell Glossary

A glossary of systems programming, shell, and Rust concepts relevant to the james-shell project. Terms are organized alphabetically. The module references in parentheses indicate where the concept is most directly applied.

---

**Abstract Syntax Tree (AST)**
A tree-shaped data structure that represents the hierarchical syntactic structure of a parsed command. Each node in the tree corresponds to a construct in the shell grammar, such as a command, a pipeline, or a redirection. The AST is produced by the parser and consumed by the executor.
_(Modules 2, 3)_

**Alias**
A user-defined shorthand that the shell expands into a longer command string before parsing. Alias expansion typically occurs very early in the processing pipeline, before most other forms of expansion.
_(Module 5)_

**Argument / Argument vector (argv)**
An argument is a value passed to a command or function. The argument vector (argv) is the array of strings passed to a program when it is executed; by convention, `argv[0]` is the name of the program itself and the remaining elements are the arguments supplied by the user.
_(Modules 3, 4)_

**Async / Await**
Rust keywords for writing asynchronous code. `async` marks a function or block as returning a `Future`, and `.await` suspends execution until that future resolves. Useful in a shell for non-blocking I/O and concurrent task management.
_(Modules 8, 12)_

**Background process**
A process that runs without occupying the terminal's foreground, allowing the user to continue entering commands. In most shells, appending `&` to a command launches it in the background. The shell tracks background processes as part of its job control system.
_(Module 8)_

**Buffered I/O**
A technique where data is accumulated in an intermediate memory buffer before being read from or written to a file descriptor. Buffered I/O reduces the number of system calls, improving performance. Rust's `BufReader` and `BufWriter` types provide this capability.
_(Modules 6, 7)_

**Builtin command**
A command that is implemented directly inside the shell process rather than as an external executable. Builtins like `cd`, `exit`, and `export` must run inside the shell because they need to modify the shell's own state (e.g., current directory, environment variables).
_(Module 4)_

**Canonical mode (cooked mode)**
The default terminal input mode in which the terminal driver buffers input line by line, providing basic line-editing features (backspace, Ctrl-U, etc.) before delivering the completed line to the reading process. Contrast with raw mode.
_(Module 10)_

**Child process**
A process created by another process (the parent) via a system call such as `fork()` or `CreateProcess()`. In a shell, every external command is run in a child process so that the shell itself continues to operate.
_(Module 3)_

**Closure**
An anonymous function that can capture variables from its enclosing scope. In Rust, closures are defined with `|args| body` syntax and implement one or more of the `Fn`, `FnMut`, or `FnOnce` traits depending on how they use captured values.
_(Modules 3, 5, 17)_

**Command substitution**
A shell feature that executes a command and replaces the substitution expression with the command's standard output. Typically written as `$(command)` or with backticks. The shell runs the inner command in a subshell and captures its stdout.
_(Module 5)_

**Crate**
The unit of compilation and distribution in Rust. A crate can be a binary (producing an executable) or a library (producing reusable code). External crates are published to crates.io and managed via Cargo.
_(All modules)_

**Daemon**
A background process that runs continuously, detached from any controlling terminal, typically providing a system service. Daemons are created by forking and then detaching the child process from the terminal session.
_(Modules 8, 9)_

**Enum (Rust)**
A type that can be one of several named variants, each of which may carry different data. Rust enums are used extensively in shell design to represent command types, token kinds, and results (`Option`, `Result`).
_(Modules 2, 3, 18)_

**Environment variable**
A key-value pair maintained by the operating system and inherited by child processes. Environment variables like `PATH`, `HOME`, and `USER` configure program behavior. Shells provide builtins (`export`, `unset`) to manipulate them.
_(Modules 4, 5)_

**EOF (End of File)**
A condition signaling that there is no more data to read from a file or stream. In an interactive shell, the user can send EOF with Ctrl-D. The shell's REPL loop uses EOF as a signal to exit gracefully.
_(Modules 1, 6)_

**Escape character**
A character (commonly `\`) that removes the special meaning of the character that follows it. In shell syntax, escaping prevents wildcard expansion, whitespace splitting, and other interpretation. In terminal control, escape sequences (beginning with `\x1b`) control cursor position, color, and other display attributes.
_(Modules 2, 5, 10)_

**Exit code / Exit status**
An integer value returned by a process when it terminates, indicating success or failure. By convention, `0` means success and any non-zero value indicates an error. The shell stores the most recent exit code in the special variable `$?`.
_(Modules 3, 4, 18)_

**File descriptor**
An integer handle used by the operating system to identify an open file, socket, pipe, or other I/O resource within a process. The three standard file descriptors are 0 (stdin), 1 (stdout), and 2 (stderr).
_(Modules 6, 7)_

**Foreground process**
The process (or process group) currently connected to the terminal for input and output. Only one process group can be in the foreground at a time. The shell places each command it runs into the foreground unless the user requests background execution.
_(Module 8)_

**Fork**
A Unix system call that creates a new child process by duplicating the calling (parent) process. After `fork()`, both parent and child continue executing from the same point, but with different return values. The child typically calls `exec()` to replace itself with a new program.
_(Module 3)_

**Glob / Globbing**
A pattern-matching mechanism for filenames using wildcard characters such as `*` (any sequence), `?` (any single character), and `[...]` (character class). The shell expands glob patterns into lists of matching file paths before executing the command.
_(Module 5)_

**Handle (Windows)**
The Windows equivalent of a file descriptor. A `HANDLE` is an opaque value returned by the OS to represent an open resource such as a file, pipe, process, or thread.
_(Modules 3, 6, 7)_

**Here document / Here string**
A here document (`<<DELIMITER`) is a form of redirection that provides multi-line input inline within a script. A here string (`<<<word`) feeds a single string to a command's standard input. Both avoid the need for temporary files or echo-pipe constructions.
_(Module 6)_

**Interprocess Communication (IPC)**
Any mechanism that allows separate processes to exchange data. Common IPC methods include pipes, sockets, shared memory, message queues, and signals. Shells rely heavily on pipes and signals for IPC.
_(Modules 7, 9)_

**Iterator (Rust)**
A trait providing a sequence of values via the `next()` method. Rust iterators are lazy (they do not compute values until consumed) and support a rich set of adapter methods like `map`, `filter`, `collect`, and `fold`.
_(Modules 2, 5, 15)_

**Job / Job control**
A job is a pipeline or command group managed by the shell. Job control is the ability to suspend (`Ctrl-Z`), resume (`fg`, `bg`), and monitor running jobs. Each job has a job number and consists of one or more processes in a process group.
_(Module 8)_

**Kernel**
The core component of an operating system that manages hardware resources, process scheduling, memory, and system calls. Shell commands ultimately translate into kernel operations via system calls.
_(Modules 3, 9)_

**Lexer / Lexing / Tokenizer**
The first stage of input processing, which breaks a raw string of characters into a sequence of tokens (meaningful units like words, operators, and delimiters). The lexer handles quoting rules, escape characters, and operator recognition.
_(Module 2)_

**Lifetime (Rust)**
A compile-time annotation (e.g., `'a`) that tells the Rust borrow checker how long a reference is valid. Lifetimes prevent dangling references and use-after-free bugs without runtime overhead.
_(Modules 2, 3, 12)_

**Macro (Rust)**
A metaprogramming feature that generates code at compile time. Declarative macros (`macro_rules!`) match patterns and expand into code. Procedural macros operate on token streams and can derive trait implementations, define attributes, or create function-like macros.
_(Modules 13, 20)_

**Ownership (Rust)**
Rust's core memory management concept: every value has exactly one owner, and the value is dropped (freed) when the owner goes out of scope. Ownership can be transferred (moved) or temporarily lent (borrowed) via references. This system eliminates data races and memory leaks at compile time.
_(All modules)_

**Parent process**
The process that created a given child process. The parent is responsible for waiting on (reaping) its children to collect their exit status. In a shell, the shell process is the parent of every external command it runs.
_(Modules 3, 8)_

**Parser / Parsing**
The stage of input processing that takes a stream of tokens from the lexer and constructs an Abstract Syntax Tree according to the shell's grammar rules. The parser enforces syntax rules and detects errors like unmatched quotes or misplaced operators.
_(Module 2)_

**PATH**
An environment variable containing a colon-separated (Unix) or semicolon-separated (Windows) list of directories. When the user types a command name, the shell searches these directories in order to find a matching executable.
_(Modules 3, 4)_

**Pattern matching (Rust)**
A powerful control-flow mechanism using the `match` keyword that destructures values and selects code branches based on their shape. Pattern matching works with enums, structs, tuples, literals, and ranges, and the compiler ensures all cases are handled.
_(Modules 2, 3, 18)_

**PID (Process ID)**
A unique integer assigned by the operating system to each running process. PIDs are used to send signals, wait on processes, and identify processes in job control. The shell's own PID is available as `$$`.
_(Modules 3, 8, 9)_

**Pipe / Pipeline**
A pipe is a unidirectional IPC channel connecting the stdout of one process to the stdin of another. A pipeline is a sequence of commands connected by pipes (`cmd1 | cmd2 | cmd3`), where data flows left to right through each stage.
_(Module 7)_

**Process**
An instance of a running program, consisting of executable code, a memory address space, file descriptors, environment variables, and OS-managed metadata (PID, state, priority). Each external command the shell runs becomes a separate process.
_(Module 3)_

**Process group**
A collection of related processes identified by a process group ID (PGID), typically equal to the PID of the group leader. The shell uses process groups to manage job control: signals can be sent to an entire group at once.
_(Module 8)_

**Prompt**
The text displayed by the shell to indicate it is ready to accept input. The prompt typically shows information like the current user, hostname, and working directory. Custom prompts can include dynamic content via escape sequences and command substitution.
_(Modules 1, 10)_

**Raw mode**
A terminal input mode in which characters are delivered to the reading process immediately, without line buffering or interpretation of special characters. Raw mode is essential for implementing line editing, tab completion, and other interactive features.
_(Module 10)_

**Redirection**
The mechanism by which a shell reroutes a command's standard input, output, or error streams to or from files, devices, or other file descriptors. Syntax includes `>` (overwrite), `>>` (append), `<` (input), `2>` (stderr), and combinations like `2>&1`.
_(Module 6)_

**REPL (Read-Eval-Print Loop)**
The core loop of an interactive shell: read a line of input, evaluate (parse and execute) it, print any output, and loop back to read again. The REPL is the entry point and main driver of the shell.
_(Module 1)_

**Result / Option (Rust)**
Rust's standard error-handling types. `Result<T, E>` represents either success (`Ok(T)`) or failure (`Err(E)`). `Option<T>` represents either a value (`Some(T)`) or absence (`None`). Both are enums used pervasively instead of exceptions or null pointers.
_(Modules 3, 18)_

**Session (terminal)**
A collection of process groups associated with a controlling terminal. A session is created when a user logs in. Each session has at most one foreground process group and zero or more background process groups.
_(Modules 8, 9)_

**Shebang (#!)**
The character sequence `#!` at the very beginning of a script file, followed by the path to an interpreter (e.g., `#!/bin/bash`). The kernel uses the shebang to determine which program should interpret the script when it is executed directly.
_(Module 11)_

**Shell**
A command-line interpreter that reads user input, parses it into commands, and executes those commands by creating processes or running builtins. Shells also provide programming constructs (variables, loops, conditionals) for scripting.
_(All modules)_

**Signal**
An asynchronous notification sent to a process by the kernel or another process. Common signals include `SIGINT` (Ctrl-C, interrupt), `SIGTSTP` (Ctrl-Z, suspend), `SIGTERM` (terminate), and `SIGCHLD` (child exited). Processes can catch, ignore, or use the default handler for most signals.
_(Module 9)_

**Stdin / Stdout / Stderr**
The three standard I/O streams available to every process. Stdin (fd 0) is the default input source, stdout (fd 1) is the default output destination, and stderr (fd 2) is the default destination for error and diagnostic messages. Shells manipulate these streams via redirection and piping.
_(Modules 6, 7)_

**Struct (Rust)**
A composite data type that groups related values (fields) under a single name. Structs in Rust are used to model shell components like commands, jobs, tokens, and the shell state itself.
_(Modules 1, 2, 3, 8)_

**Subshell**
A child shell process that inherits the parent shell's environment but operates independently. Changes to variables or the working directory in a subshell do not affect the parent. Subshells are created for command substitution, pipelines, and parenthesized command groups.
_(Modules 5, 7)_

**System call (syscall)**
A request from a user-space program to the operating system kernel to perform a privileged operation such as creating a process, reading a file, or sending a signal. All shell operations that interact with the OS ultimately go through system calls.
_(Modules 3, 6, 7, 9)_

**Terminal / TTY / PTY**
A terminal (TTY, from "teletypewriter") is a device or interface for text-based interaction with the operating system. A PTY (pseudo-terminal) is a software emulation of a terminal, used by terminal emulator applications. The terminal handles input echoing, line editing (in cooked mode), and signal generation from keyboard shortcuts.
_(Modules 1, 10)_

**Token**
A discrete unit produced by the lexer, representing a meaningful element of the input such as a word, operator, keyword, or separator. Each token typically carries a type tag and the source text (or value) it represents.
_(Module 2)_

**Trait (Rust)**
A language feature that defines a set of methods that a type must implement, similar to interfaces in other languages. Traits enable polymorphism and are used for abstractions like `Iterator`, `Display`, `Read`, `Write`, and custom shell behaviors.
_(Modules 3, 14, 20)_

**Trie**
A tree-like data structure where each node represents a character, used for efficient prefix-based lookups. In a shell, tries are useful for implementing tab completion and command lookup, as they allow fast matching of partially-typed input against a set of known strings.
_(Module 17)_

**Wait / Waitpid**
System calls that cause the parent process to block until one of its child processes changes state (typically exits). Waiting on children is necessary to collect their exit status and prevent zombie processes. The shell uses wait after launching foreground commands.
_(Modules 3, 8)_

**Word splitting**
The shell's process of breaking expanded text into separate arguments using delimiters (by default, spaces, tabs, and newlines defined in `$IFS`). Word splitting occurs after variable expansion and command substitution but before glob expansion.
_(Module 5)_

**Zombie process**
A process that has finished execution but still has an entry in the process table because its parent has not yet called `wait()` to collect its exit status. Zombies consume no resources beyond their process table entry, but accumulated zombies indicate a bug in the parent program.
_(Modules 3, 8)_
