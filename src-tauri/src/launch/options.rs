//! Per-launch choices the page makes after a settings-sync question (spec
//! 2026-10-15 §B3): the same launch is re-run with one of these set.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct LaunchOptions {
    /// This launch only: no settings sync before it or after it; the next
    /// sync point captures what this session changed.
    #[serde(default)]
    pub skip_settings_sync: bool,
    /// The user said a game Lucerna lost track of (a launcher restart) is
    /// closed: its marker is removed and the sync goes ahead.
    #[serde(default)]
    pub game_closed_confirmed: bool,
}
