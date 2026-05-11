use rustyline::{
    Changeset, Context, Helper, Highlighter, Hinter, Validator,
    completion::{Completer, Pair},
    line_buffer::LineBuffer,
};

use crate::builtin::Builtin;

#[derive(Helper, Hinter, Highlighter, Validator)]
pub(crate) struct AutoCompletion;

impl AutoCompletion {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl Completer for AutoCompletion {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let commands = Builtin::builtin_cmds();
        let mut candidates = Vec::new();

        for cmd in commands {
            if cmd.starts_with(line) {
                candidates.push(Pair {
                    display: format!("{} ", cmd.to_owned()),
                    replacement: format!("{} ", cmd.to_owned()),
                });
            }
        }

        Ok((0, candidates))
    }

    fn update(&self, line: &mut LineBuffer, start: usize, elected: &str, cl: &mut Changeset) {
        let end = line.pos();
        line.replace(start..end, elected, cl);
    }
}
