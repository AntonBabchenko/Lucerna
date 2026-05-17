//! Forge mappings layer — TSRG v1 parser + `ObfIndex` lookup.
//!
//! SpecialSource (Tasks 13-17) consumes an `ObfIndex` to remap classes
//! / methods / fields. TSRG v1 is the format Forge 1.13-1.16 ships
//! (via the mcp_config artifact, extracted by installertools MCP_DATA).
//! TSRG v2 + PROGUARD live in Phase 3 because they're modern-era only.

pub mod obf;
pub mod srg;

pub use obf::ObfIndex;
pub use srg::load_tsrg_v1;
