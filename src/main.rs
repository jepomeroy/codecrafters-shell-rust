use std::error::Error;

use rustyline::history::DefaultHistory;

use crate::{autocomplete::AutoCompletion, commands::Commands};

mod autocomplete;
mod builtin;
mod commands;
mod redirect;

fn main() -> Result<(), Box<dyn Error>> {
    let mut rl = rustyline::Editor::<AutoCompletion, DefaultHistory>::new()?;
    rl.set_helper(Some(AutoCompletion::new()));

    let commands = Commands::new();
    loop {
        let readline = rl.readline("$ ");
        match readline {
            Ok(line) => commands.process_command(line.trim()),
            Err(e) => println!("Error: {e}"),
        }
    }
}
