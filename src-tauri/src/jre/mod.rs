//! Java Runtime Environment downloader.

pub mod install;
pub mod manifest;

pub use install::{ensure_jre, java_executable_path, DEFAULT_LEGACY_COMPONENT};
