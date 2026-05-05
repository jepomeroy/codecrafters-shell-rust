use crate::commands::Commands;
use std::io::{self, Write};

mod builtin;
mod commands;

fn main() {
    let mut input = String::new();
    let commands = Commands::new();
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        match io::stdin().read_line(&mut input) {
            Ok(_) => commands.process_command(input.trim()),
            Err(e) => println!("Error: {e}"),
        }

        input.clear();
    }
}
