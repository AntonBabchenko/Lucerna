//! Typed progress event for verify + repair. Kept separate from
//! `versions::InstallProgress` so the UI never conflates it with the
//! launch-time "Downloading…" indicator.

use serde::Serialize;
use specta::Type;
use tauri_specta::Event;

use super::VerifyCategory;

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifyPhase {
    Manifest,
    Hashing,
    Repairing,
    Complete,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct VerifyProgress {
    pub instance_id: String,
    pub phase: VerifyPhase,
    pub files_done: u32,
    pub files_total: u32,
    /// Cumulative bytes within the current phase. `f64` (not `u64`) is a
    /// specta/serde-JS quirk shared with `InstallProgress` and playtime.
    pub bytes_done: f64,
    pub current_category: Option<VerifyCategory>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_serializes_snake_case() {
        let json = serde_json::to_string(&VerifyPhase::Hashing).unwrap();
        assert_eq!(json, "\"hashing\"");
    }

    #[test]
    fn progress_round_trips() {
        let p = VerifyProgress {
            instance_id: "i".into(),
            phase: VerifyPhase::Repairing,
            files_done: 2,
            files_total: 4,
            bytes_done: 10.0,
            current_category: Some(crate::verify::VerifyCategory::Assets),
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"repairing\""));
        assert!(s.contains("\"assets\""));
    }
}
