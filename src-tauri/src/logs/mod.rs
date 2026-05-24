//! Log + crash report viewer surface. Enumerates files from
//! `.minecraft/logs/`, `.minecraft/crash-reports/`, and our own
//! `<instance>/logs/` captures, with gzip-aware reading. Pattern-
//! matches known crash signatures via the `diagnose` submodule.

pub mod diagnose;
pub mod files;
pub mod read;
