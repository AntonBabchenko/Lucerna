//! command_history.txt (spec §B5): the lines typed since the baseline travel;
//! the shared history keeps the last 50, without an immediate repeat.

use std::path::Path;

use crate::error::{Error, Result};

/// The game keeps the last 50 commands.
pub(crate) const CAP: usize = 50;

/// The line separator a file uses, kept when Lucerna rewrites it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sep {
    Lf,
    CrLf,
}

impl Sep {
    pub(crate) fn platform() -> Self {
        if cfg!(windows) {
            Sep::CrLf
        } else {
            Sep::Lf
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Sep::Lf => "\n",
            Sep::CrLf => "\r\n",
        }
    }
}

/// The instance's history and its separator; `None` when there is no file.
pub(crate) fn read(path: &Path) -> Result<Option<(Vec<String>, Sep)>> {
    let Some(bytes) = super::fsio::read_if_present(path)? else {
        return Ok(None);
    };
    let text = String::from_utf8(bytes)
        .map_err(|e| Error::io(path.display().to_string(), format!("not UTF-8: {e}")))?;
    Ok(Some(split(&text)))
}

pub(crate) fn split(text: &str) -> (Vec<String>, Sep) {
    let sep = if text.contains("\r\n") {
        Sep::CrLf
    } else {
        Sep::Lf
    };
    let lines = text.lines().map(str::to_string).collect();
    (lines, sep)
}

/// The lines added since `baseline`: what follows the longest overlap of the
/// baseline's tail with `now`'s head (the game trims the front at 50).
pub(crate) fn delta<'a>(baseline: &[String], now: &'a [String]) -> &'a [String] {
    let most = baseline.len().min(now.len());
    let overlap = (0..=most)
        .rev()
        .find(|&k| baseline[baseline.len() - k..] == now[..k])
        .unwrap_or(0);
    &now[overlap..]
}

/// Append `now`'s new lines to `shared`. Re-capturing the same lines (a crash
/// between saving and committing) changes nothing.
pub(crate) fn capture(baseline: &[String], now: &[String], shared: &[String]) -> Vec<String> {
    let added = delta(baseline, now);
    if added.is_empty() || shared.ends_with(added) {
        return shared.to_vec();
    }
    let mut out = shared.to_vec();
    for line in added {
        // The game's own rule: a command typed again right away is kept once.
        if out.last() != Some(line) {
            out.push(line.clone());
        }
    }
    let excess = out.len().saturating_sub(CAP);
    out.drain(..excess);
    out
}

pub(crate) fn render(lines: &[String], sep: Sep) -> Vec<u8> {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push_str(sep.as_str());
    }
    text.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn new_lines_travel() {
        let got = capture(&l(&["/a"]), &l(&["/a", "/b", "/c"]), &l(&["/x"]));
        assert_eq!(got, l(&["/x", "/b", "/c"]));
    }

    #[test]
    fn capturing_the_same_lines_twice_changes_nothing() {
        let shared = l(&["/x", "/b"]);
        assert_eq!(capture(&l(&["/a"]), &l(&["/a", "/b"]), &shared), shared);
    }

    #[test]
    fn the_new_lines_are_found_after_the_game_trimmed_the_front() {
        let base: Vec<String> = (0..50).map(|i| format!("/c{i}")).collect();
        let mut now: Vec<String> = base[2..].to_vec();
        now.push("/new1".into());
        now.push("/new2".into());
        assert_eq!(delta(&base, &now), l(&["/new1", "/new2"]).as_slice());
    }

    #[test]
    fn an_immediate_repeat_is_dropped_and_fifty_are_kept() {
        let shared: Vec<String> = (0..50).map(|i| format!("/s{i}")).collect();
        let got = capture(&[], &l(&["/s49", "/n"]), &shared);
        assert_eq!(got.len(), CAP);
        assert_eq!(got.last().unwrap(), "/n");
        assert_eq!(got[CAP - 2], "/s49", "no second /s49");
    }

    #[test]
    fn the_separator_is_kept() {
        let (lines, sep) = split("/a\r\n/b\r\n");
        assert_eq!((lines.clone(), sep), (l(&["/a", "/b"]), Sep::CrLf));
        assert_eq!(render(&lines, sep), b"/a\r\n/b\r\n");
        assert_eq!(render(&lines, Sep::Lf), b"/a\n/b\n");
    }
}
