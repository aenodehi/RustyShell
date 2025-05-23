#[allow(unused_imports)]
use std::io::{self, Write};
use std::process;

fn main() {
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input).unwrap();
        if bytes_read == 0 {
            break;
        }

        let command = input.trim();

        if command == "exit 0" {
            process::exit(0);
        }

        println!("{}: command not found", command);
    }
}
