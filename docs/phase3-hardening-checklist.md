# Phase 3 Hardening Checklist (Pre-Launch)

## Scope and intent

Phase 1 and 2 modules are marked done in `docs/progress.md`:
- Modules 1–7: `Done`
- Modules 8–10: `Done`

Before moving into Modules 11–13 (`testing + robustness`), we should lock down behavioral invariants so future script/pipeline/editor refactors do not regress non-interactive and persistence paths.

## 1) Regression gates to add before Module 11

### Terminal + I/O paths
- [x] Raw-mode only for interactive input (`stdin` TTY): covered by `src/editor.rs::read_line`.
- [x] Verify stdin-pipe + pseudo-terminal stdout does not invoke `crossterm` event loop.
- [x] Verify non-interactive prompt/output path does not add ANSI/control overhead.
- [x] Verify keyboard-edit features still work in interactive session.

### History persistence
- [x] `.jsh_history` load on startup is validated with an explicit cross-process test.
- [x] `.jsh_history` is append-only for normal command sessions.
- [x] Empty/whitespace commands are never persisted.
- [x] Concurrent temp-home tests avoid cross-test collisions.

### Builtin / pipeline contracts
- [x] Stateful builtins (e.g. `cd`, `export`) are rejected in non-terminal pipeline positions.
- [x] Supported pure builtins run correctly in pipeline contexts without deadlock.
- [x] Warnings are emitted for background builtins and still complete in foreground.
- [x] Exit codes are explicit and stable for builtin-pipeline edge cases.

### CLI and shell semantics
- [x] Exit status of rejected pipelines is non-zero and deterministic.
- [x] `echo`/redirection behavior remains stable for both stdout and stderr routing.
- [x] Script-fed stdin (`printf ... | james-shell`) still executes commands exactly as file input.

## 2) Upstream + downstream protection strategy

- Upstream code changes should include a regression test in the same commit whenever semantics change.
- Downstream safety: run the full `cargo test` set after each hardening cycle.
- Keep `editor_integration.rs` and `phase1_regressions.rs` for shell-behavior guardrails.
- Add module-level notes in `docs` when new invariants are formalized.

## 3) Concrete “hardening now” additions

- [x] Make history tests use unique home directories to prevent cross-test collisions.
- [x] Verify cross-process history readback (`history_persists_across_sessions`) after multiple sessions.
- [ ] Add CI matrix for platform+version coverage once module-13 CI config is introduced.
- [ ] Add a small regression harness for long/unusual input streams and malformed script files.
