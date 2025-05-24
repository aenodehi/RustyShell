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
            
            if arg == "echo" || arg == "exit" || arg == "type" {
                println!("{} is a shell builtin", arg);
                continue;
            }

            if let Ok(path_var) = std::env::var("PATH") {
                let mut found = false;
                let paths = path_var.split(':');
                for dir in paths {
                    let full_path = std::path::Path::new(dir).join(arg);
                    if full_path.exists() && full_path.is_file() {
                        println!("{} is {}", arg, full_path.display());
                        found = true;
                        break;
                    }
                }

                if !found {
                    println!("{}: not found", arg);
                }
                continue;
            }
        }

        // Fallback: unknown command
        println!("{}: command not found", command);
    }
}

