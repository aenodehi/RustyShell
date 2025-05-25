use std::io::{self, Write};
use std::process::{self, Command, Stdio};
use std::path::Path;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::fs::File;


fn main() {
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).unwrap() == 0 {
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

        if trimmed == "exit 0" {
            process::exit(0);
        }

        if command == "echo" {
            let mut cleaned_args = Vec::new();
            let mut stdout_redirect: Option<File> = None;
            let mut stderr_redirect: Option<File> = None;

            let mut i = 0;
            while i < args.len() {
                if args[i] == ">" || args[i] == "1>" {
                    if i + 1 < args.len() {
                        match File::create(&args[i + 1]) {
                            Ok(file) => stdout_redirect = Some(file),
                            Err(e) => {
                                eprintln!("{}: {}", args[i + 1], e);
                                break;
                            }
                        }
                        i += 2;
                        continue;
                    } else {
                        eprintln!("{}: missing filename", args[i]);
                        break;
                    }
                } else if args[i] == "2>" {
                    if i + 1 < args.len() {
                        match File::create(&args[i + 1]) {
                            Ok(file) => stderr_redirect = Some(file),
                            Err(e) => {
                                eprintln!("{}: {}", args[i + 1], e);
                                break;
                            }
                        }
                        i += 2;
                        continue;
                    } else {
                        eprintln!("2>: missing filename");
                        break;
                    }
                }

                cleaned_args.push(args[i].clone());
                i += 1;
            }

            let output = cleaned_args.join(" ");
            if let Some(mut file) = stdout_redirect {
                if let Err(e) = writeln!(file, "{}", output) {
                    eprintln!("echo: failed to write to file: {}", e);
                }
            } else {
                println!("{}", output);
            }
            continue;
        }

        if command == "type" {
            if let Some(arg) = args.first() {
                if ["echo", "exit", "type", "pwd", "cd"].contains(&arg.as_str()) {
                    println!("{} is a shell builtin", arg);
                    continue;
                }

                if let Ok(path_var) = std::env::var("PATH") {
                    let found = path_var.split(':').any(|dir| {
                        let full_path = Path::new(dir).join(arg);
                        if full_path.exists() && full_path.is_file() {
                            println!("{} is {}", arg, full_path.display());
                            true
                        } else {
                            false
                        }
                    });

                    if !found {
                        println!("{}: not found", arg);
                    }
                }
            } else {
                println!("type: missing argument");
            }
            continue;
        }

        if command == "pwd" {
            match std::env::current_dir() {
                Ok(path) => println!("{}", path.display()),
                Err(err) => eprintln!("pwd: {}", err),
            }
            continue;
        }

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

        // External command
        if let Ok(path_var) = std::env::var("PATH") {
            let (mut args_vec, stderr_redirect) =
                parse_command_with_stderr_redirection(parts.clone());
            let mut stdout_redirect: Option<File> = None;

            // Clone early to avoid borrowing conflict
            let command = if !args_vec.is_empty() {
                args_vec[0].clone()
            } else {
                continue;
            };

            let mut i = 0;
            while i < args_vec.len() {
                if args_vec[i] == ">" || args_vec[i] == "1>" {
                    if i + 1 < args_vec.len() {
                        let filename = args_vec[i + 1].clone();
                        match File::create(&filename) {
                            Ok(file) => {
                                stdout_redirect = Some(file);
                                args_vec.drain(i..=i + 1);
                                continue;
                            }
                            Err(e) => {
                                eprintln!("{}: {}", filename, e);
                                break;
                            }
                        }
                    } else {
                        eprintln!("{}: missing filename", args_vec[i]);
                        break;
                    }
                }
                i += 1;
            }

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
                    found = true;

                    let mut cmd = Command::new(full_path);
                    cmd.args(&args_vec[1..]);

                    if let Some(ref file) = stdout_redirect {
                        cmd.stdout(Stdio::from(file.try_clone().unwrap()));
                    } else {
                        cmd.stdout(Stdio::piped());
                    }

                    if let Some(ref stderr_file) = stderr_redirect {
                        match File::create(stderr_file) {
                            Ok(file) => {
                                cmd.stderr(Stdio::from(file));
                            }
                            Err(e) => {
                                eprintln!("{}: {}", stderr_file, e);
                                continue;
                            }
                        }
                    } else {
                        cmd.stderr(Stdio::piped());
                    }

                    match cmd.spawn().and_then(|child| child.wait_with_output()) {
                        Ok(output) => {
                            if stdout_redirect.is_none() {
                                print!("{}", String::from_utf8_lossy(&output.stdout));
                            }
                            if stderr_redirect.is_none() {
                                eprint!("{}", String::from_utf8_lossy(&output.stderr));
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to execute {}: {}", command, e);
                        }
                    }
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
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes => in_double_quotes = !in_double_quotes,
            '\\' => {
                if in_single_quotes {
                    current.push('\\');
                } else if let Some(next_ch) = chars.next() {
                    if in_double_quotes && ['\\', '"', '$', '\n'].contains(&next_ch) {
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
            ' ' | '\t' if !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                while let Some(&next) = chars.peek() {
                    if next == ' ' || next == '\t' {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn parse_command_with_stderr_redirection(parts: Vec<String>) -> (Vec<String>, Option<String>) {
    let mut cmd_parts = Vec::new();
    let mut stderr_file = None;

    let mut iter = parts.into_iter().peekable();
    while let Some(part) = iter.next() {
        if part == "2>" {
            if let Some(file) = iter.next() {
                stderr_file = Some(file);
            }
        } else {
            cmd_parts.push(part);
        }
    }

    (cmd_parts, stderr_file)
}

