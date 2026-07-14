//! Minecraft launch — argument construction, native extraction,
//! process spawn + lifecycle. Multi-instance: run state is keyed by instance id
//! in a shared `ProcessRegistry`.

pub mod args;
pub mod guardrail;
pub mod natives;
pub mod quick_play;
pub mod spawn;

pub use quick_play::QuickPlay;
pub use spawn::{
    is_any_running, is_running, kill_all_running, running_snapshot, start, stop, ProcessExited,
    ProcessSpawned,
};
