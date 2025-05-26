# RustyShell

*In this challenge, I build my own POSIX-compliant shell capable of interpreting shell commands, running external programs, and supporting builtin commands like **`cd`**, **`pwd`**, **`echo`**, and more.*

**Built by CodeCrafters.io**

**RustyShell** is a minimal shell implemented in Rust, offering the following features:

- **Built-in commands**: `cd`, `pwd`, `echo`, `exit`, `type`, `history`
- **External command execution** via `PATH`
- **Multi-stage pipelines** (e.g. `ls | grep foo | sort`)
- **I/O redirection**
  - Standard output: `>`, `>>`
  - Standard error: `2>`, `2>>`
  - Combined pipelines suppress broken-pipe errors
- **Command history** with optional limit: `history [n]`
- **Tab completion** for builtins and executables using `rustyline`
- **Single- and double-quote handling** to preserve literal text and spaces

---

## Getting Started

### Prerequisites

- Rust toolchain (`rustc` and `cargo`)
- Unix-like environment (Linux, macOS)

### Building

Clone the repository and build with Cargo:

```bash
git clone https://github.com/aenodehi/RustyShell.git
cd rusty-shell
cargo build --release
```

The compiled binary will be available at `target/release/rusty-shell`.

### Running

Launch the shell:

```bash
./target/release/rusty-shell
```

You’ll see a prompt:

```bash
$
```

Type commands as you would in a typical POSIX shell.

---

## Features

### Built-ins

- **`cd [dir]`**  
  Change working directory. Supports absolute paths, relative paths, and `~` for the home directory.

- **`pwd`**  
  Print the current working directory.

- **`echo ...`**  
  Print arguments to standard output. Supports stdout and stderr redirection.

- **`type <cmd>`**  
  Identify whether `<cmd>` is a shell builtin or an external executable in `$PATH`.

- **`history [n]`**  
  List previously entered commands. With `[n]`, limit output to the last `n` entries.

- **`exit 0`**  
  Exit the shell with status code 0.

### External Commands & PATH

RustyShell searches the `PATH` environment variable for executables:

```bash
$ ls -l /tmp
```

### Pipelines

Chain multiple commands with `|`:

```bash
$ cat file.txt | grep foo | wc -l
```

Intermediate stages redirect stderr to `/dev/null` to suppress broken-pipe errors.

### I/O Redirection

- **Stdout**: `>` to overwrite, `>>` to append  
- **Stderr**: `2>` to overwrite, `2>>` to append

```bash
$ ls missing.txt 2> errors.log
$ echo "Done" >> output.log
```

### Tab Completion

Press `<TAB>` to complete builtin names or executable filenames. Lists multiple matches if ambiguous.

### Quoting

Supports single (`'…'`) and double (`"…"`) quotes to preserve literal text, including spaces and special characters.

---

## Development

- **Main source**: `src/main.rs`  
- **`tokenize()`**: parses input into tokens, handling quotes and escapes  
- **`handle_pipeline()`**: sets up Unix pipes and forks for multi-stage pipelines  
- **`run_builtin()`**: executes shell builtins  

Contributions welcome!

---

## License

MIT License — feel free to use, modify, and distribute.
