//! Per-instance JVM heap memory policy.
//!
//! Single source of truth for the adaptive memory bounds shown in the UI and
//! the default heap assigned to a new instance. Pure functions over total
//! physical RAM (MB) so they are unit-testable without touching the OS.

/// Slider floor (MB). Matches the historical UI minimum.
const SLIDER_MIN_MB: u32 = 1024;
/// Slider granularity (MB).
const SLIDER_STEP_MB: u32 = 256;
/// Slider ceiling when physical RAM can't be read — preserves the prior 8 GB
/// behavior so the control stays usable on detection failure.
const FALLBACK_MAX_MB: u32 = 8192;
/// Adaptive-default band for a new instance.
const DEFAULT_MIN_MB: u32 = 2048;
const DEFAULT_MAX_MB: u32 = 6144;

/// Round `mb` down to the nearest `SLIDER_STEP_MB`, clamping up to
/// `SLIDER_MIN_MB` if the floored result would be lower.
fn round_to_step(mb: u32) -> u32 {
    let floored = (mb / SLIDER_STEP_MB) * SLIDER_STEP_MB;
    floored.max(SLIDER_MIN_MB)
}

/// Saturating `u64`-MB → `u32`-MB (RAM in MB always fits, but never panic).
fn as_u32_mb(mb: u64) -> u32 {
    mb.min(u32::MAX as u64) as u32
}

/// Slider minimum (MB). Exposed so the command and UI share one constant.
pub fn slider_min_mb() -> u32 {
    SLIDER_MIN_MB
}

/// Slider step (MB). Exposed so the command and UI share one constant.
pub fn slider_step_mb() -> u32 {
    SLIDER_STEP_MB
}

/// Adaptive default max-heap (MB) for a NEW instance: ~40% of physical RAM,
/// clamped to `[DEFAULT_MIN_MB, DEFAULT_MAX_MB]`, then normalized onto the
/// slider grid and range. Unknown RAM → `DEFAULT_MIN_MB`.
pub fn default_heap_mb(total_ram_mb: Option<u64>) -> u32 {
    let band = match total_ram_mb {
        Some(ram) => as_u32_mb(ram * 2 / 5).clamp(DEFAULT_MIN_MB, DEFAULT_MAX_MB),
        None => DEFAULT_MIN_MB,
    };
    // The raw band is neither step-aligned nor bounded by the slider ceiling:
    // on a 1.5 GB machine the band floor (2048) exceeds slider_max_mb (1536),
    // and 40% of 8 GB is 3276, which a 256 MB-step range input cannot represent
    // — the thumb would snap away from the value its own label shows.
    clamp_heap_mb(band, total_ram_mb)
}

/// Round `requested_mb` to the NEAREST `SLIDER_STEP_MB` and clamp it into
/// `[SLIDER_MIN_MB, slider_max_mb]`. IPC is a trust boundary: a heap value
/// arriving from the UI is normalized here and never written to disk raw.
/// Mirrors `clampRound` in `MemorySlider.svelte`, which does the same for live
/// UX only — this is the authoritative one.
pub fn clamp_heap_mb(requested_mb: u32, total_ram_mb: Option<u64>) -> u32 {
    // saturating_add so a hostile u32::MAX cannot wrap around to a tiny heap.
    let stepped =
        (requested_mb.saturating_add(SLIDER_STEP_MB / 2) / SLIDER_STEP_MB) * SLIDER_STEP_MB;
    stepped.clamp(SLIDER_MIN_MB, slider_max_mb(total_ram_mb))
}

/// Heap for a new instance: an explicit request (clamped), else the adaptive
/// default. Kept pure and separate from `create_instance` so the create path's
/// only real decision is unit-testable — `create_instance` needs a live
/// `tauri::AppHandle` and cannot be tested here.
pub fn resolve_heap_mb(requested_mb: Option<u32>, total_ram_mb: Option<u64>) -> u32 {
    match requested_mb {
        Some(mb) => clamp_heap_mb(mb, total_ram_mb),
        None => default_heap_mb(total_ram_mb),
    }
}

/// Slider max (MB) = full physical RAM, rounded down to the step.
/// Unknown RAM → `FALLBACK_MAX_MB`.
pub fn slider_max_mb(total_ram_mb: Option<u64>) -> u32 {
    match total_ram_mb {
        Some(ram) => round_to_step(as_u32_mb(ram)),
        None => FALLBACK_MAX_MB,
    }
}

/// Warning threshold (MB) ≈ 75% of physical RAM, rounded to the step and never
/// above `slider_max_mb`. Unknown RAM → equals `slider_max_mb` (no warning band).
pub fn recommended_max_mb(total_ram_mb: Option<u64>) -> u32 {
    match total_ram_mb {
        Some(ram) => round_to_step(as_u32_mb(ram * 3 / 4)).min(slider_max_mb(total_ram_mb)),
        None => slider_max_mb(total_ram_mb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_heap_clamps_into_band() {
        assert_eq!(default_heap_mb(None), 2048);
        assert_eq!(default_heap_mb(Some(4096)), 2048); // 40% = 1638 → floor 2048
        assert_eq!(default_heap_mb(Some(8192)), 3328); // 40% = 3276 → nearest step 3328
        assert_eq!(default_heap_mb(Some(16384)), 6144); // 40% = 6553 → cap 6144
        assert_eq!(default_heap_mb(Some(32768)), 6144); // 40% = 13107 → cap 6144
    }

    #[test]
    fn slider_max_is_full_ram_or_fallback() {
        assert_eq!(slider_max_mb(None), 8192);
        assert_eq!(slider_max_mb(Some(8192)), 8192);
        assert_eq!(slider_max_mb(Some(16384)), 16384);
        assert_eq!(slider_max_mb(Some(16300)), 16128); // rounded down to step
        assert_eq!(slider_max_mb(Some(512)), 1024); // floored up to min
    }

    #[test]
    fn recommended_is_three_quarters_capped_at_max() {
        assert_eq!(recommended_max_mb(None), 8192); // == fallback max, no band
        assert_eq!(recommended_max_mb(Some(8192)), 6144);
        assert_eq!(recommended_max_mb(Some(16384)), 12288);
        assert_eq!(recommended_max_mb(Some(4096)), 3072);
        // On tiny RAM both collapse to the floor; recommended never exceeds max.
        assert_eq!(recommended_max_mb(Some(512)), 1024);
        assert!(recommended_max_mb(Some(512)) <= slider_max_mb(Some(512)));
    }

    #[test]
    fn clamp_rounds_to_step_and_clamps_into_range() {
        // Below the floor / above the ceiling.
        assert_eq!(clamp_heap_mb(0, Some(16384)), 1024);
        assert_eq!(clamp_heap_mb(999_999, Some(16384)), 16384);
        // Off-step values round to the NEAREST step (matches the UI's clampRound).
        assert_eq!(clamp_heap_mb(3000, Some(16384)), 3072);
        assert_eq!(clamp_heap_mb(3100, Some(16384)), 3072);
        assert_eq!(clamp_heap_mb(3200, Some(16384)), 3328);
        // Unknown RAM falls back to the 8 GB ceiling.
        assert_eq!(clamp_heap_mb(999_999, None), 8192);
        // A hostile value must not wrap around to a tiny heap.
        assert_eq!(clamp_heap_mb(u32::MAX, Some(16384)), 16384);
    }

    #[test]
    fn default_is_step_aligned_and_inside_the_slider_range() {
        for ram in [
            None,
            Some(512),
            Some(1536),
            Some(4096),
            Some(8192),
            Some(16384),
        ] {
            let d = default_heap_mb(ram);
            assert_eq!(
                d % SLIDER_STEP_MB,
                0,
                "default {d} off-step for ram {ram:?}"
            );
            assert!(
                d >= SLIDER_MIN_MB,
                "default {d} below the floor for ram {ram:?}"
            );
            assert!(
                d <= slider_max_mb(ram),
                "default {d} above the slider max for ram {ram:?}"
            );
        }
    }

    #[test]
    fn resolve_uses_the_default_only_when_unspecified() {
        assert_eq!(
            resolve_heap_mb(None, Some(16384)),
            default_heap_mb(Some(16384))
        );
        assert_eq!(resolve_heap_mb(Some(4096), Some(16384)), 4096);
        // An explicit request is clamped, never trusted.
        assert_eq!(resolve_heap_mb(Some(999_999), Some(16384)), 16384);
    }
}
