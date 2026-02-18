# james-shell — Progress Tracker

## Current Module: 9 — Signal Handling

### Phase 1: Traditional Shell Foundation (Modules 1–7)

| Module | Title | Status | Git Tag |
|--------|-------|--------|---------|
| 1 | The REPL Loop | **Done** | v0.1.0 |
| 2 | Command Parsing & Tokenization | **Done** | v0.2.0 |
| 3 | Process Execution | **Done** | v0.3.0 |
| 4 | Built-in Commands | **Done** | v0.4.0 |
| 5 | Environment Variables & Expansion | **Done** | v0.5.0 |
| 6 | I/O Redirection | **Done** | v0.6.0 |
| 7 | Pipes | **Done** | v0.7.0 |

### Phase 2: Polish & UX (Modules 8–10)

| Module | Title | Status | Git Tag |
|--------|-------|--------|---------|
| 8 | Job Control | **Done** | v0.8.0 |
| 9 | Signal Handling | **Done** | v0.9.0 |
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
