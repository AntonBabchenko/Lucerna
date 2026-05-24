//! Diagnoser knowledge base. Adding a pattern: append a `Pattern`
//! entry to `PATTERNS`, add two unit tests in `engine.rs::tests`
//! (one positive against a real-shaped log excerpt, one negative
//! against an unrelated trace). The first matching entry in
//! `PATTERNS` wins — order by specificity (most specific first).

use crate::logs::files::LogSource;
use once_cell::sync::Lazy;
use regex::Regex;

pub struct Pattern {
    pub id: &'static str,
    pub matcher: Matcher,
    pub title: &'static str,
    pub explanation: &'static str,
    pub recommendation: &'static str,
    pub source_hint: SourceHint,
}

pub enum Matcher {
    /// Case-sensitive substring match. Cheapest; use unless a regex
    /// is genuinely needed for shape or capture.
    Substring(&'static str),
    /// Pre-compiled regex via `once_cell::sync::Lazy`.
    Regex(&'static Lazy<Regex>),
}

impl Matcher {
    /// Returns the byte offset of the first match, or `None`.
    pub fn find(&self, haystack: &str) -> Option<usize> {
        match self {
            Matcher::Substring(needle) => haystack.find(needle),
            Matcher::Regex(re) => re.find(haystack).map(|m| m.start()),
        }
    }
}

/// Which log file kinds make sense for this pattern. The engine
/// uses it as a hint to skip clearly-irrelevant patterns — correctness
/// is unaffected if a hint is wrong, only CPU is wasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceHint {
    Any,
    Crash,
    LauncherStdout,
    GameLog,
}

impl SourceHint {
    pub fn matches(self, src: LogSource) -> bool {
        match self {
            SourceHint::Any => true,
            SourceHint::Crash => matches!(src, LogSource::Crash),
            SourceHint::LauncherStdout => matches!(src, LogSource::Launcher),
            SourceHint::GameLog => matches!(src, LogSource::Game),
        }
    }
}

/// The knowledge base — populated in Task 3.
pub const PATTERNS: &[Pattern] = &[];
