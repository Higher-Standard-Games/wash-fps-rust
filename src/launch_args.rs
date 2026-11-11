//! `washoi.args` beside the executable: the command line a plain
//! double-click starts with, for a tester's region, data folder or
//! `--offline` without a script or a shortcut. Read only when the game is
//! started with no arguments, so a command line always wins.
//!
//! One or more arguments per line, split at spaces; double quotes keep a
//! path with spaces in one argument. A line starting with `#` is a comment.
//!
//! ```text
//! # Testing against the US servers, with the data on another drive.
//! --region us
//! --data "D:\Washoi data"
//! ```
//!
//! No TypeScript source: the browser game has no command line.

use std::path::{Path, PathBuf};

/// The file's name, beside the executable.
pub const FILE_NAME: &str = "washoi.args";
/// Larger than any command line; a bigger file is not an args file.
const MAX_LEN: u64 = 64 * 1024;

/// Where the file would be: beside the running executable.
#[must_use]
pub fn path() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(FILE_NAME))
}

/// The arguments in `path`; empty when there is no such file.
#[must_use]
pub fn read(path: &Path) -> Vec<String> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Vec::new();
    };
    if !meta.is_file() || meta.len() > MAX_LEN {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// The arguments in the file's text.
#[must_use]
pub fn parse(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut arg = String::new();
        let mut quoted = false;
        let mut started = false;
        for c in line.chars() {
            match c {
                '"' => {
                    quoted = !quoted;
                    started = true;
                }
                c if c.is_whitespace() && !quoted => {
                    if started {
                        args.push(std::mem::take(&mut arg));
                        started = false;
                    }
                }
                c => {
                    arg.push(c);
                    started = true;
                }
            }
        }
        if started {
            args.push(arg);
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_at_spaces_and_quotes_keep_a_path_whole() {
        let text = "\u{feff}# a comment\n\n--region us\r\n  --data \"D:\\Washoi data\"  \n--offline\n\"\"\n";
        assert_eq!(
            parse(text),
            ["--region", "us", "--data", "D:\\Washoi data", "--offline", ""]
        );
    }

    #[test]
    fn no_file_is_no_arguments() {
        assert!(read(Path::new("/nonexistent/washoi.args")).is_empty());
        assert!(parse("# only comments\n   \n").is_empty());
    }
}
