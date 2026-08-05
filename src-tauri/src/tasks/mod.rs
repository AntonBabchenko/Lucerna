//! Per-file provenance and outcome records shared by every task that installs
//! files. Deliberately I/O-free: `reports` persists these, `commands` emit them.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::mods::platform::ModSource;

/// Where a file's bytes came from.
///
/// NOT `ModSource`: that enum is Copy+Hash+Eq, is persisted in four on-disk
/// schemas and keys the exhaustive `platform_for` match, so widening it with
/// archive/local would force meaningless HTTP-client arms. This maps *from* it.
///
/// Named `TaskOrigin`, never `Origin` — `l10n::store::Origin` already owns the
/// short name and the bindings exporter dedupes on the bare Rust type name
/// (the same trap that forced `ModInstallPhase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskOrigin {
    Modrinth,
    Curseforge,
    Ftb,
    Atlauncher,
    /// A file extracted from a modpack/archive rather than fetched from a
    /// named platform (e.g. a pack's `overrides/` entry).
    Archive,
    /// A user-supplied local file, or a source with no report-worthy
    /// platform identity (see the `Hangar` mapping below).
    Local,
    /// A file from the game's own distribution — Mojang's CDN, a loader's
    /// maven, the JRE mirror.
    ///
    /// Deliberately NOT `Local`: that variant means a file the user supplied,
    /// and these are fetched over the network like any other platform's
    /// content. One variant covers all three sources rather than splitting
    /// hairs the report already records per row — the exact host lives in
    /// `TaskDetail::host`.
    ///
    /// No `ModSource` maps here, and none ever should: this origin is produced
    /// only by the version-install pipeline, which knows nothing about mod
    /// platforms.
    Game,
}

impl TaskOrigin {
    /// Every variant. Mirrors `ModSource::ALL` / `AiProvider::ALL` — keeps
    /// `every_mod_source_maps_to_a_task_origin` a real totality check rather
    /// than a tautology, and catches a forgotten arm here the same way those
    /// two catch one in their own call sites.
    pub const ALL: [TaskOrigin; 7] = [
        TaskOrigin::Modrinth,
        TaskOrigin::Curseforge,
        TaskOrigin::Ftb,
        TaskOrigin::Atlauncher,
        TaskOrigin::Archive,
        TaskOrigin::Local,
        TaskOrigin::Game,
    ];
}

impl From<ModSource> for TaskOrigin {
    fn from(source: ModSource) -> Self {
        match source {
            ModSource::Modrinth => TaskOrigin::Modrinth,
            ModSource::Curseforge => TaskOrigin::Curseforge,
            ModSource::Ftb => TaskOrigin::Ftb,
            ModSource::Atlauncher => TaskOrigin::Atlauncher,
            // Hangar serves server plugins, which are out of the install-report
            // scope this type exists for — no `TaskDetail` is ever built for
            // Hangar content today. The mapping still has to be total (this is
            // exactly what `every_mod_source_maps_to_a_task_origin` enforces),
            // so it lands on `Local` rather than being left unmapped.
            ModSource::Hangar => TaskOrigin::Local,
            // Vanilla Tweaks packs are datapacks, which are outside the
            // install-report scope this type exists for — no `TaskDetail` is
            // built for them. Lands on `Local` for the same reason Hangar
            // does: the mapping has to stay total.
            ModSource::VanillaTweaks => TaskOrigin::Local,
        }
    }
}

/// Whether a file's bytes were pulled over the network for this task, or were
/// already sitting in the content-addressed cache from a prior install.
/// Orthogonal to [`crate::mods::store::Placement`] — see [`DetailOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Fetched {
    Downloaded,
    Cached,
}

/// What happened to one file.
///
/// `fetched` and `placement` are ORTHOGONAL. `fetch_to_cache` always ensures the
/// sha-keyed file exists in `mod-cache/` (downloading only on a miss) and
/// `materialize` then hardlinks *from that cache path* either way — so a freshly
/// downloaded jar is ALSO `Linked`. Collapsing them into one union would make
/// "linked" read as "was not downloaded", which is false.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DetailOutcome {
    Installed {
        fetched: Fetched,
        placement: crate::mods::store::Placement,
    },
    /// Destination already held a byte-identical file; no store call was made.
    Unchanged,
    Skipped {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

/// One file's provenance and outcome — a row in a per-file install report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct TaskDetail {
    pub name: String,
    pub install_path: String,
    pub origin: TaskOrigin,
    pub host: Option<String>,
    // f64 not u64: specta forbids exporting BigInt-style types to TS (the
    // f64-not-u64 convention used elsewhere in this crate, e.g.
    // `journal::JournalEntry::at_unix_ms`). 2^53 bytes is far beyond any
    // realistic file size.
    pub bytes: Option<f64>,
    pub sha1: Option<String>,
    pub outcome: DetailOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mod_source_maps_to_a_task_origin() {
        for src in crate::mods::platform::ModSource::ALL {
            let origin: TaskOrigin = src.into();
            assert!(TaskOrigin::ALL.contains(&origin));
        }
    }

    #[test]
    fn origin_serialises_snake_case() {
        let json = serde_json::to_string(&TaskOrigin::Curseforge).unwrap();
        assert_eq!(json, "\"curseforge\"");
    }

    #[test]
    fn game_origin_is_listed_in_all() {
        // `ALL` is a FIXED-SIZE array: adding a variant to the enum does not
        // make it stop compiling, so nothing but this test catches a variant
        // that was added to `TaskOrigin` and forgotten here. Every consumer
        // that iterates origins would silently skip it.
        assert!(
            TaskOrigin::ALL.contains(&TaskOrigin::Game),
            "TaskOrigin::Game must be listed in ALL"
        );
        assert_eq!(TaskOrigin::ALL.len(), 7);
    }

    #[test]
    fn game_origin_serialises_snake_case() {
        let json = serde_json::to_string(&TaskOrigin::Game).unwrap();
        assert_eq!(json, "\"game\"");
    }
}
