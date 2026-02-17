mod parser;

use std::io::{self, Write};

fn main() {
    // Set up Ctrl-C handler so it doesn't kill the shell.
    ctrlc::set_handler(|| {
        println!();
        let _ = io::stdout().flush();
    })
    .expect("Failed to set Ctrl-C handler");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

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

                // Parse the input into a structured Command
                match parser::parse(trimmed) {
                    Some(cmd) => println!("{cmd:?}"),
                    None => continue,
                }
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                break;
            }
        }
    }
}
