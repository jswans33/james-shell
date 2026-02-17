use std::io::{self, Write};

fn main() {
    // Set up Ctrl-C handler so it doesn't kill the shell.
    // The closure runs on a separate thread when Ctrl-C is pressed.
    ctrlc::set_handler(|| {
        println!();
        // Flush so the newline actually appears before the next prompt
        let _ = io::stdout().flush();
    })
    .expect("Failed to set Ctrl-C handler");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        // Print the prompt WITHOUT a newline
        print!("jsh> ");
        // Flush is required because print! buffers output.
        // Without this, the user wouldn't see the prompt.
        if stdout.flush().is_err() {
            break;
        }

        // Read one line of input
        let mut input = String::new();
        match stdin.read_line(&mut input) {
            Ok(0) => {
                // EOF: user pressed Ctrl-D (Unix) or Ctrl-Z+Enter (Windows)
                println!("\nGoodbye!");
                break;
            }
            Ok(_) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    // User just pressed Enter — show a fresh prompt
                    continue;
                }
                println!("You typed: {trimmed}");
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                break;
            }
        }
    }
}
