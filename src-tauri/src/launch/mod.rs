//! Minecraft launch — argument construction, native extraction,
//! process spawn + lifecycle. Single-instance for v0.1.0.

pub mod args;
pub mod natives;
pub mod quick_play;
pub mod spawn;

pub use quick_play::QuickPlay;
pub use spawn::{is_running, start, stop, ProcessExited, ProcessSpawned};
