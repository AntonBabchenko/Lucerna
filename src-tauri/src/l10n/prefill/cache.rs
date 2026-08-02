//! A local, content-addressed cache of model output.
//!
//! The address IS the input: hash(source string, role, target locale, model,
//! prompt version, glossary version). Two consequences worth stating —
//! re-running after a mod update costs only the strings whose ENGLISH
//! changed (the cache is deliberately not keyed on mod version or jar hash),
//! and changing the prompt is a miss rather than a pack that silently mixes
//! two phrasings.
//!
//! One flat JSON map per locale. A pack-sized run is tens of thousands of
//! short entries — a few MB, read once at the start of a run and written
//! incrementally as namespaces complete.

use crate::l10n::prefill::role::UiRole;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub type CacheMap = BTreeMap<String, String>;

/// Everything about the pipeline that can change an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineId {
    pub target_lang: String,
    pub model: String,
    pub prompt_version: u32,
    /// Which vanilla glossary was in play — the Minecraft version it came
    /// from, or "none" when no glossary was available.
    pub glossary_version: String,
}

/// Hex SHA-256 over the tuple, NUL-separated so no two different tuples can
/// concatenate to the same bytes.
#[must_use]
pub fn cache_key(source_en: &str, role: UiRole, id: &PipelineId) -> String {
    let mut h = Sha256::new();
    for part in [
        source_en,
        match role {
            UiRole::Name => "name",
            UiRole::Prose => "prose",
            UiRole::Other => "other",
        },
        &id.target_lang,
        &id.model,
        &id.prompt_version.to_string(),
        &id.glossary_version,
    ] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

/// Read the cache. A missing or malformed file is an empty cache, never an
/// error.
#[must_use]
pub fn load(path: &Path) -> CacheMap {
    std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

/// Write the cache atomically (tmp + rename), matching `l10n::store::save` —
/// including its per-call temp suffix. This path depends only on the target
/// language, so two runs in one process (two instances pre-filling at once,
/// which the run's own concurrency doc says is expected) write the very same
/// file; a temp name shared between them lets one rename a half-written cache
/// into place, which `load` would then silently read as empty.
pub fn save(path: &Path, cache: &CacheMap) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(crate::l10n::store::temp_suffix());
    std::fs::write(
        &tmp,
        // Unreachable: a `BTreeMap<String, String>` has no non-serialisable
        // shape. An empty map is still the right degenerate answer for a
        // cache — the next run simply re-earns the entries.
        serde_json::to_vec(cache).unwrap_or_else(|_| b"{}".to_vec()),
    )?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l10n::prefill::role::UiRole;

    fn id() -> PipelineId {
        PipelineId {
            target_lang: "ru_ru".to_string(),
            model: "claude-x".to_string(),
            prompt_version: 1,
            glossary_version: "1.20.1".to_string(),
        }
    }

    #[test]
    fn the_same_input_hashes_the_same_every_time() {
        assert_eq!(
            cache_key("Energy", UiRole::Name, &id()),
            cache_key("Energy", UiRole::Name, &id())
        );
    }

    #[test]
    fn role_source_model_prompt_and_glossary_all_change_the_key() {
        let base = cache_key("Energy", UiRole::Name, &id());
        assert_ne!(base, cache_key("Energy", UiRole::Prose, &id()));
        assert_ne!(base, cache_key("Power", UiRole::Name, &id()));

        let mut other = id();
        other.model = "claude-y".to_string();
        assert_ne!(base, cache_key("Energy", UiRole::Name, &other));

        let mut other = id();
        other.prompt_version = 2;
        assert_ne!(
            base,
            cache_key("Energy", UiRole::Name, &other),
            "a reworded prompt must be a cache miss, not a silent mix"
        );

        let mut other = id();
        other.glossary_version = "1.21.1".to_string();
        assert_ne!(base, cache_key("Energy", UiRole::Name, &other));
    }

    #[test]
    fn concatenation_cannot_forge_a_collision() {
        // Without a separator, ("ab","c") and ("a","bc") would hash alike.
        // The pair must be two ADJACENT fields in the hashed tuple, in the
        // order they are hashed (target_lang then model) — otherwise the two
        // concatenations differ anyway and the test proves nothing.
        let mut a = id();
        a.target_lang = "ab".to_string();
        a.model = "c".to_string();
        let mut b = id();
        b.target_lang = "a".to_string();
        b.model = "bc".to_string();
        assert_ne!(
            cache_key("X", UiRole::Name, &a),
            cache_key("X", UiRole::Name, &b)
        );
    }

    #[test]
    fn round_trips_through_disk_and_tolerates_a_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cache.json");
        assert!(
            load(&path).is_empty(),
            "a missing cache file is empty, not an error"
        );
        let mut c = load(&path);
        c.insert("k1".to_string(), "Энергия".to_string());
        save(&path, &c).expect("save");
        assert_eq!(load(&path).get("k1").map(String::as_str), Some("Энергия"));
    }

    #[test]
    fn a_corrupt_cache_file_is_treated_as_empty() {
        // A cache is an optimisation; it must never be able to fail a run.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cache.json");
        std::fs::write(&path, b"{not json").expect("write");
        assert!(load(&path).is_empty());
    }
}
