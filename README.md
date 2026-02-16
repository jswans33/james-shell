# james-shell

A Unix shell built from scratch in Rust, following a 20-module self-paced curriculum that covers everything from a basic REPL loop to structured data pipelines and a plugin system.

## Prerequisites

Before you begin, make sure the following are installed on your machine.

### Required

| Tool | Minimum Version | Purpose |
|------|----------------|---------|
| **Rust** (via [rustup](https://rustup.rs)) | 1.85+ (edition 2024) | Compiler and standard library |
| **Cargo** | Ships with Rust | Build system and package manager |
| **Git** | 2.x | Version control |

### Platform Support

The project targets **Linux**, **macOS**, and **Windows**. Some modules (8-10) use Unix-specific system calls via the `nix` crate, which are gated behind `cfg(unix)` for cross-platform compatibility.

### Recommended Development Tools

These are cargo subcommands that improve the development workflow. Install them after Rust is set up:

```bash
# Linter (usually included with rustup)
rustup component add clippy

# Formatter (usually included with rustup)
rustup component add rustfmt

# File watcher for automatic rebuild on save
cargo install cargo-watch

# Dependency security auditor
cargo install cargo-audit
```

`cargo-fuzz` is used later (Module 13+) and requires nightly Rust:

```bash
cargo install cargo-fuzz
rustup toolchain install nightly
```

## Installation

### Quick Start

```bash
# 1. Install Rust (if you don't have it)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Clone the repository
git clone https://github.com/jswans33/james-shell.git
cd james-shell

# 3. Build the project
cargo build

# 4. Run it
cargo run
```

### Automated Setup

A setup script is included that checks prerequisites, installs missing tools, and verifies the build:

```bash
./setup.sh
```

Run `./setup.sh --check` to verify your environment without installing anything.

### Manual Step-by-Step

If you prefer to set things up yourself:

1. **Install Rust via rustup:**

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

   On Windows, download and run [rustup-init.exe](https://rustup.rs) instead.

2. **Verify the installation:**

   ```bash
   rustc --version    # Should print 1.85.0 or later
   cargo --version    # Should print a matching version
   ```

3. **Clone and build:**

   ```bash
   git clone https://github.com/jswans33/james-shell.git
   cd james-shell
   cargo build
   ```

4. **Run the tests:**

   ```bash
   cargo test
   ```

5. **Run the shell:**

   ```bash
   cargo run
   ```

## Development Workflow

### Common Commands

```bash
cargo build              # Compile the project
cargo run                # Build and run
cargo test               # Run all tests
cargo clippy             # Run the linter
cargo fmt                # Format all source files
cargo fmt -- --check     # Check formatting without changing files
```

### Watch Mode (auto-rebuild on save)

```bash
# Rerun tests on every file change
cargo watch -c -x test

# Rerun clippy on every file change
cargo watch -c -x clippy

# Rebuild and run on every file change
cargo watch -x run
```

## Project Structure

```
james-shell/
├── Cargo.toml          # Project manifest and dependencies
├── Cargo.lock          # Locked dependency versions (committed)
├── src/
│   └── main.rs         # Entry point
├── docs/
│   ├── syllabus.md     # Full 20-module curriculum overview
│   ├── progress.md     # Current progress tracker
│   ├── module-00-foundations.md  # Rust prerequisites
│   ├── module-01-repl-loop.md   # Module 1: The REPL Loop
│   ├── ...                      # Modules 2-20
│   ├── reference-architecture.md  # Architecture and data flow
│   ├── reference-crates.md       # Guide to all external crates
│   ├── reference-glossary.md     # Systems programming glossary
│   ├── reference-cheatsheet.md   # Quick reference
│   └── reference-resources.md    # Books and external resources
└── setup.sh            # Automated setup script
```

## Curriculum Overview

The project is organized into 20 modules across four phases:

| Phase | Modules | What You Build |
|-------|---------|---------------|
| **Phase 1: Walking** | 0-3 | REPL loop, parser, process execution |
| **Phase 2: Running** | 4-7 | Builtins, variables, redirection, pipes |
| **Phase 3: Flying** | 8-10 | Job control, signals, line editing |
| **Phase 4: Scripting** | 11-13 | Control flow, advanced features, testing |
| **Phase 5: Beyond Bash** | 14-20 | Structured data, typed pipelines, plugins |

See [`docs/syllabus.md`](docs/syllabus.md) for the full curriculum and [`docs/progress.md`](docs/progress.md) for current status.

## Troubleshooting

### `rustc` or `cargo` not found after installing Rust

Your shell needs to pick up the cargo bin directory. Run:

```bash
source "$HOME/.cargo/env"
```

Or restart your terminal. If you use a non-default shell config, add this to your profile:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### Build fails with "edition 2024" errors

This project uses Rust edition 2024, which requires Rust 1.85 or later. Update with:

```bash
rustup update stable
```

### `nix` crate fails to compile on Windows

The `nix` crate is Unix-only. On Windows, it is conditionally compiled and should be excluded automatically. If you hit this, make sure you are using the `cfg(unix)` gated dependency entry (see `docs/reference-crates.md` for details).

### `cargo watch` or other tools not found

These are optional cargo subcommands installed separately:

```bash
cargo install cargo-watch
cargo install cargo-audit
```
