//! OS-divergent primitives, isolated behind one module so a later macOS
//! spec slots in by adding `cfg` arms here rather than across the codebase.
//!
//! Boundary: subprocess spawning stays in `process::` (the documented
//! chokepoint — Windows `taskkill` is a subprocess and lives there). This
//! module owns the NON-subprocess OS calls (chmod, POSIX signals, Win32
//! window detection) plus the dispatch entry points the launcher calls.
//! Enforced by `tests/structural_platform_chokepoint.rs`.

/// True iff this platform supports in-app self-update (download + verify +
/// launch an installer). Windows-only today; Linux is check-and-notify and
/// macOS lands in a later spec.
pub fn supports_in_app_install() -> bool {
    cfg!(target_os = "windows")
}
