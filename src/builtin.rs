use anyhow::anyhow;
use std::{collections::HashMap, process::exit};

struct CommandData<'a> {
    cmd: &'a str,
    args: Option<&'a str>,
    builtin: Vec<&'a str>,
}

impl<'a> CommandData<'a> {
    fn new(cmd: &'a str, args: Option<&'a str>, builtin: Vec<&'a str>) -> Self {
        Self { cmd, args, builtin }
    }
}

pub(crate) struct Builtin {}

impl Builtin {
    fn echo(cmd_data: CommandData) -> i32 {
        match cmd_data.args {
            Some(args) => println!("{}", args),
            _ => println!(),
        }

        0
    }

    fn exit(_cmd_data: CommandData) -> i32 {
        exit(0);
    }

    fn check_type(cmd_data: CommandData) -> i32 {
        if let Some(type_arg) = cmd_data.args {
            if cmd_data.builtin.contains(&type_arg) {
                println!("{} is a shell builtin", type_arg);
            } else {
                println!("{}: not found", type_arg);
            }
        } else {
            println!("type requires one arg");
        }

        0
    }

    fn unknown(cmd_data: CommandData) -> i32 {
        println!("{}: command not found", cmd_data.cmd);

        0
    }
}

pub(crate) struct Commands<'a> {
    commands: HashMap<&'a str, for<'b> fn(CommandData<'b>) -> i32>,
}

impl<'a> Commands<'a> {
    pub(crate) fn new() -> Self {
        let mut cmds = HashMap::<&str, for<'b> fn(CommandData<'b>) -> i32>::new();

        // echo
        cmds.insert("echo", Builtin::echo);
        // exit
        cmds.insert("exit", Builtin::exit);
        // type
        cmds.insert("type", Builtin::check_type);

        Self { commands: cmds }
    }

    fn get_command(&self, name: &str) -> Option<&for<'b> fn(CommandData<'b>) -> i32> {
        self.commands.get(name)
    }

    fn is_builtin(&self, cmd: &str) -> bool {
        self.commands.contains_key(cmd)
    }

    fn list_builtin_commands(&self) -> Vec<&str> {
        self.commands.keys().copied().collect()
    }
}

pub(crate) fn process_command(input: &str) {
    let cmds = Commands::new();
    match parse_cmd(input) {
        Ok((cmd, args)) => {
            let cmd_data = CommandData::new(cmd, args, cmds.list_builtin_commands());
            if let Some(func) = cmds.get_command(cmd) {
                func(cmd_data);
            } else {
                Builtin::unknown(cmd_data);
            }
        }
        Err(_) => println!(),
    }
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

    #[test]
    fn test_is_builtin() {
        let commands = Commands::new();
        let builtin = commands.list_builtin_commands();

        for b in builtin {
            assert!(commands.is_builtin(b))
        }
    }
}
