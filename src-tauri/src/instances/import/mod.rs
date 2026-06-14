//! Import an existing instance from another launcher into an isolated
//! Lucerna instance. Readers parse a foreign format into `ForeignInstance`;
//! the shared pipeline copies content and recovers mod identities. The
//! source is always read-only.

pub mod discovery;
pub mod model;
pub mod pipeline;
pub mod readers;

pub use model::{
    build_import_plan, scan_content, ContentCategory, ContentEntry, ForeignInstance, ImportPlan,
    ImportProgress, KnownMod,
};
