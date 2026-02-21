use james_shell::{
    ast::Connector,
    editor::{LineEditor, EDITOR_ACTIVE},
    executor, expander,
    jobs::JobTable,
    parser, redirect, script_parser,
};
use std::io::{self, Write};
use std::sync::atomic::Ordering;

/// Send SIGHUP (and SIGCONT so stopped jobs can receive it) to every tracked
/// job's process group when the shell exits.
/// Errors (e.g. ESRCH for already-exited jobs) are silently ignored —
/// this is best-effort cleanup and must not disrupt the shell's exit path.
#[cfg(unix)]
fn send_sighup_to_jobs(job_table: &james_shell::jobs::JobTable) {
    for job in job_table.jobs_sorted() {
        // Skip jobs that have already finished — kill(-pgid, …) would return
        // ESRCH and is harmless, but filtering avoids unnecessary syscalls.
        if matches!(job.status, james_shell::jobs::JobStatus::Done(_)) {
            continue;
        }
        // SAFETY: pgid is valid, signals are standard values, return ignored intentionally.
        unsafe {
            // Ignore return values: ESRCH means the process group is already gone.
            libc::kill(-(job.pgid as libc::pid_t), libc::SIGHUP);
            libc::kill(-(job.pgid as libc::pid_t), libc::SIGCONT);
        }
    }
}

fn main() {
    ctrlc::set_handler(|| {
        // While the line editor is in raw mode, Ctrl-C is delivered as a key
        // event (ISIG is off on Unix) and handled there. Only print the newline
        // when a foreground command is running (editor not active).
        if !EDITOR_ACTIVE.load(Ordering::Relaxed) {
            println!();
            let _ = io::stdout().flush();
        }
    })
    .expect("Failed to set Ctrl-C handler");

    #[cfg(unix)]
    // SAFETY: called once, single-threaded, before spawning any children.
    unsafe {
        // Shell must survive Ctrl-Z, Ctrl-\, and broken pipes at the prompt.
        // SIGINT is already handled by the ctrlc crate above (prints newline, EINTR).
        //
        // IMPORTANT: SIG_IGN is inherited by forked children AND survives exec().
        // That means every child spawned after this point would also ignore these
        // signals — which is wrong. The pre_exec block in executor.rs explicitly
        // resets them back to SIG_DFL before exec() to restore correct child behavior.
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let mut last_exit_code: i32 = 0;
    let mut job_table = JobTable::new();
    let mut editor = LineEditor::new();

    loop {
        // Reap any completed background jobs and print "[N] Done cmd" before
        // showing the prompt — this is how bash notifies you that a background
        // job finished.
        job_table.reap();

        let input = match editor.read_line("jsh> ") {
            Ok(Some(line)) => line,
            Ok(None) => {
                // Only print the goodbye message for interactive sessions.
                // Child shells spawned for whole-chain background execution read
                // from a pipe, not a TTY, and must not print to the terminal.
                use std::io::IsTerminal;
                if std::io::stdin().is_terminal() {
                    println!("Goodbye!");
                }
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(e) => {
                eprintln!("Error reading input: {e}");
                break;
            }
        };

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Add to history before parsing so even malformed commands are recorded,
        // consistent with bash behaviour.
        editor.add_to_history(trimmed);

        // Parse into quote-aware words.
        let mut words = match parser::parse_words(trimmed) {
            Ok(words) => words,
            Err(msg) => {
                eprintln!("{msg}");
                last_exit_code = 2;
                continue;
            }
        };

        // Detect a trailing `&` background operator and strip it.
        // When present, the last pipeline in the chain runs in the background.
        // The command text (for display in `jobs`) is the line without `&`.
        let background = words
            .last()
            .map(|w| parser::is_background_word(w))
            .unwrap_or(false);
        if background {
            words.pop();
        }
        let command_text = trimmed
            .trim_end_matches(|c: char| c == '&' || c == ' ')
            .to_string();

        // Split by chain operators (&&, ||, ;) into ordered entries.
        let chain = match script_parser::parse_chain(words) {
            Ok(chain) => chain,
            Err(msg) => {
                eprintln!("{msg}");
                last_exit_code = 2;
                continue;
            }
        };

        if chain.is_empty() {
            continue;
        }

        // Phase 1 — Pre-validate pipeline structure for every chain entry up-front.
        //
        // This separates STATIC structural validation (pipeline token layout: does
        // every `|` have a command on each side?) from DYNAMIC evaluation (word
        // expansion and redirect resolution, which depend on runtime state like $?).
        //
        // Validating all entries now means a syntax error in a branch that would be
        // short-circuited by && / || is still reported, rather than silently ignored
        // because the branch happened not to run.
        let mut pre_validated: Vec<(Vec<Vec<parser::Word>>, Connector)> = Vec::new();
        let mut syntax_ok = true;

        for entry in &chain {
            match parser::split_pipeline(&entry.words) {
                Ok(pipeline_words) => {
                    pre_validated.push((pipeline_words, entry.connector.clone()));
                }
                Err(msg) => {
                    eprintln!("{msg}");
                    last_exit_code = 2;
                    syntax_ok = false;
                    break;
                }
            }
        }

        if !syntax_ok {
            continue;
        }

        // Phase 2 — Whole-chain background.
        //
        // When the line ends with `&` and the chain has more than one entry, the
        // entire chain must run as a single background job so that && / || exit-code
        // semantics work correctly inside it.  A simple per-entry background flag
        // cannot achieve this: backgrounding an early entry returns immediately with
        // an unknown exit code, so && / || gates become meaningless.
        //
        // The solution: spawn a child james-shell process and feed it the command
        // text on stdin.  The child executes the full chain in its foreground while
        // the parent shell registers it as a background job and returns the prompt.
        // (Single-entry chains continue to use the per-command background path below.)
        if background && pre_validated.len() > 1 {
            let exe = std::env::current_exe()
                .unwrap_or_else(|_| std::path::PathBuf::from("james-shell"));
            match std::process::Command::new(&exe)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .spawn()
            {
                Ok(mut child) => {
                    // Write the command text and signal EOF so the child shell
                    // executes the chain and exits cleanly.
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = writeln!(stdin, "{command_text}");
                        // stdin drops here, closing the pipe and triggering EOF
                    }
                    let (job_id, pid) = job_table.add(child, command_text.clone());
                    println!("[{job_id}] {pid}");
                    last_exit_code = 0;
                }
                Err(e) => {
                    eprintln!("jsh: failed to spawn background shell: {e}");
                    last_exit_code = 1;
                }
            }
            continue; // prompt is ready; the chain runs in the child
        }

        // Phase 3 — Execute chain entries with short-circuit logic.
        //
        // We iterate the pre-validated pipeline segments (so split_pipeline is not
        // called a second time).  Word expansion and redirect resolution happen here
        // because they depend on the runtime value of $? after each entry runs.
        let mut should_exit = false;

        for (i, (pipeline_words, connector)) in pre_validated.into_iter().enumerate() {
            // Decide whether this entry should run based on the connector and
            // the exit code left by the previous entry.
            let should_run = match connector {
                Connector::Sequence => true,
                Connector::And => last_exit_code == 0,
                Connector::Or => last_exit_code != 0,
            };
            if !should_run {
                continue;
            }

            // For single-entry chains, the background flag is passed through to the
            // executor as normal.  Multi-entry chains with & are handled in Phase 2
            // above, so this path is only reached when entry_count == 1.
            let entry_background = background && (i == 0);

            let mut commands = Vec::new();
            let mut had_parse_error = false;

            for segment_words in pipeline_words {
                let (seg_words, redirections) = match
                    redirect::extract_redirections_from_words(&segment_words, last_exit_code)
                {
                    Ok(pair) => pair,
                    Err(msg) => {
                        eprintln!("{msg}");
                        last_exit_code = 2;
                        had_parse_error = true;
                        break;
                    }
                };

                let args = expander::expand_words(&seg_words, last_exit_code);
                if args.is_empty() {
                    eprintln!("jsh: syntax error: empty command");
                    last_exit_code = 2;
                    had_parse_error = true;
                    break;
                }

                let command = parser::Command {
                    program: args[0].clone(),
                    args: args[1..].to_vec(),
                };
                commands.push(executor::PipelineCommand { command, redirections });
            }

            if had_parse_error || commands.is_empty() {
                if commands.is_empty() && !had_parse_error {
                    last_exit_code = 2;
                }
                break;
            }

            let action = if commands.len() == 1 {
                let command = commands.swap_remove(0);
                executor::execute(
                    &command.command,
                    &command.redirections,
                    entry_background,
                    &mut job_table,
                    &command_text,
                )
            } else {
                executor::execute_pipeline(
                    commands,
                    entry_background,
                    &mut job_table,
                    &command_text,
                )
            };

            match action {
                executor::ExecutionAction::Continue(code) => {
                    last_exit_code = code;
                }
                executor::ExecutionAction::Exit(code) => {
                    last_exit_code = code;
                    should_exit = true;
                    break;
                }
            }
        }

        if should_exit {
            break;
        }
    }

    #[cfg(unix)]
    send_sighup_to_jobs(&job_table);

    std::process::exit(last_exit_code);
}
