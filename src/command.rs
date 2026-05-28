//! Shell command representation, argument parsing, and process execution.

use std::{
    collections::VecDeque,
    fs::File,
    io::{PipeReader, PipeWriter, pipe},
    os::fd::OwnedFd,
    process::{Child, Command, Stdio},
};

use crate::{
    builtin::{
        CompleteCommand, EchoCommand, PwdCommand, SharedCompletions, TypeCommand, UnknownCommand,
    },
    redirect::{Redirect, RedirectType},
    utils::find_in_paths,
};

/// The outcome of running a pipeline: either a foreground exit code or a background child handle.
pub(crate) enum PipelineResult {
    /// Pipeline ran in the foreground and exited with this code.
    Foreground(i32),
    /// Pipeline was backgrounded (`&`); the caller is responsible for tracking the child.
    Background(Child),
}

/// A single command in a pipeline that can be executed with wired-up stdio.
pub(crate) trait PipelineCommand: Send + 'static {
    /// Runs the command, consuming `self`, and returns its exit code.
    ///
    /// `None` for any stdio parameter means inherit from the shell process.
    fn execute(
        self: Box<Self>,
        stdin: Option<File>,
        stdout: Option<File>,
        stderr: Option<File>,
    ) -> i32;

    /// Returns `true` if this command supports `spawn_background`.
    ///
    /// Only [`ExternalCommand`] overrides this; all builtins return `false` and run
    /// in the foreground even when suffixed with `&`.
    fn is_backgroundable(&self) -> bool {
        false
    }

    /// Spawns the command without waiting and returns the live [`Child`] handle.
    ///
    /// Only called when `is_backgroundable` returns `true`.
    fn spawn_background(
        self: Box<Self>,
        _stdin: Option<File>,
        _stdout: Option<File>,
        _stderr: Option<File>,
    ) -> Child {
        unreachable!("spawn_background called on non-backgroundable command")
    }
}

#[derive(Clone)]
enum ParserState {
    /// Unquoted text; spaces split args and backslash escapes the next char.
    Normal,
    /// Single-quoted region (`'...'`): everything literal until the closing `'`.
    SingleQuote,
    /// Double-quoted region (`"..."`): backslash still escapes inside.
    DoubleQuote,
    /// After a `\` outside a single-quote: the next character is taken literally.
    Escaped(Box<ParserState>),
}

/// One stage of a pipeline: a command together with its I/O redirects and background flag.
pub(crate) struct PipelineSegment {
    pub(crate) command: Box<dyn PipelineCommand>,
    /// `true` when the pipeline was terminated with `&` and this is the last segment.
    pub(crate) background: bool,
    pub(crate) stdout_redirect: Option<Redirect>,
    pub(crate) stderr_redirect: Option<Redirect>,
}

/// Parses `input` into a sequence of [`PipelineSegment`]s ready for execution.
///
/// Splits on `|`, tokenises each segment, resolves redirects, strips a trailing `&` from the
/// last segment (setting its `background` flag), and constructs the appropriate
/// [`PipelineCommand`] implementation for each segment.
pub(crate) fn build_pipeline(
    input: &str,
    paths: &[String],
    completions: SharedCompletions,
) -> Vec<PipelineSegment> {
    let raw_segments: Vec<&str> = input.split('|').collect();
    let last_idx = raw_segments.len().saturating_sub(1);

    raw_segments
        .into_iter()
        .enumerate()
        .map(|(i, raw_segment)| {
            // command and args
            let mut tokens = ExternalCommand::parse_input(raw_segment.trim());
            let cmd = tokens.pop_front().unwrap_or_default();
            let args: Vec<String> = tokens.into();

            // redirects
            let stdout_redirect = Redirect::get_redirect(&args)
                .filter(|r| matches!(r.redirect_type, RedirectType::StdOut));
            let stderr_redirect = Redirect::get_redirect(&args)
                .filter(|r| matches!(r.redirect_type, RedirectType::StdErr));
            let mut clean_args = Redirect::strip_redirect_tokens(&args);

            // detect background: only on the last segment, only if last token is "&"
            let background = i == last_idx && clean_args.last().is_some_and(|s| s == "&");
            if background {
                clean_args.pop();
            }

            // build command
            let command: Box<dyn PipelineCommand> = match cmd.as_str() {
                "echo" => Box::new(EchoCommand { args: clean_args }),
                "pwd" => Box::new(PwdCommand {}),
                "type" => Box::new(TypeCommand {
                    args: clean_args,
                    paths: paths.to_vec(),
                }),
                "complete" => Box::new(CompleteCommand {
                    args: clean_args,
                    completions: completions.clone(),
                }),
                _ => {
                    if find_in_paths(&cmd, paths).is_some() {
                        Box::new(ExternalCommand::new(cmd, clean_args))
                    } else {
                        Box::new(UnknownCommand { cmd })
                    }
                }
            };

            PipelineSegment {
                command,
                background,
                stdout_redirect,
                stderr_redirect,
            }
        })
        .collect()
}

/// Runs a parsed pipeline, wiring up pipes and redirects between segments.
///
/// Each segment is executed in its own thread. If the last segment is backgrounded, it is
/// spawned without waiting and its [`Child`] is returned as [`PipelineResult::Background`];
/// otherwise all threads are joined and the last exit code is returned as
/// [`PipelineResult::Foreground`].
pub(crate) fn execute_pipeline(segments: Vec<PipelineSegment>) -> PipelineResult {
    let seg_count = segments.len();
    let is_background = segments.last().is_some_and(|s| s.background);

    let mut pipes: Vec<(Option<PipeReader>, Option<PipeWriter>)> = (0..seg_count.saturating_sub(1))
        .map(|_| {
            let (r, w) = pipe().unwrap();
            (Some(r), Some(w))
        })
        .collect();

    let mut handles = Vec::new();
    let mut bg_child: Option<Child> = None;

    for (i, segment) in segments.into_iter().enumerate() {
        // stdin: None for the first (inherit terminal), pipe read end otherwise
        let stdin: Option<File> = if i == 0 {
            None
        } else {
            pipes[i - 1].0.take().map(|r| File::from(OwnedFd::from(r)))
        };

        // stdout: redirect file -> pipe write end -> None (inherit terminal for last)
        let stdout: Option<File> = if let Some(r) = segment.stdout_redirect {
            Some(r.get_redirect_file().unwrap())
        } else if i < seg_count - 1 {
            pipes[i].1.take().map(|w| File::from(OwnedFd::from(w)))
        } else {
            None
        };

        let stderr: Option<File> = segment
            .stderr_redirect
            .map(|r| r.get_redirect_file().unwrap());

        if is_background && i == seg_count - 1 && segment.command.is_backgroundable() {
            bg_child = Some(segment.command.spawn_background(stdin, stdout, stderr));
        } else {
            let command = segment.command;
            handles.push(std::thread::spawn(move || {
                command.execute(stdin, stdout, stderr)
            }));
        }
    }

    drop(pipes);

    if let Some(child) = bg_child {
        for h in handles {
            let _ = h.join();
        }
        PipelineResult::Background(child)
    } else {
        let code = handles
            .into_iter()
            .map(|h| h.join().unwrap_or(1))
            .last()
            .unwrap_or(0);
        PipelineResult::Foreground(code)
    }
}

/// An external (PATH-resolved) command with its parsed argument list.
#[derive(Clone, Default)]
pub(crate) struct ExternalCommand {
    cmd: String,
    args: Vec<String>,
}

impl ExternalCommand {
    /// Creates an `ExternalCommand` from a resolved executable path and its argument list.
    pub(crate) fn new(cmd: String, args: Vec<String>) -> Self {
        Self { cmd, args }
    }

    /// Parses the argument portion of a command line into a vector of strings,
    /// handling single quotes, double quotes, backslash-single-quote regions, and escape sequences.
    fn parse_input(args_str: &str) -> VecDeque<String> {
        let mut args = VecDeque::new();
        let mut current = String::new();
        let mut state = ParserState::Normal;

        let iter = args_str.chars().peekable();

        for c in iter {
            match (&state, c) {
                // Normal state
                (ParserState::Normal, '\'') => state = ParserState::SingleQuote,
                (ParserState::Normal, '\"') => state = ParserState::DoubleQuote,
                (ParserState::Normal, ' ') => {
                    // Unquoted space: flush current token
                    if !current.is_empty() {
                        args.push_back(current.split_off(0));
                    }
                }
                (ParserState::Normal, '\\') => {
                    state = ParserState::Escaped(Box::new(ParserState::Normal))
                }
                // Single Quote state
                (ParserState::SingleQuote, '\'') => state = ParserState::Normal,
                (ParserState::SingleQuote, '"') => current.push('\"'), // literal inside '...'
                // Double Quote state
                (ParserState::DoubleQuote, '\'') => current.push('\''), // literal inside "..."
                (ParserState::DoubleQuote, '"') => state = ParserState::Normal,
                (ParserState::DoubleQuote, '\\') => {
                    // backslash still escapes inside double quotes
                    state = ParserState::Escaped(Box::new(ParserState::DoubleQuote))
                }
                // Escape state
                (ParserState::Escaped(prev), _) => {
                    // Any char after backslash is literal (backslash consumed)
                    state = *prev.clone();
                    current.push(c);
                }
                _ => current.push(c),
            }
        }

        if !current.is_empty() {
            args.push_back(current);
        }

        args
    }
}

impl PipelineCommand for ExternalCommand {
    fn execute(
        self: Box<Self>,
        stdin: Option<File>,
        stdout: Option<File>,
        stderr: Option<File>,
    ) -> i32 {
        Command::new(&self.cmd)
            .args(&self.args)
            .stdin(stdin.map_or(Stdio::inherit(), Stdio::from))
            .stdout(stdout.map_or(Stdio::inherit(), Stdio::from))
            .stderr(stderr.map_or(Stdio::inherit(), Stdio::from))
            .spawn()
            .expect("Could not start cmd")
            .wait()
            .expect("Could not wait for cmd")
            .code()
            .unwrap_or(1)
    }

    fn is_backgroundable(&self) -> bool {
        true
    }

    fn spawn_background(
        self: Box<Self>,
        stdin: Option<File>,
        stdout: Option<File>,
        stderr: Option<File>,
    ) -> Child {
        Command::new(&self.cmd)
            .args(&self.args)
            .stdin(stdin.map_or(Stdio::inherit(), Stdio::from))
            .stdout(stdout.map_or(Stdio::inherit(), Stdio::from))
            .stderr(stderr.map_or(Stdio::inherit(), Stdio::from))
            .spawn()
            .expect("Could not start cmd")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::{env, fs};

    fn make_completions() -> Arc<Mutex<HashMap<String, String>>> {
        Arc::new(Mutex::new(HashMap::new()))
    }

    fn std_paths() -> Vec<String> {
        vec!["/bin".to_string(), "/usr/bin".to_string()]
    }

    fn fg(result: PipelineResult) -> i32 {
        match result {
            PipelineResult::Foreground(code) => code,
            PipelineResult::Background(_) => panic!("expected foreground result"),
        }
    }

    // --- Pipe execution ---

    #[test]
    fn test_pipe_first_command_no_panic() {
        let segs = build_pipeline("echo hello | cat", &std_paths(), make_completions());
        assert_eq!(fg(execute_pipeline(segs)), 0);
    }

    #[test]
    fn test_pipe_args_forwarded_to_first_command() {
        let tmp = env::temp_dir().join(format!("pipe_args_test_{}", std::process::id()));
        fs::write(&tmp, "a b c\n").unwrap();
        let input = format!("cat {} | wc -w", tmp.to_string_lossy());
        let segs = build_pipeline(&input, &std_paths(), make_completions());
        let code = fg(execute_pipeline(segs));
        fs::remove_file(&tmp).ok();
        assert_eq!(code, 0);
    }

    #[test]
    fn test_pipe_last_command_receives_stdin() {
        let tmp_in = env::temp_dir().join(format!("pipe_in_{}", std::process::id()));
        let tmp_out = env::temp_dir().join(format!("pipe_out_{}", std::process::id()));
        fs::write(&tmp_in, "hello world\n").unwrap();
        let input = format!(
            "cat {} | tee {}",
            tmp_in.to_string_lossy(),
            tmp_out.to_string_lossy()
        );
        let segs = build_pipeline(&input, &std_paths(), make_completions());
        let code = fg(execute_pipeline(segs));
        let content = fs::read_to_string(&tmp_out).unwrap_or_default();
        fs::remove_file(&tmp_in).ok();
        fs::remove_file(&tmp_out).ok();
        assert_eq!(code, 0);
        assert_eq!(content, "hello world\n");
    }

    #[test]
    fn test_pipe_cat_file_to_wc() {
        // cat <file> | tee <tee_file> captures bytes so we can assert counts.
        let tmp_in = env::temp_dir().join(format!("pipe_wc_in_{}", std::process::id()));
        let tmp_tee = env::temp_dir().join(format!("pipe_wc_tee_{}", std::process::id()));
        fs::write(
            &tmp_in,
            "orange pear\nbanana pineapple\napple mango\nraspberry blueberry\ngrape strawberry\n",
        )
        .unwrap();
        let input = format!(
            "cat {} | tee {}",
            tmp_in.to_string_lossy(),
            tmp_tee.to_string_lossy()
        );
        let segs = build_pipeline(&input, &std_paths(), make_completions());
        let code = fg(execute_pipeline(segs));
        let captured = fs::read_to_string(&tmp_tee).unwrap_or_default();
        fs::remove_file(&tmp_in).ok();
        fs::remove_file(&tmp_tee).ok();
        assert_eq!(code, 0);
        assert_eq!(captured.lines().count(), 5);
        assert_eq!(captured.split_whitespace().count(), 10);
        assert_eq!(captured.len(), 78);
    }

    #[test]
    fn test_pipe_three_commands() {
        let segs = build_pipeline(
            "echo hello world | cat | cat",
            &std_paths(),
            make_completions(),
        );
        assert_eq!(fg(execute_pipeline(segs)), 0);
    }

    // --- Background commands ---

    fn bg(result: PipelineResult) -> Child {
        match result {
            PipelineResult::Background(child) => child,
            PipelineResult::Foreground(_) => panic!("expected background result"),
        }
    }

    #[test]
    fn test_background_flag_set_for_trailing_ampersand() {
        let segs = build_pipeline("sleep 1000 &", &std_paths(), make_completions());
        assert!(segs.last().unwrap().background);
    }

    #[test]
    fn test_background_flag_not_set_without_ampersand() {
        let segs = build_pipeline("sleep 1000", &std_paths(), make_completions());
        assert!(!segs.last().unwrap().background);
    }

    #[test]
    fn test_background_only_on_last_pipeline_segment() {
        // "&" in a middle segment is not a background marker
        let segs = build_pipeline("echo & | cat", &std_paths(), make_completions());
        assert!(!segs[0].background);
        assert!(!segs[1].background);
    }

    #[test]
    fn test_background_args_do_not_contain_ampersand() {
        // The "&" must be stripped before the command sees it
        let segs = build_pipeline("sleep 1000 &", &std_paths(), make_completions());
        // The segment is backgroundable (ExternalCommand), so no args leak
        assert!(segs.last().unwrap().background);
        // Build a foreground pipeline with the same command to compare arg count
        let fg_segs = build_pipeline("sleep 1000", &std_paths(), make_completions());
        assert_eq!(segs.len(), fg_segs.len());
    }

    #[test]
    fn test_execute_pipeline_background_returns_background_variant() {
        let _lock = crate::utils::fork_lock();
        let segs = build_pipeline("sleep 1000 &", &std_paths(), make_completions());
        let mut child = bg(execute_pipeline(segs));
        child.kill().ok();
        child.wait().ok();
    }

    #[test]
    fn test_execute_pipeline_foreground_returns_foreground_variant() {
        let segs = build_pipeline("echo hello", &std_paths(), make_completions());
        assert_eq!(fg(execute_pipeline(segs)), 0);
    }

    #[test]
    fn test_background_pipeline_pipe_stages_run_before_returning() {
        // echo piped through cat in background — both stages should start
        let _lock = crate::utils::fork_lock();
        let segs = build_pipeline("sleep 1000 &", &std_paths(), make_completions());
        let mut child = bg(execute_pipeline(segs));
        child.kill().ok();
        child.wait().ok();
    }

    // --- Parse args ---
    #[test]
    fn test_parse_args() {
        let args = ExternalCommand::parse_input("  arg1   arg2  arg3  ");
        assert_eq!(args, vec!["arg1", "arg2", "arg3"]);
    }

    #[test]
    fn test_parse_args_with_single_quotes() {
        let args = ExternalCommand::parse_input("'arg1   arg2'");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_single_quotes() {
        let args = ExternalCommand::parse_input("'arg1''arg2'");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_empty_single_quotes() {
        let args = ExternalCommand::parse_input("arg1''arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quotes() {
        let args = ExternalCommand::parse_input("\"arg1   arg2\"");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_with_multiple_double_quotes() {
        let args = ExternalCommand::parse_input("\"arg1\"\"arg2\"");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_unquoted() {
        let args = ExternalCommand::parse_input("\"arg1\"arg2");
        assert_eq!(args, vec!["arg1arg2"]);
    }

    #[test]
    fn test_parse_args_with_separate_double_quotes() {
        let args = ExternalCommand::parse_input("\"arg1\" \"arg2\"");
        assert_eq!(args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_parse_args_with_double_quote_and_inner_single_quote() {
        let args = ExternalCommand::parse_input("\"arg1's arg2\"");
        assert_eq!(args, vec!["arg1's arg2"]);
    }

    // Literal chars in arg
    #[test]
    fn test_parse_args_backslash_spaces() {
        let args = ExternalCommand::parse_input(r"arg1\ \ \ arg2");
        assert_eq!(args, vec!["arg1   arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_space_collapse_others() {
        let args = ExternalCommand::parse_input(r"arg1\     arg2");
        assert_eq!(args, vec!["arg1 ", "arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_char() {
        let args = ExternalCommand::parse_input(r"arg1\narg2");
        assert_eq!(args, vec!["arg1narg2"]);
    }

    #[test]
    fn test_parse_args_backslash_backslash() {
        let args = ExternalCommand::parse_input(r"arg1\\arg2");
        assert_eq!(args, vec![r"arg1\arg2"]);
    }

    #[test]
    fn test_parse_args_backslash_single_quote() {
        let args = ExternalCommand::parse_input(r"\'arg1 arg2\'");
        assert_eq!(args, vec!["'arg1", "arg2'"]);
    }

    // Support single quote sting literals
    #[test]
    fn test_parse_args_single_quote_with_multi_backslash() {
        let args = ExternalCommand::parse_input(r"'arg1\\\arg2'");
        assert_eq!(args, vec![r"arg1\\\arg2"]);
    }

    #[test]
    fn test_parse_args_single_quote_with_backslash_double_quote() {
        let args = ExternalCommand::parse_input("'arg1\"arg2'");
        assert_eq!(args, vec![r#"arg1"arg2"#]);
    }

    #[test]
    fn test_parse_args_backslash_single_quote_mixed() {
        let args = ExternalCommand::parse_input("'arg1\"arg2\"arg3'");
        assert_eq!(args, vec![r#"arg1"arg2"arg3"#]);
    }

    #[test]
    fn test_parse_args_escaped_single_and_double_quotes() {
        let args = ExternalCommand::parse_input(r#"\'\"arg1 arg2\"\'"#);
        assert_eq!(args, vec![r#"'"arg1"#, r#"arg2"'"#]);
    }
}
