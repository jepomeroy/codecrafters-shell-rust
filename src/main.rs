#[allow(unused_imports)]
use std::io::{self, Write};

mod builtin;

fn main() {
    let mut input = String::new();
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        match io::stdin().read_line(&mut input) {
            Ok(_) => builtin::process_command(input.trim()),
            Err(e) => println!("Error: {e}"),
        }

        input.clear();
    }
}
