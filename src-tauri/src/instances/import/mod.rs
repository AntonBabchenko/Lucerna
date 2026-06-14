//! Import an existing instance from another launcher into an isolated
//! Lucerna instance. Readers parse a foreign format into `ForeignInstance`;
//! the shared pipeline copies content and recovers mod identities. The
//! source is always read-only.

// Submodules added in later tasks:
// pub mod discovery;  // added in later tasks
// pub mod pipeline;   // added in later tasks
// pub mod readers;    // added in later tasks

pub mod model;

pub use model::{
    ContentCategory, ContentEntry, ForeignInstance, ImportPlan, ImportProgress, KnownMod,
};
