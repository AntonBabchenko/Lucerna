//! Single chokepoint for outbound HTTP. Every request is funnelled
//! through `client::http()` after passing through the host allowlist.
//!
//! `use reqwest::*` outside this module is forbidden by `CLAUDE.md`
//! forbidden patterns.
//!
//! [`consent`] is the second, narrower tier: a dial to a host the *user*
//! supplied, gated on an opt-in Settings permission instead of the allowlist
//! (which can only ever list destinations Lucerna itself chooses).

pub mod allowlist;
pub mod bytes;
pub mod client;
pub mod consent;
pub mod download;
pub mod json;
pub mod loopback;
pub mod request;
pub mod text;
pub mod throttle;

pub use bytes::get_bytes;
pub use client::http;
pub use download::{download_with_sha, DownloadProgress};
pub use json::get_json;
pub use text::get_text;
