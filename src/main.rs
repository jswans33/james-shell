use james_shell::{executor, expander, jobs::JobTable, parser, redirect};
use std::io::{self, Write};

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
        println!();
        let _ = io::stdout().flush();
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

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut last_exit_code: i32 = 0;
    let mut job_table = JobTable::new();

    loop {
        // Reap any completed background jobs and print "[N] Done cmd" before
        // showing the prompt — this is how bash notifies you that a background
        // job finished.
        job_table.reap();

        print!("jsh> ");
        if stdout.flush().is_err() {
            break;
        }

        let mut input = String::new();
        match stdin.read_line(&mut input) {
            Ok(0) => {
                println!("\nGoodbye!");
                break;
            }
            Ok(_) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Parse into quote-aware words and split pipeline segments.
                let mut words = match parser::parse_words(trimmed) {
                    Ok(words) => words,
                    Err(msg) => {
                        eprintln!("{msg}");
                        last_exit_code = 2;
                        continue;
                    }
                };

                // Detect a trailing `&` background operator and strip it.
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

                let pipeline_words = match parser::split_pipeline(&words) {
                    Ok(words) => words,
                    Err(msg) => {
                        eprintln!("{msg}");
                        last_exit_code = 2;
                        continue;
                    }
                };

                let mut commands = Vec::new();
                let mut had_parse_error = false;

                for segment_words in pipeline_words {
                    let (words, redirections) = match
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

                    let args = expander::expand_words(&words, last_exit_code);
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
                    if commands.is_empty() && !had_parse_error && last_exit_code == 0 {
                        last_exit_code = 2;
                    }
                    continue;
                }

                let action = if commands.len() == 1 {
                    let command = commands.swap_remove(0);
                    executor::execute(
                        &command.command,
                        &command.redirections,
                        background,
                        &mut job_table,
                        &command_text,
                    )
                } else {
                    executor::execute_pipeline(
                        commands,
                        background,
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
                        break;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                break;
            }
        }
    }

    #[cfg(unix)]
    send_sighup_to_jobs(&job_table);

    std::process::exit(last_exit_code);
}
