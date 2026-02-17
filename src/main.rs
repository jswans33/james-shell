use james_shell::{executor, expander, parser, redirect};
use std::io::{self, Write};

fn main() {
    ctrlc::set_handler(|| {
        println!();
        let _ = io::stdout().flush();
    })
    .expect("Failed to set Ctrl-C handler");

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut last_exit_code: i32 = 0;

    loop {
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
                let words = match parser::parse_words(trimmed) {
                    Ok(words) => words,
                    Err(msg) => {
                        eprintln!("{msg}");
                        last_exit_code = 2;
                        continue;
                    }
                };
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

                    // Build a Command from expanded args and execute with redirections.
                    let command = parser::Command {
                        program: args[0].clone(),
                        args: args[1..].to_vec(),
                    };
                    commands.push(executor::PipelineCommand { command, redirections });
                }

                if had_parse_error || commands.is_empty() {
                    if commands.is_empty() && !had_parse_error && last_exit_code == 0 {
                        // Should only occur for malformed input we couldn't parse as a segment.
                        last_exit_code = 2;
                    }
                    continue;
                }

                let action = if commands.len() == 1 {
                    let command = commands.swap_remove(0);
                    executor::execute(&command.command, &command.redirections)
                } else {
                    executor::execute_pipeline(commands)
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

    std::process::exit(last_exit_code);
}
