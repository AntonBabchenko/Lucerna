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

/// Round `mb` down to the nearest `SLIDER_STEP_MB`, never below `SLIDER_MIN_MB`.
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
/// clamped to `[DEFAULT_MIN_MB, DEFAULT_MAX_MB]`. Unknown RAM → `DEFAULT_MIN_MB`.
pub fn default_heap_mb(total_ram_mb: Option<u64>) -> u32 {
    match total_ram_mb {
        Some(ram) => as_u32_mb(ram * 2 / 5).clamp(DEFAULT_MIN_MB, DEFAULT_MAX_MB),
        None => DEFAULT_MIN_MB,
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
        assert_eq!(default_heap_mb(Some(8192)), 3276); // 40% = 3276.8 → 3276 in band
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
        // Never exceeds the slider max even on tiny RAM.
        assert!(recommended_max_mb(Some(512)) <= slider_max_mb(Some(512)));
    }
}
