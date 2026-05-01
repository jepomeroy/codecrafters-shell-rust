#[allow(unused_imports)]
use std::io::{self, Write};

fn main() {
    let mut input = String::new();
    print!("$ ");
    io::stdout().flush().unwrap();

    match io::stdin().read_line(&mut input) {
        Ok(_) => println!("{}: command not found", input.trim()),
        Err(e) => println!("Error: {e}"),
    }
}
