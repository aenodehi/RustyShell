use std::io::Write;
use std::env;
use std::fs;
use std::process::{self, Command, Stdio};
use std::path::Path;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::fs::{File, OpenOptions};

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::{Editor, Helper, Context, Config, CompletionType};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{Validator, ValidationResult, ValidationContext};
use rustyline::history::FileHistory;

use nix::sys::wait::waitpid;
use nix::unistd::{close, dup2, execvp, fork, pipe, ForkResult};
use std::ffi::CString;
use std::os::unix::io::RawFd;
use std::os::fd::IntoRawFd;
use libc;

struct ShellCompleter;

impl Hinter for ShellCompleter {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Highlighter for ShellCompleter {}
impl Validator for ShellCompleter {
    fn validate(&self, _ctx: &mut ValidationContext<'_>) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}
impl Helper for ShellCompleter {}

impl Completer for ShellCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let prefix = &line[..pos];
        let mut completions = vec![];


        let builtins = ["exit", "echo", "cd", "pwd", "type"];
        for &builtin in &builtins {
            if builtin.starts_with(prefix) {
                completions.push(Pair {
                    display: builtin.to_string(),
                    replacement: format!("{} ", builtin),
                });
            }
        }

        if let Ok(paths) = env::var("PATH") {
            for dir in paths.split(':') {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        if let Ok(file_type) = entry.file_type() {
                            if file_type.is_file() {
                                if let Some(name) = entry.file_name().to_str() {
                                    if name.starts_with(prefix) {
                                        completions.push(Pair {
                                            display: name.to_string(),
                                            replacement: format!("{} ", name),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort by display text for consistent output
        completions.sort_by(|a, b| a.display.cmp(&b.display));

        // Calculate the start of the word to replace
        let start = line[..pos]
            .rfind(|c: char| c == ' ' || c == '\t')
            .map_or(0, |i| i + 1);

        Ok((start, completions))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();

    let mut rl = Editor::<_, FileHistory>::with_config(config).unwrap();    
    let completer = ShellCompleter;
    rl.set_helper(Some(completer));

  
    loop {
        let readline = rl.readline("$ ");
        let input = match readline {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line.as_str());
                line
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break Ok(()),
            Err(_) => continue,
        };

        //let _ = rl.add_history_entry(line.as_str());

        let trimmed = input.trim();
        let parts = tokenize(trimmed);
        if parts.is_empty() {
            continue;
        }

        let command = &parts[0];
        let args = &parts[1..];

        if trimmed == "exit 0" {
            process::exit(0);
        }

        if trimmed.contains('|') {
            handle_pipeline(trimmed);
            continue;
        }

        if command == "echo" {
            let mut cleaned_args = Vec::new();
            let mut stdout_redirect: Option<File> = None;
            let mut stderr_redirect: Option<File> = None;
            let mut args = parts[1..].to_vec();

            let mut i = 0;
            while i < args.len() {
                if args[i] == ">" || args[i] == "1>" || args[i] == ">>" || args[i] == "1>>" {
                    if i + 1 < args.len() {
                        let filename = &args[i + 1];
                        if let Some(parent) = Path::new(filename).parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let file = if args[i] == ">" || args[i] == "1>" {
                            File::create(filename)
                        } else {
                            OpenOptions::new().append(true).create(true).open(filename)
                        };
                        match file {
                            Ok(file) => {
                                stdout_redirect = Some(file);
                                args.drain(i..=i + 1);
                                continue;
                            }
                            Err(e) => {
                                eprintln!("{}: {}", filename, e);
                                break;
                            }
                        }
                    }
                } else if args[i] == "2>" || args[i] == "2>>" {
                    if i + 1 < args.len() {
                        let filename = &args[i + 1];
                        if let Some(parent) = Path::new(filename).parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let file = if args[i] == "2>" {
                            File::create(filename)
                        } else {
                            OpenOptions::new().append(true).create(true).open(filename)
                        };
                        match file {
                            Ok(file) => {
                                stderr_redirect = Some(file);
                                args.drain(i..=i + 1);
                                continue;
                            }
                            Err(e) => {
                                eprintln!("{}: {}", filename, e);
                                break;
                            }
                        }
                    }
                }

                cleaned_args.push(args[i].clone());
                i += 1;
            }

            let output = cleaned_args.join(" ");
            if let Some(mut file) = stdout_redirect {
                if let Err(e) = writeln!(file, "{}", output) {
                    if let Some(mut stderr_file) = stderr_redirect {
                        let _ = writeln!(stderr_file, "echo: failed to write to file: {}", e);
                    } else {
                        eprintln!("echo: failed to write to file: {}", e);
                    }
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

                let mut found = false;
                if let Ok(path_var) = std::env::var("PATH") {
                    found = path_var.split(':').any(|dir| {
                        let full_path = Path::new(dir).join(arg);
                        check_executable(&full_path, arg)
                    });
                }

                if !found {
                    let system_dirs = ["/usr/bin", "/bin", "/usr/local/bin"];
                    found = system_dirs.iter().any(|dir| {
                        let full_path = Path::new(dir).join(arg);
                        check_executable(&full_path, arg)
                    });
                }

                if !found {
                    println!("{}: not found", arg);
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

        // External command execution
        if let Ok(path_var) = std::env::var("PATH") {
            let mut args_vec = parts.clone();
            let (stderr_path_opt, stderr_append) = parse_stderr_redirection(&mut args_vec);
            let mut stderr_file: Option<File> = None;

            if let Some(ref stderr_path) = stderr_path_opt {
                if let Some(parent) = Path::new(stderr_path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let result = if stderr_append {
                    OpenOptions::new().append(true).create(true).open(stderr_path)
                } else {
                    File::create(stderr_path)
                };
                match result {
                    Ok(file) => stderr_file = Some(file),
                    Err(e) => {
                        eprintln!("{}: {}", stderr_path, e);
                    }
                }
            }

            let mut stdout_redirect: Option<File> = None;

            let command = if !args_vec.is_empty() {
                args_vec[0].clone()
            } else {
                continue;
            };

            let mut i = 0;
            while i < args_vec.len() {
                if args_vec[i] == ">" || args_vec[i] == "1>" || args_vec[i] == ">>" || args_vec[i] == "1>>" {
                    if i + 1 < args_vec.len() {
                        let filename = args_vec[i + 1].clone();
                        let result = if args_vec[i].ends_with(">>") {
                            OpenOptions::new().append(true).create(true).open(&filename)
                        } else {
                            File::create(&filename)
                        };
                        match result {
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
                    }
                }
                i += 1;
            }

            let mut found = false;
            for dir in path_var.split(':') {
                let full_path = Path::new(dir).join(&command);
                if full_path.exists() && full_path.is_file()
                    && full_path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false) {
                    found = true;

                    let mut cmd = Command::new(full_path);
                    cmd.arg0(&command);
                    cmd.args(&args_vec[1..]);

                    if let Some(ref file) = stdout_redirect {
                        cmd.stdout(Stdio::from(file.try_clone().unwrap()));
                    } else {
                        cmd.stdout(Stdio::piped());
                    }

                    if let Some(file) = &stderr_file {
                        cmd.stderr(Stdio::from(file.try_clone().unwrap()));
                    } else {
                        cmd.stderr(Stdio::piped());
                    }

                    match cmd.spawn().and_then(|child| child.wait_with_output()) {
                        Ok(output) => {
                            if stdout_redirect.is_none() {
                                print!("{}", String::from_utf8_lossy(&output.stdout));
                            }
                            if stderr_file.is_none() {
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
                    } else if !in_double_quotes && !in_single_quotes {
                        current.push(next_ch);
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

fn parse_stderr_redirection(args: &mut Vec<String>) -> (Option<String>, bool) {
    let mut stderr_file = None;
    let mut append = false;
    let mut i = 0;

    while i < args.len() {
        if args[i] == "2>" || args[i] == "2>>" {
            if i + 1 < args.len() {
                stderr_file = Some(args[i + 1].clone());
                append = args[i] == "2>>";
                args.drain(i..=i + 1);
                continue;
            }
        }
        i += 1;
    }

    (stderr_file, append)
}

fn check_executable(path: &Path, arg: &str) -> bool {
    if path.exists()
        && path.is_file()
        && path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    {
        println!("{} is {}", arg, path.display());
        true
    } else {
        false
    }
}



fn handle_pipeline(cmd: &str) {
    let parts: Vec<&str> = cmd.split('|').map(str::trim).collect();
    if parts.len() != 2 {
        eprintln!("Only single pipelines supported");
        return;
    }

    let left_cmd: Vec<CString> = parts[0]
        .split_whitespace()
        .map(|s| CString::new(s).unwrap())
        .collect();
    let right_cmd: Vec<CString> = parts[1]
        .split_whitespace()
        .map(|s| CString::new(s).unwrap())
        .collect();

    let (read_end, write_end): (RawFd, RawFd) = {
        let (r, w) = pipe().expect("pipe failed");
        (r.into_raw_fd(), w.into_raw_fd())
    };

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // First child: execute left side of pipe
            unsafe {
                libc::dup2(write_end, libc::STDOUT_FILENO);
            }
            close(read_end).ok();
            close(write_end).ok();
            execvp(&left_cmd[0], &left_cmd).expect("execvp failed for left command");
        }
        Ok(ForkResult::Parent { .. }) => {
            match unsafe { fork() } {
                Ok(ForkResult::Child) => {
                    // Second child: execute right side of pipe
                    unsafe {
                        libc::dup2(read_end, libc::STDIN_FILENO);
                    }
                    close(read_end).ok();
                    close(write_end).ok();
                    execvp(&right_cmd[0], &right_cmd).expect("execvp failed for right command");
                }
                Ok(ForkResult::Parent { .. }) => {
                    // Parent process
                    close(read_end).ok();
                    close(write_end).ok();
                    waitpid(None, None).ok();
                    waitpid(None, None).ok();
                }
                Err(_) => {
                    eprintln!("Failed to fork second child");
                }
            }
        }
        Err(_) => {
            eprintln!("Failed to fork first child");
        }
    }
}

