use std::io::Write;
//use std::io::{self, Write};
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
use nix::unistd::{close, execvp, fork, pipe, ForkResult};
//use nix::unistd::dup2;
use std::ffi::CString;
//use std::os::unix::io::RawFd;
use std::os::fd::IntoRawFd;
use libc;
use std::os::unix::io::AsRawFd;

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

    let mut history: Vec<String> = Vec::new();

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


        history.push(trimmed.to_string());

        if command == "history" {
            match args.len() {
                0 => {

                    for (i, cmd) in history.iter().enumerate() {
                        println!("{}  {}", i + 1, cmd);
                    }
                }
                1 => {
                    if let Ok(n) = args[0].parse::<usize>() {
                        let total = history.len();

                        let start = total.saturating_sub(n);
                        for (i, cmd) in history.iter().enumerate().skip(start) {
                            println!("{:>4}  {}", i + 1, cmd);
                        }
                    } else {
                        eprintln!("history: {}: numeric argument required", args[0]);
                    }
                }
                _ => {
                    eprintln!("history: too many arguments");
                }
            }
            continue;
        }

        if command == "type" && args.get(0).map(|s| s.as_str()) == Some("history") {
            println!("history is a shell builtin");
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

fn is_builtin(cmd: &str) -> bool {
    matches!(cmd, "cd" | "pwd" | "echo" | "exit" | "type")
}

fn run_builtin(
    cmd: &str,
    args: &[&str],
    stdout_redirect: Option<&mut File>,
    stderr_redirect: Option<&mut File>,
) {
    match cmd {
        "echo" => {
            let output = args.join(" ");
            if let Some(out) = stdout_redirect {
                let _ = writeln!(out, "{}", output);
            } else {
                println!("{}", output);
            }
        }
        "pwd" => {
            match env::current_dir() {
                Ok(path) => {
                    if let Some(out) = stdout_redirect {
                        let _ = writeln!(out, "{}", path.display());
                    } else {
                        println!("{}", path.display());
                    }
                }
                Err(err) => {
                    if let Some(err_file) = stderr_redirect {
                        let _ = writeln!(err_file, "pwd: {}", err);
                    } else {
                        eprintln!("pwd: {}", err);
                    }
                }
            }
        }
        "cd" => {
            let target = args.get(0).map(|s| *s).unwrap_or("");
            let target_dir = if target == "~" {
                env::var("HOME").unwrap_or_else(|_| ".".into())
            } else {
                target.to_string()
            };
            if let Err(err) = env::set_current_dir(&target_dir) {
                if let Some(err_file) = stderr_redirect {
                    let _ = writeln!(err_file, "cd: {}: {}", target_dir, err);
                } else {
                    eprintln!("cd: {}: {}", target_dir, err);
                }
            }
        }
        "exit" => {
            process::exit(0);
        }
        "type" => {
            if let Some(arg) = args.first() {
                if is_builtin(arg) {
                    println!("{} is a shell builtin", arg);
                } else {
                    let mut found = false;
                    if let Ok(path_var) = env::var("PATH") {
                        found = path_var.split(':').any(|dir| {
                            let full_path = Path::new(dir).join(arg);
                            full_path.exists() && full_path.is_file()
                                && full_path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
                        });
                    }

                    if found {
                        println!("{} is a binary in PATH", arg);
                    } else {
                        println!("{}: not found", arg);
                    }
                }
            } else {
                println!("type: missing argument");
            }
        }
        _ => {
            if let Some(err_file) = stderr_redirect {
                let _ = writeln!(err_file, "{}: builtin not implemented", cmd);
            } else {
                eprintln!("{}: builtin not implemented", cmd);
            }
        }
    }
}

fn handle_pipeline(cmd_line: &str) {
    let stages: Vec<&str> = cmd_line.split('|').map(str::trim).collect();
    let num_cmds = stages.len();

    if num_cmds < 2 {
        eprintln!("Pipeline must contain at least two commands.");
        return;
    }

    // Prepare N–1 pipes
    let mut pipes = Vec::with_capacity(num_cmds - 1);
    for _ in 0..(num_cmds - 1) {
        match pipe() {
            Ok((r, w)) => pipes.push((r, w)),
            Err(err) => {
                eprintln!("pipe failed: {}", err);
                return;
            }
        }
    }

    // Fork each stage
    for i in 0..num_cmds {
        let stage = stages[i];
        let parts: Vec<String> = tokenize(stage);
        let cmd_name = parts[0].as_str();
        let args: Vec<&str> = parts.iter().skip(1).map(|s| s.as_str()).collect();

        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                // If not first, read from previous pipe
                if i > 0 {
                    let (prev_read, _) = &pipes[i - 1];
                    unsafe { libc::dup2(prev_read.as_raw_fd(), libc::STDIN_FILENO) };
                }
                // If not last, write to next pipe
                if i < num_cmds - 1 {
                    let (_, next_write) = &pipes[i];
                    unsafe { libc::dup2(next_write.as_raw_fd(), libc::STDOUT_FILENO) };
                }

                // **Suppress Broken pipe** on intermediate stages
                if i < num_cmds - 1 {
                    if let Ok(devnull) = OpenOptions::new().read(true).open("/dev/null") {
                        unsafe {
                            libc::dup2(devnull.as_raw_fd(), libc::STDERR_FILENO);
                        }
                    }
                }

                // Close all pipe fds
                for (r_fd, w_fd) in &pipes {
                    let _ = close(r_fd.as_raw_fd());
                    let _ = close(w_fd.as_raw_fd());
                }

                // Builtin?
                if is_builtin(cmd_name) {
                    run_builtin(cmd_name, &args, None, None);
                    process::exit(0);
                }

                // External exec
                let cstrs: Vec<CString> = parts
                    .iter()
                    .map(|s| CString::new(s.as_str()).unwrap())
                    .collect();
                execvp(&cstrs[0], &cstrs).unwrap_or_else(|e| {
                    eprintln!("execvp failed: {}", e);
                    process::exit(1);
                });
                unreachable!();
            }
            Ok(ForkResult::Parent { .. }) => {
                // parent continues to next stage
            }
            Err(e) => {
                eprintln!("fork failed: {}", e);
                return;
            }
        }
    }

    // Parent: close all pipe ends
    for (r_fd, w_fd) in pipes {
        let _ = close(r_fd);
        let _ = close(w_fd);
    }

    // Wait for all children
    for _ in 0..num_cmds {
        let _ = waitpid(None, None);
    }
}

