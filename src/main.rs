use rustyline::{CompletionType, Config, config::BellStyle, history::DefaultHistory};

use crate::{autocomplete::AutoCompletion, commands::Commands};

mod autocomplete;
mod builtin;
mod commands;
mod redirect;
mod utils;

/// Starts the interactive shell REPL: reads a line, dispatches it, and loops forever.
fn main() -> Result<(), anyhow::Error> {
    let mut commands = Commands::new();

    let config = Config::builder()
        .bell_style(BellStyle::Audible)
        .completion_type(CompletionType::List)
        .build();

    let mut rl = rustyline::Editor::<AutoCompletion, DefaultHistory>::with_config(config)?;

    rl.set_helper(Some(AutoCompletion::new(commands.shared_completions())));

    loop {
        let readline = rl.readline("$ ");
        match readline {
            Ok(line) => commands.process_command(line.trim()),
            Err(e) => println!("Error: {e}"),
        }
    }
}
