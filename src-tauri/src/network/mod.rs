//! Single chokepoint for outbound HTTP. Every request is funnelled
//! through `client::http()` and logged to `audit`.
//!
//! `use reqwest::*` outside this module is forbidden by `CLAUDE.md`
//! forbidden patterns.

pub mod allowlist;
pub mod audit;
pub mod bytes;
pub mod client;
pub mod download;
pub mod json;
pub mod text;

pub use audit::{audit_violations, clear_for_test as clear_audit_for_test, recent, record, AuditEntry};
pub use bytes::get_bytes;
pub use client::http;
pub use download::{download_with_sha, DownloadProgress};
pub use json::get_json;
pub use text::get_text;
