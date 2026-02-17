mod executor;
mod parser;

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

                if let Some(cmd) = parser::parse(trimmed) {
                    last_exit_code = executor::execute(&cmd);
                }
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                break;
            }
        }
    }

    std::process::exit(last_exit_code);
}
