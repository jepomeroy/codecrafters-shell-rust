use std::process::exit;

pub(crate) fn process_command(input: &str) {
    match input {
        "exit" => exit(0),
        _ => println!("{}: command not found", input),
    }
}
