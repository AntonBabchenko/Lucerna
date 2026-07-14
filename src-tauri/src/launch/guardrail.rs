//! Pure decision helpers for multi-instance launch: the soft RAM warning and
//! the tray hide-on-first rule. Kept side-effect-free so they are unit-tested
//! without real processes (project pattern: `reconcile_running`, `build_argv`).

use serde::Serialize;
use specta::Type;

/// Percent of physical RAM above which launching another instance warns.
pub const RAM_WARN_PERCENT: u8 = 80;

/// Non-blocking pre-launch RAM warning payload (surfaced to the UI, which owns
/// the confirm decision). Megabytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RamWarning {
    pub reserved_mb: u32,
    pub total_mb: u32,
}

/// Return `Some(RamWarning)` when the sum of already-running `-Xmx` plus the
/// `candidate_mb` about to launch exceeds `pct`% of `total_ram_mb`. `None` when
/// under the threshold or when total RAM is unknown (0). Saturating math so a
/// pathological instance file cannot overflow.
pub fn ram_warning(
    running_heaps_mb: &[u32],
    candidate_mb: u32,
    total_ram_mb: u64,
    pct: u8,
) -> Option<RamWarning> {
    if total_ram_mb == 0 {
        return None;
    }
    let reserved: u64 = running_heaps_mb
        .iter()
        .map(|&m| u64::from(m))
        .sum::<u64>()
        .saturating_add(u64::from(candidate_mb));
    let threshold = total_ram_mb.saturating_mul(u64::from(pct)) / 100;
    if reserved > threshold {
        Some(RamWarning {
            reserved_mb: reserved.min(u64::from(u32::MAX)) as u32,
            total_mb: total_ram_mb.min(u64::from(u32::MAX)) as u32,
        })
    } else {
        None
    }
}

/// Hide the launcher to tray on launch only when the user opted in AND this is
/// the FIRST running instance (no instance was running before this one).
pub fn should_hide_on_launch(opted_in: bool, was_any_running_before: bool) -> bool {
    opted_in && !was_any_running_before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_warning_under_threshold() {
        // 2048 + 2048 = 4096 <= 80% of 16384 (13107).
        assert_eq!(ram_warning(&[2048], 2048, 16384, 80), None);
    }

    #[test]
    fn warns_over_threshold() {
        // 8192 + 6144 = 14336 > 80% of 16384 (13107).
        assert_eq!(
            ram_warning(&[8192], 6144, 16384, 80),
            Some(RamWarning {
                reserved_mb: 14336,
                total_mb: 16384
            })
        );
    }

    #[test]
    fn first_launch_counts_only_candidate() {
        // Nothing running: 12288 <= 13107 → no warn; 14000 > 13107 → warn.
        assert_eq!(ram_warning(&[], 12288, 16384, 80), None);
        assert!(ram_warning(&[], 14000, 16384, 80).is_some());
    }

    #[test]
    fn unknown_total_ram_never_warns() {
        assert_eq!(ram_warning(&[8192, 8192], 8192, 0, 80), None);
    }

    #[test]
    fn tray_hides_only_on_first_when_opted_in() {
        assert!(should_hide_on_launch(true, false));
        assert!(!should_hide_on_launch(true, true));
        assert!(!should_hide_on_launch(false, false));
    }
}
