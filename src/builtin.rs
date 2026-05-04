use anyhow::anyhow;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::exit;
use std::{env, fs};

pub(crate) struct Builtin {}

impl Builtin {
    fn echo(args: Option<&str>) -> i32 {
        match args {
            Some(args) => println!("{}", args),
            _ => println!(),
        }

        0
    }

    fn exit() -> i32 {
        exit(0);
    }

    fn check_type(type_arg: &str, builtins: Vec<&str>, arg_path: Option<String>) -> i32 {
        if builtins.contains(&type_arg) {
            println!("{} is a shell builtin", type_arg);
        } else if let Some(path) = arg_path {
            println!("{} is {}", type_arg, path);
        } else {
            println!("{}: not found", type_arg);
        }

        0
    }

    fn unknown(cmd: &str) -> i32 {
        println!("{}: command not found", cmd);

        0
    }
}

pub(crate) struct Commands<'a> {
    commands: Vec<&'a str>,
    paths: Vec<String>,
}

impl<'a> Commands<'a> {
    pub(crate) fn new() -> Self {
        let commands = vec!["echo", "exit", "type"];

        let paths = match env::var("PATH") {
            Ok(path_var) => env::split_paths(&path_var)
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            Err(_) => vec![],
        };

        Self { commands, paths }
    }

    fn is_executalble_command(&self, cmd: &str) -> Option<String> {
        for path in &self.paths {
            let cmd_path = Path::new(path).join(cmd);

            if cmd_path.exists() {
                match fs::metadata(&cmd_path) {
                    Ok(metadata) => {
                        if metadata.permissions().mode() & 0o111 != 0 {
                            return Some(cmd_path.to_string_lossy().into_owned());
                        }
                    }
                    Err(e) => eprintln!("{e}"),
                }
            }
        }

        None
    }

    fn list_builtin_commands(&self) -> Vec<&str> {
        self.commands.clone()
    }

    fn parse_cmd(input: &str) -> Result<(&str, Option<&str>), anyhow::Error> {
        if input.is_empty() {
            return Err(anyhow!("command is empty"));
        }

        if let Some((cmd, args)) = input.split_once(' ') {
            Ok((cmd, Some(args)))
        } else {
            Ok((input, None))
        }
    }

    pub(crate) fn process_command(&self, input: &str) {
        match Commands::parse_cmd(input) {
            Ok((cmd, args)) => {
                match cmd {
                    "echo" => Builtin::echo(args),
                    "exit" => Builtin::exit(),
                    "type" => {
                        let arg_cmd = args.unwrap_or("");
                        Builtin::check_type(
                            arg_cmd,
                            self.list_builtin_commands(),
                            self.is_executalble_command(arg_cmd),
                        )
                    }
                    _ => match self.is_executalble_command(cmd) {
                        Some(_path) => todo!(),
                        None => Builtin::unknown(cmd),
                    },
                };
            }
            Err(_) => println!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let input = "";
        let e = Commands::parse_cmd(input);
        assert!(e.is_err());
    }

    #[test]
    fn test_cmd_only_parse() {
        let input = "foo";
        if let Ok((cmd, args)) = Commands::parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_none());
        }
    }

    #[test]
    fn test_simple_parse() {
        let input = "foo bar";
        if let Ok((cmd, args)) = Commands::parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_some());
            assert_eq!(args, Some("bar"));
        }
    }

    #[test]
    fn test_long_parse() {
        let input = "foo bar baz bop";
        if let Ok((cmd, args)) = Commands::parse_cmd(input) {
            assert_eq!(cmd, "foo");
            assert!(args.is_some());
            assert_eq!(args, Some("bar baz bop"));
        }
    }

    #[test]
    fn test_list_commands() {
        let commands = Commands::new();
        let builtins = commands.list_builtin_commands();

        assert_eq!(builtins, vec!["echo", "exit", "type"])
    }
}
