use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::{
        fs::PermissionsExt,
        io::{FromRawFd, RawFd},
        process::CommandExt,
    },
    path::Path,
    process::{self, Command, Stdio},
};

use nix::unistd::{close, dup2, pipe};

use rustyline::{
    completion::{Completer, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::Hinter,
    history::FileHistory,
    validate::{ValidationContext, ValidationResult, Validator},
    Context, Editor, Helper, Config, CompletionType,
};

struct ShellCompleter;

impl Helper for ShellCompleter {}
impl Highlighter for ShellCompleter {}

impl Hinter for ShellCompleter {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Validator for ShellCompleter {
    fn validate(&self, _ctx: &mut ValidationContext<'_>) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}


impl Completer for ShellCompleter {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let prefix = &line[..pos];
        let mut completions = vec![];

        for &builtin in &["exit", "echo", "cd", "pwd", "type"] {
            if builtin.starts_with(prefix) {
                completions.push(Pair {
                    display: builtin.into(),
                    replacement: format!("{} ", builtin),
                });
            }
        }

        if let Ok(paths) = env::var("PATH") {
            for dir in paths.split(':') {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(prefix) {
                            completions.push(Pair {
                                display: name.clone(),
                                replacement: format!("{} ", name),
                            });
                        }
                    }
                }
            }
        }

        completions.sort_by(|a, b| a.display.cmp(&b.display));

        let start = prefix.rfind(|c: char| c == ' ' || c == '\t').map_or(0, |i| i + 1);

        Ok((start, completions))
    }
}





// ========== Main ==========

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::builder().completion_type(CompletionType::List).build();
    let mut rl = Editor::<ShellCompleter, FileHistory>::with_config(config)?;
    rl.set_helper(Some(ShellCompleter));

    loop {
        let line = match rl.readline("$ ") {
            Ok(line) if !line.trim().is_empty() => {
                rl.add_history_entry(line.as_str()).ok();
                line
            }
            Ok(_) => continue,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break Ok(()),
            Err(_) => continue,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.contains('|') {
            let pipeline_parts: Vec<Vec<String>> = trimmed
                .split('|')
                .map(|s| tokenize(s.trim()))
                .collect();
            execute_pipeline(pipeline_parts);
            continue;
        }

        let tokens = tokenize(trimmed);
        if tokens.is_empty() {
            continue;
        }

        if run_builtin(&tokens) {
            continue;
        }

        run_external(tokens)?;
    }
}


// ========== Tokenizer ==========
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' => {
                if let Some(next_ch) = chars.next() {
                    current.push(next_ch);
                }
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                while matches!(chars.peek(), Some(' ' | '\t')) {
                    chars.next();
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

// ========== Built-in Commands ==========

fn run_builtin(tokens: &[String]) -> bool {
    match tokens[0].as_str() {
        "exit" if tokens.len() == 2 && tokens[1] == "0" => {
            process::exit(0);
        }
        "echo" => {
            let mut args = tokens[1..].to_vec();
            let (stdout, stderr) = handle_redirection(&mut args);
            let output = args.join(" ");

            if let Some(mut out_file) = stdout {
                writeln!(out_file, "{}", output).ok();
            } else {
                println!("{}", output);
            }

            if let Some(mut err_file) = stderr {
                writeln!(err_file, "").ok();
            }

            true
        }
        "cd" => {
            let dir = tokens.get(1).map(|s| s.as_str()).unwrap_or("");
            let path = if dir == "~" {
                env::var("HOME").unwrap_or_else(|_| ".".to_string())
            } else {
                dir.to_string()
            };

            if let Err(_) = env::set_current_dir(path) {
                eprintln!("cd: No such file or directory");
            }
            true
        }
        "pwd" => {
            match env::current_dir() {
                Ok(path) => println!("{}", path.display()),
                Err(e) => eprintln!("pwd: {}", e),
            }
            true
        }
        "type" => {
            if let Some(arg) = tokens.get(1) {
                if ["echo", "exit", "type", "pwd", "cd"].contains(&arg.as_str()) {
                    println!("{} is a shell builtin", arg);
                } else {
                    check_type(arg);
                }
            } else {
                println!("type: missing argument");
            }
            true
        }
        _ => false,
    }
}

fn check_type(cmd: &str) {
    if let Ok(path_var) = env::var("PATH") {
        for dir in path_var.split(':') {
            let path = Path::new(dir).join(cmd);
            if check_executable(&path, cmd) {
                return;
            }
        }
    }
    println!("{}: not found", cmd);
}

fn check_executable(path: &Path, cmd: &str) -> bool {
    if path.exists() && path.is_file() && path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false) {
        println!("{} is {}", cmd, path.display());
        true
    } else {
        false
    }
}

// ========== External Command Execution ==========

fn run_external(mut tokens: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let command = tokens[0].clone();
    let (_stderr_path, _stderr_append) = parse_stderr_redirection(&mut tokens);
    let (stdout_redirect, stderr_redirect) = handle_redirection(&mut tokens);

    if let Ok(path_var) = env::var("PATH") {
        for dir in path_var.split(':') {
            let full_path = Path::new(dir).join(&command);
            if check_executable(&full_path, &command) {
                let mut cmd = Command::new(full_path);
                cmd.args(&tokens[1..]);

                if let Some(ref out) = stdout_redirect {
                    cmd.stdout(Stdio::from(out.try_clone()?));
                }

                if let Some(ref err) = stderr_redirect {
                    cmd.stderr(Stdio::from(err.try_clone()?));
                }

                match cmd.spawn().and_then(|c| c.wait_with_output()) {
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
                return Ok(());
            }
        }
    }

    println!("{}: command not found", command);
    Ok(())
}

// ========== Redirection Helpers ==========

fn parse_stderr_redirection(tokens: &mut Vec<String>) -> (Option<String>, bool) {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "2>" || tokens[i] == "2>>" {
            if i + 1 < tokens.len() {
                let path = tokens.remove(i + 1);
                let append = tokens.remove(i) == "2>>";
                return (Some(path), append);
            }
        }
        i += 1;
    }
    (None, false)
}

fn handle_redirection(tokens: &mut Vec<String>) -> (Option<File>, Option<File>) {
    let mut stdout = None;
    let mut stderr = None;
    let mut i = 0;

    while i < tokens.len() {
        let is_stdout = matches!(tokens[i].as_str(), ">" | "1>" | ">>" | "1>>");
        let is_stderr = matches!(tokens[i].as_str(), "2>" | "2>>");

        if (is_stdout || is_stderr) && i + 1 < tokens.len() {
            let path = tokens.remove(i + 1);
            let mode = tokens.remove(i);

            let file = if mode.ends_with(">>") {
                OpenOptions::new().append(true).create(true).open(path)
            } else {
                File::create(path)
            };

            match file {
                Ok(f) => {
                    if is_stdout {
                        stdout = Some(f);
                    } else {
                        stderr = Some(f);
                    }
                }
                Err(e) => {
                    eprintln!("Redirection error: {}", e);
                }
            }

            continue;
        }

        i += 1;
    }

    (stdout, stderr)
}

// ========== Pipeline ==========

fn execute_pipeline(commands: Vec<Vec<String>>) {
    let mut prev_read: Option<RawFd> = None;

    for (i, cmd) in commands.iter().enumerate() {
        let (reader, writer) = if i < commands.len() - 1 {
            match pipe() {
                Ok((r, w)) => (Some(r), Some(w)),
                Err(e) => {
                    eprintln!("pipe error: {}", e);
                    return;
                }
            }
        } else {
            (None, None)
        };

        match unsafe { libc::fork() } {
            -1 => {
                eprintln!("fork failed");
                return;
            }
            0 => {
                if let Some(fd) = prev_read {
                    dup2(fd, libc::STDIN_FILENO).ok();
                    close(fd).ok();
                }

                if let Some(fd) = writer {
                    dup2(fd, libc::STDOUT_FILENO).ok();
                    close(fd).ok();
                }

                if let Err(e) = Command::new(&cmd[0])
                    .args(&cmd[1..])
                    .exec()
                {
                    eprintln!("exec failed: {}", e);
                    process::exit(1);
                }
            }
            pid => {
                if let Some(fd) = prev_read {
                    close(fd).ok();
                }
                if let Some(fd) = writer {
                    close(fd).ok();
                }

                prev_read = reader;
                unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
            }
        }
    }
}
