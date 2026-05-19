//! Shell command representation, argument parsing, and process execution.

use std::{
    collections::VecDeque,
    io::Error,
    process::{Child, ChildStdout, Command, Stdio},
};

use crate::redirect::{Redirect, RedirectType};

#[derive(Default, Clone)]
enum OutputType {
    #[default]
    Inherit,
    Piped,
}

impl OutputType {
    fn get_output(&self) -> Stdio {
        match self {
            OutputType::Inherit => Stdio::inherit(),
            OutputType::Piped => Stdio::piped(),
        }
    }
}

enum ParserState {
    /// Unquoted text; spaces split args and backslash escapes the next char.
    Normal,
    /// Backslash-single-quote region (`\'...\' `): content is literal, spaces don't split,
    /// `\'` closes (emitting `'`), `\"` emits `"`.
    BSQOpen,
    /// Single-quoted region (`'...'`): everything literal until the closing `'`.
    SingleQuote,
    /// Double-quoted region (`"..."`): backslash still escapes inside.
    DoubleQuote,
    /// After a `\` outside a single-quote: the next character is taken literally.
    Escaped,
}

/// A parsed shell command, optionally chained to the next command via a pipe.
#[derive(Clone)]
pub(crate) struct InternalCommand {
    cmd: String,
    args: Vec<String>,
    output: OutputType,
    redirect: Option<Redirect>,
    background: bool,
    next: Option<Box<InternalCommand>>,
}

impl Default for InternalCommand {
    fn default() -> Self {
        Self {
            cmd: Default::default(),
            args: Default::default(),
            output: Default::default(),
            redirect: Default::default(),
            background: Default::default(),
            next: Default::default(),
        }
    }
}

impl InternalCommand {
    /// Builds a pipeline from `inputs`, where each element is one pipe segment.
    ///
    /// Returns `None` when `inputs` is empty.
    pub(crate) fn new(mut inputs: VecDeque<&str>) -> Option<Self> {
        if let Some(curr_cmd_input) = inputs.pop_front() {
            // set up the output
            let output = if inputs.is_empty() {
                OutputType::Inherit
            } else {
                OutputType::Piped
            };

            // get the cmd and args
            let mut args = InternalCommand::parse_input(curr_cmd_input);
            let cmd: String = args.pop_front().expect("Command should exist");

            let background = args.back().is_some_and(|s| s == "&");
            let args = if background {
                let mut a: Vec<String> = Vec::from(args);
                a.pop();
                a
            } else {
                let a: Vec<String> = Vec::from(args);
                a
            };

            let redirect = Redirect::get_redirect(&args.clone());

            let next = InternalCommand::new(inputs);

            Some(Self {
                cmd,
                args,
                output,
                redirect,
                background,
                next: next.map(Box::new),
            })
        } else {
            None
        }
    }

    /// Creates a foreground command with the given executable path and argument list.
    pub(crate) fn new_with_args(cmd: String, args: Vec<String>) -> Self {
        let redirect = Redirect::get_redirect(&args.clone());

        Self {
            cmd,
            args,
            redirect,
            ..Default::default()
        }
    }

    /// Runs the command as a foreground child process and returns its exit code.
    ///
    /// If a redirect is present, stdout or stderr is wired to the target file.
    /// Without a redirect, the command's stdout is captured and printed to the shell's stdout.
    pub(crate) fn execute(&self, io_input: Option<ChildStdout>) -> i32 {
        if let Some(redirect) = &self.redirect {
            let args = self.args[0..redirect.position].to_vec();
            let mut command = Command::new(&self.cmd);

            command.args(args);

            let output_file = match redirect.get_redirect_file() {
                Ok(f) => f,
                Err(e) => {
                    println!("Error: {}", e);
                    return 1;
                }
            };

            let output = match redirect.redirect_type {
                RedirectType::StdOut => command.stdout(output_file).spawn(),
                RedirectType::StdErr => command.stderr(output_file).spawn(),
            };

            match output {
                Ok(mut child) => match child.wait() {
                    Ok(status) => status.code().unwrap_or(0),
                    Err(e) => {
                        println!("Error: {}", e);
                        1
                    }
                },
                Err(e) => {
                    println!("Error: {}", e);
                    1
                }
            }
        } else {
            let mut command = Command::new(&self.cmd);

            if self.next.is_some() {
                command.args(&self.args);
                if let Some(input) = io_input {
                    command.stdin(input);
                }
                let cmd_output = command
                    .stdout(self.output.get_output())
                    .spawn()
                    .expect("Failed to run cmd");
                self.next
                    .clone()
                    .unwrap()
                    .execute(Some(cmd_output.stdout.expect("Failed to get output")))
            } else {
                command.args(&self.args);
                if let Some(input) = io_input {
                    command.stdin(input);
                }
                match command
                    .stdout(self.output.get_output())
                    .output()
                {
                    Ok(output) => {
                        print!("{}", String::from_utf8_lossy(&output.stdout));
                        output.status.code().unwrap_or(0)
                    }
                    Err(e) => {
                        println!("Error: {}", e);
                        1
                    }
                }
            }
        }
    }

    /// Spawns the command as a background child process and returns the [`Child`] handle.
    pub(crate) fn execute_background(&mut self) -> Result<Child, Error> {
        Command::new(&self.cmd).args(self.args.iter()).spawn()
    }

    /// Returns the command name (the executable, without arguments).
    pub(crate) fn get_command(&self) -> &str {
        &self.cmd
    }

    /// Returns the command name and all arguments joined by spaces.
    pub(crate) fn get_command_with_args(&self) -> String {
        format!("{} {}", self.cmd, self.args.join(" "))
    }

    /// Returns a clone of the argument list (excludes the command name and any redirect tokens).
    pub(crate) fn get_args(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Returns `true` if the command was suffixed with `&` (run in the background).
    pub(crate) fn is_background(&self) -> bool {
        self.background
    }

    /// Parses the argument portion of a command line into a vector of strings,
    /// handling single quotes, double quotes, backslash-single-quote regions, and escape sequences.
    fn parse_input(args_str: &str) -> VecDeque<String> {
        let mut args = VecDeque::new();
        let mut current = String::new();
        let mut state = ParserState::Normal;
        // Remembers whether Escaped was entered from Normal or DoubleQuote,
        // so we can return to the right state afterward.
        let mut prev_state = ParserState::Normal;

        let mut iter = args_str.chars().peekable();

        while let Some(c) = iter.next() {
            match c {
                '\'' => match state {
                    ParserState::Normal => state = ParserState::SingleQuote,
                    ParserState::SingleQuote => state = ParserState::Normal,
                    ParserState::DoubleQuote => current.push('\''), // literal inside "..."
                    ParserState::BSQOpen => current.push('\''),     // literal inside \'...\'
                    ParserState::Escaped => {
                        // \' — literal single quote, return to prior context
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push('\'');
                    }
                },
                '\"' => match state {
                    ParserState::Normal => state = ParserState::DoubleQuote,
                    ParserState::SingleQuote => current.push('\"'), // literal inside '...'
                    ParserState::DoubleQuote => state = ParserState::Normal,
                    ParserState::BSQOpen => current.push('\"'), // literal inside \'...\'
                    ParserState::Escaped => {
                        // \" — literal double quote, return to prior context
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push('\"');
                    }
                },
                ' ' => match state {
                    ParserState::Normal => {
                        // Unquoted space: flush current token
                        if !current.is_empty() {
                            args.push_back(current.split_off(0));
                        }
                    }
                    ParserState::Escaped => {
                        // \ followed by space: literal space
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push(' ');
                    }
                    // Inside any quote context: space is literal
                    _ => current.push(' '),
                },
                '\\' => match state {
                    ParserState::Escaped => {
                        // \\ — literal backslash
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push('\\');
                    }
                    // Backslash is literal inside single quotes
                    ParserState::SingleQuote => current.push('\\'),
                    ParserState::BSQOpen => {
                        // Peek to decide: \' closes the region, \" escapes the quote,
                        // anything else the backslash is literal.
                        if iter.peek() == Some(&'\'') {
                            iter.next();
                            current.push('\'');
                            state = ParserState::Normal;
                        } else if iter.peek() == Some(&'"') {
                            iter.next();
                            current.push('"');
                        } else {
                            current.push('\\');
                        }
                    }
                    // \' in Normal: open a BSQ region and emit the leading '
                    ParserState::Normal if iter.peek() == Some(&'\'') => {
                        iter.next();
                        current.push('\'');
                        state = ParserState::BSQOpen;
                    }
                    _ => {
                        // Normal or DoubleQuote: start an escape sequence
                        prev_state = state;
                        state = ParserState::Escaped;
                    }
                },
                other => match state {
                    ParserState::Escaped => {
                        // Any other escaped char is taken literally (backslash consumed)
                        state = prev_state;
                        prev_state = ParserState::Normal;
                        current.push(other);
                    }
                    _ => current.push(other),
                },
            }
        }

        if !current.is_empty() {
            args.push_back(current);
        }

        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

    // --- Pipe execution ---

    #[test]
    fn test_pipe_first_command_no_panic() {
        // Regression: first cmd in a pipeline called execute(None) and panicked.
        let mut cmds: VecDeque<&str> = VecDeque::new();
        cmds.push_back("echo hello");
        cmds.push_back("cat");
        let cmd = InternalCommand::new(cmds).unwrap();
        assert_eq!(cmd.execute(None), 0);
    }

    #[test]
    fn test_pipe_args_forwarded_to_first_command() {
        // Regression: args were not passed to the first command in a pipeline.
        let tmp = env::temp_dir().join(format!("pipe_args_test_{}", std::process::id()));
        fs::write(&tmp, "a b c\n").unwrap();
        let first = format!("cat {}", tmp.to_string_lossy());
        let mut cmds: VecDeque<&str> = VecDeque::new();
        cmds.push_back(first.as_str());
        cmds.push_back("wc -w");
        let cmd = InternalCommand::new(cmds).unwrap();
        let code = cmd.execute(None);
        fs::remove_file(&tmp).ok();
        assert_eq!(code, 0);
    }

    #[test]
    fn test_pipe_last_command_receives_stdin() {
        // Regression: last cmd in pipeline ignored io_input and read empty stdin.
        // Uses `tee` to capture what actually flows through InternalCommand::execute.
        let tmp_in = env::temp_dir().join(format!("pipe_in_{}", std::process::id()));
        let tmp_out = env::temp_dir().join(format!("pipe_out_{}", std::process::id()));
        fs::write(&tmp_in, "hello world\n").unwrap();
        let first = format!("cat {}", tmp_in.to_string_lossy());
        let last = format!("tee {}", tmp_out.to_string_lossy());
        let mut cmds: VecDeque<&str> = VecDeque::new();
        cmds.push_back(first.as_str());
        cmds.push_back(last.as_str());
        let cmd = InternalCommand::new(cmds).unwrap();
        let code = cmd.execute(None);
        let content = fs::read_to_string(&tmp_out).unwrap_or_default();
        fs::remove_file(&tmp_in).ok();
        fs::remove_file(&tmp_out).ok();
        assert_eq!(code, 0);
        assert_eq!(content, "hello world\n");
    }

    #[test]
    fn test_pipe_cat_file_to_wc() {
        // Simulates the BR6 test case: cat <file> | wc  →  lines=5 words=10 bytes=78.
        // Uses tee to intercept the data that flows into wc so we can assert counts.
        let tmp_in = env::temp_dir().join(format!("pipe_wc_in_{}", std::process::id()));
        let tmp_tee = env::temp_dir().join(format!("pipe_wc_tee_{}", std::process::id()));
        fs::write(
            &tmp_in,
            "orange pear\nbanana pineapple\napple mango\nraspberry blueberry\ngrape strawberry\n",
        )
        .unwrap();
        // cat <file> | tee <tee_file>: captures the bytes that would reach wc.
        let first = format!("cat {}", tmp_in.to_string_lossy());
        let last = format!("tee {}", tmp_tee.to_string_lossy());
        let mut cmds: VecDeque<&str> = VecDeque::new();
        cmds.push_back(first.as_str());
        cmds.push_back(last.as_str());
        let cmd = InternalCommand::new(cmds).unwrap();
        let code = cmd.execute(None);
        let captured = fs::read_to_string(&tmp_tee).unwrap_or_default();
        fs::remove_file(&tmp_in).ok();
        fs::remove_file(&tmp_tee).ok();
        assert_eq!(code, 0);
        assert_eq!(captured.lines().count(), 5);
        assert_eq!(captured.split_whitespace().count(), 10);
        assert_eq!(captured.len(), 78);
    }

    #[test]
    fn test_pipe_second_command_receives_piped_stdin() {
        // Verify the mid-pipeline stdin wiring: execute with a ChildStdout still works.
        // We do this indirectly by chaining three commands.
        let mut cmds: VecDeque<&str> = VecDeque::new();
        cmds.push_back("echo hello world");
        cmds.push_back("cat");
        cmds.push_back("cat");
        let cmd = InternalCommand::new(cmds).unwrap();
        assert_eq!(cmd.execute(None), 0);
    }

    // --- Parse args ---
    #[test]
    fn test_parse_args() {
        let args = InternalCommand::parse_input("  arg1   arg2  arg3  ");
        assert_eq!(args, vec!["arg1", "arg2", "arg3"]);
    }

    #[test]
    fn test_parse_args_with_single_quotes() {
        let args = InternalCommand::parse_input("'arg1   arg2'");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_single_quotes() {
        let args = InternalCommand::parse_input("'arg1''arg2'");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_empty_single_quotes() {
        let args = InternalCommand::parse_input("arg1''arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quotes() {
        let args = InternalCommand::parse_input("\"arg1   arg2\"");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_double_quotes() {
        let args = InternalCommand::parse_input("\"arg1\"\"arg2\"");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_unquoted() {
        let args = InternalCommand::parse_input("\"arg1\"arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_separate_double_quotes() {
        let args = InternalCommand::parse_input("\"arg1\" \"arg2\"");
        assert_eq!(args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_inner_single_quote() {
        let args = InternalCommand::parse_input("\"arg1's arg2\"");
        assert_eq!(args, vec!["arg1's arg2"]);
    }

    // Literal chars in arg
    #[test]
    fn test_parse_args_backslash_spaces() {
        let args = InternalCommand::parse_input(r"arg1\ \ \ arg2");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_space_collapse_others() {
        let args = InternalCommand::parse_input(r"arg1\     arg2");
        assert_eq!(args, vec!["arg1 ", "arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_char() {
        let args = InternalCommand::parse_input(r"arg1\narg2");
        assert_eq!(args, vec!["arg1narg2"]);
    }

    #[test]
    fn test_parse_args_backslash_backslash() {
        let args = InternalCommand::parse_input(r"arg1\\arg2");
        assert_eq!(args, vec![r"arg1\arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_single_quote() {
        let args = InternalCommand::parse_input(r"\'arg1 arg2\'");
        assert_eq!(args, vec!["'arg1 arg2'"]);
    }

    // Support single quote sting literals
    #[test]
    fn test_parse_args_single_quote_with_multi_backslash() {
        let args = InternalCommand::parse_input(r"'arg1\\\arg2'");
        assert_eq!(args, vec![r"arg1\\\arg2"]);
    }

    #[test]
    fn test_parse_args_single_quote_with_backslash_double_quote() {
        let args = InternalCommand::parse_input("'arg1\"arg2'");
        assert_eq!(args, vec![r#"arg1"arg2"#]);
    }

    #[test]
    fn test_parse_args_backslash_single_quote_mixed() {
        let args = InternalCommand::parse_input("'arg1\"arg2\"arg3'");
        assert_eq!(args, vec![r#"arg1"arg2"arg3"#]);
    }

    #[test]
    fn test_parse_args_escaped_single_and_double_quotes() {
        let args = InternalCommand::parse_input(r#"\'\"arg1 arg2\"\'"#);
        assert_eq!(args, vec![r#"'"arg1 arg2"'"#]);
    }
}
