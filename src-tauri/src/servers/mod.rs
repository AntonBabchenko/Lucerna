//! Per-instance saved multiplayer servers, backed by the instance's own
//! `<instance>/.minecraft/servers.dat` (uncompressed NBT). Local-file-only;
//! this module makes no network calls. The connect action itself reuses the
//! existing Quick Play launch path — this module only reads/edits the list.

pub mod nbt;

use serde::Serialize;
use specta::Type;

/// One saved server, surfaced to the UI. `address` mirrors the NBT `ip` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SavedServer {
    pub name: String,
    pub address: String,
}
