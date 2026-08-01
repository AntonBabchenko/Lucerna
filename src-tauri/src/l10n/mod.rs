//! In-game mod localization: reading language files out of mod jars, storing
//! user overrides, and emitting them as a sparse resource pack.
//!
//! Deliberately NOT under `src/mods/`: this is not mod management, and
//! `structural_no_inplace_mods_write.rs` matches the bare substring
//! `OpenOptions` anywhere under that tree, so even a read-only open would fail
//! the build there.

pub mod coverage;
pub mod scan;
pub mod validate;
