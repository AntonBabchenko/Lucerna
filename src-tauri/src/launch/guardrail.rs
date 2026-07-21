//! Pure decision helper for multi-instance launch: the soft RAM warning. Kept
//! side-effect-free so it is unit-tested without real processes (project
//! pattern: `reconcile_running`, `build_argv`).

use serde::Serialize;
use specta::Type;

// Distinct from `instances::memory`'s per-instance recommended band (~75%): this is the cross-instance aggregate reserve.
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
    debug_assert!(pct <= 100, "pct must be a percentage 0..=100");
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
    fn exactly_at_threshold_does_not_warn() {
        // threshold = 16384*80/100 = 13107; reserved == threshold must NOT warn
        // (pins the strict `>` against an accidental `>=`).
        assert_eq!(ram_warning(&[13107], 0, 16384, 80), None);
    }

    #[test]
    fn pct_zero_warns_on_any_positive_reserve() {
        // threshold = 0; any reserved > 0 warns, reserved == 0 does not.
        assert!(ram_warning(&[], 1, 16384, 0).is_some());
        assert_eq!(ram_warning(&[], 0, 16384, 0), None);
    }

    #[test]
    fn pct_hundred_threshold_is_full_ram() {
        // threshold = total; exactly-full does not warn, one over does.
        assert_eq!(ram_warning(&[16384], 0, 16384, 100), None);
        assert!(ram_warning(&[16384], 1, 16384, 100).is_some());
    }

    #[test]
    fn reserved_over_u32_max_truncates_saturating() {
        // A single near-u32::MAX heap pushes reserved past u32::MAX; the payload
        // truncates to u32::MAX rather than wrapping. total=1000MB, threshold=800.
        let w = ram_warning(&[u32::MAX], 1000, 1000, 80).expect("must warn");
        assert_eq!(w.reserved_mb, u32::MAX);
        assert_eq!(w.total_mb, 1000);
    }
}
