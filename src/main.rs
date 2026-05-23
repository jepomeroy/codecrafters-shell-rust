//! Entry point for the interactive shell.
//!
//! Initialises the rustyline editor with tab-completion and runs the REPL loop.

use rustyline::{CompletionType, Config, config::BellStyle, history::DefaultHistory};

use crate::{autocomplete::AutoCompletion, processor::Processor};

mod autocomplete;
mod builtin;
mod command;
mod history;
mod jobs;
mod processor;
mod redirect;
mod utils;

/// Starts the interactive shell REPL: reads a line, dispatches it, and loops forever.
fn main() -> Result<(), anyhow::Error> {
    let mut commands = Processor::new();

    let config = Config::builder()
        .bell_style(BellStyle::Audible)
        .completion_type(CompletionType::List)
        .build();

    let mut rl = rustyline::Editor::<AutoCompletion, DefaultHistory>::with_config(config)?;

    rl.set_helper(Some(AutoCompletion::new(commands.shared_completions())));

    loop {
        let readline = rl.readline("$ ");
        match readline {
            Ok(line) => {
                let _ = rl.add_history_entry(line.clone().trim());
                commands.process_command(line.trim(), rl.history_mut());
            }
            Err(e) => println!("Error: {e}"),
        }
    }
}
