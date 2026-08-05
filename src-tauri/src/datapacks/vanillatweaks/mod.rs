//! Vanilla Tweaks datapack builder.
//!
//! VT is a builder, not a catalogue: you tick packs and the site builds a zip
//! on demand. There are no project ids and no file hashes — but every pack
//! does carry a version string, which is what makes update checking a
//! comparison rather than a blind rebuild. See
//! `docs/superpowers/specs/2026-08-05-vanilla-tweaks-design.md`.

pub mod family;

pub use family::family_for;
