use std::io::{self, Write};
use std::process::{self, Command};
use std::path::Path;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;

fn main() {
    loop {
        // Prompt
        print!("$ ");
        io::stdout().flush().unwrap();

        // Input
        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input).unwrap();
        if bytes_read == 0 {
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let command = parts[0];
        let args = &parts[1..];

        // Builtin: exit
        if trimmed == "exit 0" {
            process::exit(0);
        }

        // Builtin: echo
        if command == "echo" {
            println!("{}", args.join(" "));
            continue;
        }

        // Builtin: type 
        if command == "type" {
            if let Some(arg) = args.first() {
                if *arg == "echo" || *arg == "exit" || *arg == "type" {
                    println!("{} is a shell builtin", arg);
                    continue;
                }

                if let Ok(path_var) = std::env::var("PATH") {
                    let mut found = false;
                    for dir in path_var.split(':') {
                        let full_path = Path::new(dir).join(arg);
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
            } else {
                println!("type: missing argument");
                continue;
            }
        }

        // Try to run external program
        if let Ok(path_var) = std::env::var("PATH") {
            let mut found = false;
            for dir in path_var.split(':') {
                let full_path = Path::new(dir).join(command);
                if full_path.exists() 
                    && full_path.is_file() 
                    && full_path
                    .metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false) 
                {
                    let result = Command::new(full_path)
                        .arg0(command_name)
                        .args(args)
                        .spawn()
                        .and_then(|mut child| child.wait_with_output());

                    match result {
                        Ok(output) => {
                            print!("{}", String::from_utf8_lossy(&output.stdout));
                            eprint!("{}", String::from_utf8_lossy(&output.stderr));
                        }
                        Err(e) => {
                            eprintln!("Failed to execute {}: {}", command, e);
                        }
                    }

                    found = true;
                    break;
                }
            }

            if !found {
                println!("{}: command not found", command);
            }
        }
    }
}

