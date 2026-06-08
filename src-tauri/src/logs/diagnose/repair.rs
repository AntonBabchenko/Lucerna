//! Auto-Repair: turns a diagnoser hit into a typed, confirmable fix.
//! Pure logic only — no I/O, no network. The command layer in
//! `commands.rs` orchestrates instance state + platform calls around
//! these helpers.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Static tag attached to a `Diagnosis` so the UI knows whether to
/// offer a Fix button. Membership in the actionable set is the ONLY
/// input — no instance I/O. Real precondition gating happens later in
/// `build_repair_plan`.
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairKind {
    RaiseHeap,
    ReinstallLoader,
    RedownloadMod,
    ResolveConflict,
}

/// Map a diagnoser `pattern_id` to its repair kind, or `None` for the
/// advisory-only patterns (`java-version-too-old`, `port-already-in-use`,
/// `disk-full`).
pub fn repair_kind_for(pattern_id: &str) -> Option<RepairKind> {
    match pattern_id {
        "out-of-memory" => Some(RepairKind::RaiseHeap),
        "fabric-loader-missing-main" => Some(RepairKind::ReinstallLoader),
        "corrupt-mod-jar" => Some(RepairKind::RedownloadMod),
        "mod-resolution-conflict" => Some(RepairKind::ResolveConflict),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_kind_maps_actionable_patterns() {
        assert_eq!(repair_kind_for("out-of-memory"), Some(RepairKind::RaiseHeap));
        assert_eq!(
            repair_kind_for("fabric-loader-missing-main"),
            Some(RepairKind::ReinstallLoader)
        );
        assert_eq!(
            repair_kind_for("corrupt-mod-jar"),
            Some(RepairKind::RedownloadMod)
        );
        assert_eq!(
            repair_kind_for("mod-resolution-conflict"),
            Some(RepairKind::ResolveConflict)
        );
    }

    #[test]
    fn repair_kind_none_for_advisory_patterns() {
        assert_eq!(repair_kind_for("java-version-too-old"), None);
        assert_eq!(repair_kind_for("port-already-in-use"), None);
        assert_eq!(repair_kind_for("disk-full"), None);
        assert_eq!(repair_kind_for("nonexistent"), None);
    }
}