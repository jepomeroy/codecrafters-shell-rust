use anyhow::{Error, anyhow};
use std::process::exit;

pub(crate) fn process_command(input: &str) {
    match parse_cmd(input) {
        Ok((cmd, args)) => match cmd {
            "echo" => println!("{}", args.unwrap_or("")),
            "exit" => exit(0),
            _ => println!("{}: command not found", input),
        },
        Err(_) => println!(),
    }
}

fn parse_cmd(input: &str) -> Result<(&str, Option<&str>), anyhow::Error> {
    if input.is_empty() {
        return Err(anyhow!("command is empty"));
    }

    if let Some((cmd, args)) = input.split_once(' ') {
        return Ok((cmd, Some(args)));
    } else {
        return Ok((input, None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let input = "";
        let e = parse_cmd(input);
        assert!(e.is_err());
    }

    #[test]
    fn test_cmd_only_parse() {
        let input = "foo";
        if let Ok((cmd, args)) = parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_none());
        }
    }

    #[test]
    fn test_simple_parse() {
        let input = "foo bar";
        if let Ok((cmd, args)) = parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_some());
            assert_eq!(args, Some("bar"));
        }
    }

    #[test]
    fn test_long_parse() {
        let input = "foo bar baz bop";
        if let Ok((cmd, args)) = parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_some());
            assert_eq!(args, Some("bar baz bop"));
        }
    }
}
