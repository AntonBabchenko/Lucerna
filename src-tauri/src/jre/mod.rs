//! Java Runtime Environment downloader.

pub mod install;
pub mod manifest;
pub mod unavailable;

pub(crate) use install::marker_path;
pub use install::{ensure_jre, java_executable_path, DEFAULT_LEGACY_COMPONENT};
pub use unavailable::unavailable;
