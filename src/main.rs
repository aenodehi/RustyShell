use std::io::{self, Write};
use std::process;

fn main() {
    loop {
        // Print prompt
        print!("$ ");
        io::stdout().flush().unwrap();

        // Read user input
        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input).unwrap();
        if bytes_read == 0 {
            break; // Exit on EOF (Ctrl+D)
        }

        let command = input.trim();

        // Handle "exit 0"
        if command == "exit 0" {
            process::exit(0);
        }

        // Handle "echo ..."
        if command.starts_with("echo ") {
            let echo_output = &command[5..]; // Everything after "echo "
            println!("{}", echo_output);
            continue;
        }

        // Handle "type <builtin>"
        if command.starts_with("type ") {
            let arg = &command[5..];
            match arg {
                "echo" | "exit" | "type" => {
                    println!("{} is a shell builtin", arg);
                }
                _ => {
                    println!("type {}: command not found", arg);
                }
            }
            continue;
        }

        // Fallback: unknown command
        println!("{}: command not found", command);
    }
}

