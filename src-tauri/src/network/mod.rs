//! Single chokepoint for outbound HTTP. Every request is funnelled
//! through `client::http()` and logged to `audit`.
//!
//! `use reqwest::*` outside this module is forbidden by `CLAUDE.md`
//! forbidden patterns.

pub mod audit;
pub mod client;
pub mod download;
pub mod json;

pub use audit::{recent, record, AuditEntry};
pub use client::http;
pub use download::{download_with_sha, DownloadProgress};
pub use json::get_json;
