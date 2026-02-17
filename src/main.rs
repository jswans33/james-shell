mod builtins;
mod executor;
mod expander;
mod parser;
mod redirect;

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

                // Parse into quote-aware words, then expand
                let words = parser::parse_words(trimmed);
                let args = expander::expand_words(&words, last_exit_code);

                if args.is_empty() {
                    continue;
                }

                // Separate redirect operators from regular arguments
                let (args, redirections) = match redirect::extract_redirections(&args) {
                    Ok(pair) => pair,
                    Err(msg) => {
                        eprintln!("{msg}");
                        last_exit_code = 2;
                        continue;
                    }
                };

                if args.is_empty() {
                    continue;
                }

                // Build a Command from expanded args and execute with redirections
                let cmd = parser::Command {
                    program: args[0].clone(),
                    args: args[1..].to_vec(),
                };
                last_exit_code = executor::execute(&cmd, &redirections);
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
