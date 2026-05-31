//! Forge / NeoForge loader support.
//!
//! The Forge installer pipeline is reimplemented in Rust (no
//! `java -jar installer.jar` shell-out). Three installer eras are
//! handled by sibling modules under `installer::`:
//!   - `legacy`        — MC 1.6.x — 1.12.2
//!   - `transitional`  — MC 1.13.x — 1.16.x (Phase 2)
//!   - `modern`        — MC 1.17.x+ (Phase 3)

pub mod flavor;
pub mod installer;
pub mod mappings;
pub mod meta;
pub mod patcher;
pub mod profile;

pub use flavor::ForgeFlavor;
pub use installer::install as install_forge;
