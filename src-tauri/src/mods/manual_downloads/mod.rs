//! Finishing a pack whose files Lucerna could not download (spec
//! `2026-11-05-blocked-pack-files-dropin-design.md`): which entries are still
//! pending — from the bytes on disk, never a file's name — checking a file the
//! user downloaded against what the pack's source published, and placing it
//! through `mods::store`.

pub mod digest;
pub mod evidence;
pub mod state;
pub mod types;
