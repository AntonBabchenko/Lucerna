//! In-game mod localization: reading language files out of mod jars, storing
//! user overrides, and emitting them as a sparse resource pack.
//!
//! Deliberately NOT under `src/mods/`: this is not mod management, and
//! `structural_no_inplace_mods_write.rs` matches the bare substring
//! `OpenOptions` anywhere under that tree, so even a read-only open would fail
//! the build there.
//!
//! This tree is also deliberately NOT one of that guard's own scanned roots —
//! none of `l10n`'s writes are hardlink-bearing. See that file's module doc
//! ("`src/l10n/` was evaluated as a candidate fourth tree...") for the full
//! per-write accounting before adding one here.

pub mod coverage;
pub mod namespace_scan;
pub mod options_txt;
pub mod pack;
pub mod pack_format;
pub mod scan;
pub mod store;
pub mod validate;
