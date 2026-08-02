//! Stage 3 of mod localization: fill missing translations with a model.
//!
//! The contract is the verifier, not the model. Every candidate string is
//! checked by `prefill::verify` before it can be written, and a string that
//! fails twice is simply not written — a resource pack merges per key, so an
//! absent key falls back to the mod's own English at zero cost. That makes
//! "never write a string you cannot verify" a free guarantee rather than a
//! trade-off.

pub mod cache;
pub mod cancel;
pub mod estimate;
pub mod glossary;
pub mod plan;
pub mod prompt;
pub mod provider;
pub mod role;
pub mod run;
pub mod verify;
