# james-shell — Progress Tracker

## Current Module: 1 — The REPL Loop

### Phase 1: Traditional Shell Foundation (Modules 1–7)

| Module | Title | Status | Git Tag |
|--------|-------|--------|---------|
| 1 | The REPL Loop | **In Progress** | — |
| 2 | Command Parsing & Tokenization | Pending | — |
| 3 | Process Execution | Pending | — |
| 4 | Built-in Commands | Pending | — |
| 5 | Environment Variables & Expansion | Pending | — |
| 6 | I/O Redirection | Pending | — |
| 7 | Pipes | Pending | — |

### Phase 2: Polish & UX (Modules 8–10)

| Module | Title | Status | Git Tag |
|--------|-------|--------|---------|
| 8 | Job Control | Pending | — |
| 9 | Signal Handling | Pending | — |
| 10 | Line Editing & History | Pending | — |

### Phase 3: Scripting & Hardening (Modules 11–13)

| Module | Title | Status | Git Tag |
|--------|-------|--------|---------|
| 11 | Control Flow & Scripting | Pending | — |
| 12 | Advanced Features | Pending | — |
| 13 | Testing & Robustness | Pending | — |

### Phase 4: Beyond Bash (Modules 14–20)

| Module | Title | Status | Git Tag |
|--------|-------|--------|---------|
| 14 | Structured Data Types | Pending | — |
| 15 | Typed Pipelines | Pending | — |
| 16 | Built-in Data Format Handling | Pending | — |
| 17 | Smart Completions & Syntax Highlighting | Pending | — |
| 18 | Modern Error Handling | Pending | — |
| 19 | Modern Scripting Language | Pending | — |
| 20 | Plugin System | Pending | — |

## Notes
- Cross-platform target (Windows + Unix)
- Using `std::process::Command` instead of raw fork/exec
- Using `crossterm` instead of `termion` (cross-platform terminal handling)
- Modules 1–13 build a bash-equivalent shell
- Modules 14–20 go beyond bash with structured data, typed pipelines, and modern scripting
