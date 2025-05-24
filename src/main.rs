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

        let parts: Vec<String> = tokenize(trimmed);
        if parts.is_empty() {
            continue;
        }
        let command = &parts[0];
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
                if *arg == "echo" || *arg == "exit" || *arg == "type" || *arg == "pwd" {
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

        // Builtin: pwd
        if command == "pwd" {
            match std::env::current_dir() {
                Ok(path) => println!("{}", path.display()),
                Err(err) => eprintln!("pwd: {}", err),
            }
            continue;
        }

        // Builtin: cd
//        if command == "cd" {
//            if let Some(target_dir) = args.first() {
//                let path = Path::new(target_dir);
//                if path.is_absolute() {
//                    if let Err(_) = std::env::set_current_dir(path) {
//                        eprintln!("cd: {}: No such file or directory", target_dir);
//                    }
//                } else {
//                    eprintln!("cd: {}: Relative paths not supported yet", target_dir);
//                }
//            } else {
//                eprintln!("cd: missing operand")
//            }
//            continue;
//        }

        // Builtin: cd
        if command == "cd" {
            if let Some(path) = args.first() {
                let target_dir = if *path == "~" {
                    match std::env::var("HOME") {
                        Ok(home) => home,
                        Err(_) => {
                            eprintln!("cd: HOME not set");
                            continue;
                        }
                    }
                } else {
                    path.to_string()
                };

                if let Err(_) = std::env::set_current_dir(&target_dir) {
                    eprintln!("cd: {}: No such file or directory", target_dir);
                }
            }
            continue;
        }
        // Try to run external program
        if let Ok(path_var) = std::env::var("PATH") {
            let mut found = false;
            for dir in path_var.split(':') {
                let full_path = Path::new(dir).join(&command);
                if full_path.exists() 
                    && full_path.is_file() 
                    && full_path
                    .metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false) 
                {
                    let result = Command::new(full_path)
                        .arg0(&command)
                        .args(args.iter().map(|s| s.as_str()))
                        .spawn()
                        .and_then(|child| child.wait_with_output());

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

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            // Toggle single quote context
            '\'' if !in_double_quotes => {
                in_single_quotes = !in_single_quotes;
            }

            // Toggle double quote context
            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
            }

            // Handle backslash
            '\\' => {
                if in_single_quotes {
                    current.push('\\');
                } else if let Some(&next_ch) = chars.peek() {
                    let next_ch = chars.next().unwrap();
                    if in_double_quotes && (next_ch == '"' || next_ch == '\\' || next_ch == '$' || next_ch == '\n') {
                        if next_ch != '\n' {
                            current.push(next_ch);
                        }
                    } else {
                        current.push('\\');
                        current.push(next_ch);
                    }
                } else {
                    current.push('\\');
                }
            }

            // Whitespace (token boundary) outside any quotes
            ' ' | '\t' if !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                // Skip extra whitespace
                while let Some(&next) = chars.peek() {
                    if next == ' ' || next == '\t' {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }

            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

