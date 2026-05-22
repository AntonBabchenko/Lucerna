//! Single chokepoint for outbound HTTP. Every request is funnelled
//! through `client::http()` after passing through the host allowlist.
//!
//! `use reqwest::*` outside this module is forbidden by `CLAUDE.md`
//! forbidden patterns.

pub mod allowlist;
pub mod bytes;
pub mod client;
pub mod download;
pub mod json;
pub mod request;
pub mod text;

pub use bytes::get_bytes;
pub use client::http;
pub use download::{download_with_sha, DownloadProgress};
pub use json::get_json;
pub use text::get_text;
