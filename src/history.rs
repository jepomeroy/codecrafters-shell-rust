use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::{fs::OpenOptions, path::Path};

use rustyline::history::{FileHistory, History};

pub(crate) struct Helper {
    append_pos: usize,
}

impl Helper {
    pub(crate) fn new() -> Self {
        Self { append_pos: 0 }
    }

    pub(crate) fn append_file(&mut self, path: &str, history: &mut FileHistory) {
        if let Ok(mut file) = OpenOptions::new()
            .append(true)
            .create(true)
            .open(Path::new(path))
        {
            for i in self.append_pos..history.len() {
                let _ = writeln!(file, "{}", history[i]);
            }
        }

        self.append_pos = history.len();
    }

    pub(crate) fn read_file(&mut self, path: &str) -> Result<Vec<String>, anyhow::Error> {
        let mut hist = Vec::new();

        let file = File::open(path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            hist.push(line.to_owned());
        }

        self.append_pos = hist.len();

        Ok(hist)
    }

    pub(crate) fn write_file(&mut self, path: &str, history: &mut FileHistory) {
        self.append_pos = history.len();

        if let Ok(mut file) = OpenOptions::new()
            .append(true)
            .create(true)
            .open(Path::new(path))
        {
            for h in history.iter() {
                let _ = writeln!(file, "{}", h);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::history::History;
    use std::fs;
    use tempfile::NamedTempFile;

    fn make_history(entries: &[&str]) -> FileHistory {
        let mut h = FileHistory::new();
        for e in entries {
            let _ = h.add(e);
        }
        h
    }

    fn read_lines(path: &str) -> Vec<String> {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| l.to_owned())
            .collect()
    }

    #[test]
    fn write_file_writes_all_entries() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        let mut history = make_history(&["echo hello", "ls -la", "pwd"]);
        let mut helper = Helper::new();

        helper.write_file(&path, &mut history);

        let lines = read_lines(&path);
        assert_eq!(lines, vec!["echo hello", "ls -la", "pwd"]);
    }

    #[test]
    fn write_file_updates_append_pos() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        let mut history = make_history(&["a", "b", "c"]);
        let mut helper = Helper::new();

        helper.write_file(&path, &mut history);

        assert_eq!(helper.append_pos, 3);
    }

    #[test]
    fn append_file_writes_only_new_entries() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        let mut history = make_history(&["first", "second"]);
        let mut helper = Helper::new();

        // Simulate first append — writes both entries
        helper.append_file(&path, &mut history);
        assert_eq!(read_lines(&path), vec!["first", "second"]);

        // Add a new entry and append again — only "third" should be appended
        let _ = history.add("third");
        helper.append_file(&path, &mut history);

        assert_eq!(read_lines(&path), vec!["first", "second", "third"]);
    }

    #[test]
    fn append_file_updates_append_pos() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        let mut history = make_history(&["x", "y"]);
        let mut helper = Helper::new();

        helper.append_file(&path, &mut history);
        assert_eq!(helper.append_pos, 2);

        let _ = history.add("z");
        helper.append_file(&path, &mut history);
        assert_eq!(helper.append_pos, 3);
    }

    #[test]
    fn append_file_no_op_when_nothing_new() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        let mut history = make_history(&["only"]);
        let mut helper = Helper::new();

        helper.append_file(&path, &mut history);
        helper.append_file(&path, &mut history); // second call — nothing new

        assert_eq!(read_lines(&path), vec!["only"]);
    }

    #[test]
    fn read_file_returns_all_lines() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();
        let mut helper = Helper::new();

        let result = helper.read_file(&path).unwrap();

        assert_eq!(result, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn read_file_sets_append_pos() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        fs::write(&path, "one\ntwo\n").unwrap();
        let mut helper = Helper::new();

        helper.read_file(&path).unwrap();

        assert_eq!(helper.append_pos, 2);
    }

    #[test]
    fn read_file_missing_returns_error() {
        let mut helper = Helper::new();
        assert!(helper.read_file("/tmp/no_such_history_file_xyz").is_err());
    }
}
