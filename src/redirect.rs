//! Shell output-redirect parsing and file-handle management.

use std::fs::{File, OpenOptions};

use anyhow::anyhow;

/// The destination stream for a shell redirect.
#[derive(Default, Clone)]
pub(crate) enum RedirectType {
    /// Redirect standard output (`>`, `1>`, `>>`, `1>>`).
    #[default]
    StdOut,
    /// Redirect standard error (`2>`, `2>>`).
    StdErr,
}

/// A parsed shell redirect operator and its associated metadata.
#[derive(Default, Clone)]
pub(crate) struct Redirect {
    /// Which stream is being redirected.
    pub(crate) redirect_type: RedirectType,
    /// Index of the redirect operator token in the argument list.
    /// All args before this index are passed to the command.
    pub(crate) position: usize,
    /// Path of the file to write or append to.
    target: String,
    /// `true` for append mode (`>>` / `2>>`), `false` for overwrite (`>` / `2>`).
    append: bool,
}

impl Redirect {
    /// Constructs a `Redirect` from its component parts.
    fn new(redirect_type: RedirectType, position: usize, target: String, append: bool) -> Self {
        Self {
            redirect_type,
            position,
            target,
            append,
        }
    }

    /// Opens (or creates) the redirect target file for writing.
    ///
    /// Returns the open [`File`] handle, or an error if the path cannot be opened.
    /// The file is truncated on overwrite and seeked to the end on append.
    pub(crate) fn get_redirect_file(&self) -> Result<File, anyhow::Error> {
        match OpenOptions::new()
            .append(self.append)
            .truncate(!self.append)
            .create(true)
            .write(true)
            .open(&self.target)
        {
            Ok(f) => Ok(f),
            Err(e) => Err(anyhow!("{e}")),
        }
    }

    /// Scans `args` for the first redirect operator and returns a [`Redirect`] describing it.
    ///
    /// Recognised operators:
    /// - `>` / `1>` — overwrite stdout
    /// - `>>` / `1>>` — append stdout
    /// - `2>` — overwrite stderr
    /// - `2>>` — append stderr
    ///
    /// Returns `None` when no operator is found.
    /// Returns an error when an operator appears without a following filename.
    pub(crate) fn get_redirect(args: &[String]) -> Option<Redirect> {
        for (i, arg) in args.iter().enumerate() {
            match arg.as_str() {
                // overwrite stdout file
                ">" | "1>" => {
                    if let Some(target) = args.get(i + 1) {
                        return Some(Redirect::new(
                            RedirectType::StdOut,
                            i,
                            target.to_owned(),
                            false,
                        ));
                    }
                }
                // append stdout file
                ">>" | "1>>" => {
                    if let Some(target) = args.get(i + 1) {
                        return Some(Redirect::new(
                            RedirectType::StdOut,
                            i,
                            target.to_owned(),
                            true,
                        ));
                    }
                }
                // overwrite stderr file
                "2>" => {
                    if let Some(target) = args.get(i + 1) {
                        return Some(Redirect::new(
                            RedirectType::StdErr,
                            i,
                            target.to_owned(),
                            false,
                        ));
                    }
                }
                // append stderr file
                "2>>" => {
                    if let Some(target) = args.get(i + 1) {
                        return Some(Redirect::new(
                            RedirectType::StdErr,
                            i,
                            target.to_owned(),
                            true,
                        ));
                    }
                }
                _ => continue,
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_get_redirect_none() {
        let r = Redirect::get_redirect(&args(&["arg1", "arg2"]));
        assert!(r.is_none());
    }

    #[test]
    fn test_get_redirect_stdout_overwrite() {
        let r = Redirect::get_redirect(&args(&["arg1", ">", "out.txt"])).unwrap();
        assert!(matches!(r.redirect_type, RedirectType::StdOut));
        assert_eq!(r.target, "out.txt");
        assert_eq!(r.position, 1);
        assert!(!r.append);
    }

    #[test]
    fn test_get_redirect_stdout_overwrite_explicit() {
        let r = Redirect::get_redirect(&args(&["arg1", "1>", "out.txt"])).unwrap();
        assert!(matches!(r.redirect_type, RedirectType::StdOut));
        assert_eq!(r.target, "out.txt");
        assert!(!r.append);
    }

    #[test]
    fn test_get_redirect_stdout_append() {
        let r = Redirect::get_redirect(&args(&["arg1", ">>", "out.txt"])).unwrap();
        assert!(matches!(r.redirect_type, RedirectType::StdOut));
        assert_eq!(r.target, "out.txt");
        assert_eq!(r.position, 1);
        assert!(r.append);
    }

    #[test]
    fn test_get_redirect_stdout_append_explicit() {
        let r = Redirect::get_redirect(&args(&["arg1", "1>>", "out.txt"])).unwrap();
        assert!(matches!(r.redirect_type, RedirectType::StdOut));
        assert!(r.append);
    }

    #[test]
    fn test_get_redirect_stderr_overwrite() {
        let r = Redirect::get_redirect(&args(&["arg1", "2>", "err.txt"])).unwrap();
        assert!(matches!(r.redirect_type, RedirectType::StdErr));
        assert_eq!(r.target, "err.txt");
        assert_eq!(r.position, 1);
        assert!(!r.append);
    }

    #[test]
    fn test_get_redirect_stderr_append() {
        let r = Redirect::get_redirect(&args(&["arg1", "2>>", "err.txt"])).unwrap();
        assert!(matches!(r.redirect_type, RedirectType::StdErr));
        assert_eq!(r.target, "err.txt");
        assert!(r.append);
    }

    #[test]
    fn test_get_redirect_position_at_start() {
        let r = Redirect::get_redirect(&args(&[">", "out.txt"])).unwrap();
        assert_eq!(r.position, 0);
    }

    #[test]
    fn test_get_redirect_position_after_multiple_args() {
        let r = Redirect::get_redirect(&args(&["a", "b", "c", ">", "out.txt"])).unwrap();
        assert_eq!(r.position, 3);
    }

    #[test]
    fn test_get_redirect_first_operator_wins() {
        let r = Redirect::get_redirect(&args(&[">", "first.txt", "2>", "second.txt"])).unwrap();
        assert!(matches!(r.redirect_type, RedirectType::StdOut));
        assert_eq!(r.target, "first.txt");
    }
}
