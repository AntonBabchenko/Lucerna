//! Log + crash report viewer surface. Enumerates files from
//! `.minecraft/logs/`, `.minecraft/crash-reports/`, and our own
//! `<instance>/logs/` captures, with gzip-aware reading.

pub mod files;
pub mod read;
