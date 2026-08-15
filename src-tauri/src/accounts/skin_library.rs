//! Local saved-skins library: a JSON index (`library.json`) plus one PNG per
//! entry under `<dataroot>/skin_library/`. Pure fs functions over a directory
//! path so unit tests run in tempdirs without an AppHandle. No network.

use crate::accounts::microsoft::mc_services::SkinVariant;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinLibraryEntry {
    pub id: String,
    pub name: String,
    pub variant: SkinVariant,
    pub cape_id: Option<String>,
    pub created_at: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SkinLibraryFile {
    entries: Vec<SkinLibraryEntry>,
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("library.json")
}

/// PNG path for an entry id. Ids are UUIDv4 strings this module generated;
/// parse before joining so an id arriving over IPC can never traverse paths.
fn png_path(dir: &Path, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| Error::SkinLibrary {
        details: format!("invalid entry id: {id}"),
    })?;
    Ok(dir.join(format!("{id}.png")))
}

/// Lenient reader for LISTING only: missing or corrupt index reads as
/// empty — the library panel must never hard-fail. Mutations go through
/// `read_index_strict` instead: they rewrite the file, so reading a broken
/// index as empty would silently drop every entry it still holds.
fn read_index(dir: &Path) -> SkinLibraryFile {
    let Ok(bytes) = std::fs::read(index_path(dir)) else {
        return SkinLibraryFile::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Strict reader for the MUTATION paths (`save` / `update` / `delete`).
/// Absent reads as the empty default — a fresh library is a fact. A corrupt
/// or unreadable index is an error: `NotFound` is a fact, every other
/// failure is ignorance (Fallback discipline, Discrimination), and the
/// mutation that would follow rewrites the whole file.
fn read_index_strict(dir: &Path) -> Result<SkinLibraryFile> {
    let path = index_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SkinLibraryFile::default()),
        Err(e) => return Err(Error::io(path.display().to_string(), e)),
    };
    serde_json::from_slice(&bytes).map_err(|e| Error::SkinLibrary {
        details: format!("index is corrupt: {e}"),
    })
}

/// Atomic index write: temp file + rename (same pattern as `accounts/store.rs`).
fn write_index(dir: &Path, file: &SkinLibraryFile) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let path = index_path(dir);
    let json = serde_json::to_string_pretty(file).map_err(|e| Error::SkinLibrary {
        details: format!("serialize index: {e}"),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::io(path.display().to_string(), e));
    }
    Ok(())
}

fn valid_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::SkinLibrary {
            details: "name must not be empty".into(),
        });
    }
    Ok(trimmed.to_string())
}

/// Entries with their PNG payloads (base64), in stored order. Entries whose
/// PNG is missing or unreadable are skipped — tolerant by design.
pub fn list(dir: &Path) -> Vec<(SkinLibraryEntry, String)> {
    use base64::Engine;
    read_index(dir)
        .entries
        .into_iter()
        .filter_map(|e| {
            let path = png_path(dir, &e.id).ok()?;
            let bytes = std::fs::read(path).ok()?;
            Some((e, base64::engine::general_purpose::STANDARD.encode(bytes)))
        })
        .collect()
}

/// Validate and persist a new entry, appended at the end of the index.
///
/// The index is read strictly, and FIRST: a corrupt or unreadable index must
/// fail the save rather than be rebuilt from empty with every previously
/// saved skin dropped — and reading first means a refused save leaves no
/// orphan PNG behind.
pub fn save(
    dir: &Path,
    name: &str,
    variant: SkinVariant,
    cape_id: Option<String>,
    png: &[u8],
    now: f64,
) -> Result<SkinLibraryEntry> {
    let name = valid_name(name)?;
    crate::accounts::cosmetics::validate_skin_png(png)?;
    let mut file = read_index_strict(dir)?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let entry = SkinLibraryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        variant,
        cape_id,
        created_at: now,
    };
    let png_file = png_path(dir, &entry.id)?;
    std::fs::write(&png_file, png).map_err(|e| Error::io(png_file.display().to_string(), e))?;
    file.entries.push(entry.clone());
    if let Err(e) = write_index(dir, &file) {
        // Don't leave an orphan PNG behind if the index write failed.
        let _ = std::fs::remove_file(&png_file);
        return Err(e);
    }
    Ok(entry)
}

/// Update metadata only (name/variant/cape). PNG payloads are immutable —
/// saving an edited canvas is a new entry.
pub fn update(
    dir: &Path,
    id: &str,
    name: &str,
    variant: SkinVariant,
    cape_id: Option<String>,
) -> Result<()> {
    let name = valid_name(name)?;
    let mut file = read_index_strict(dir)?;
    let entry = file
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::SkinLibrary {
            details: format!("entry {id} not found"),
        })?;
    entry.name = name;
    entry.variant = variant;
    entry.cape_id = cape_id;
    write_index(dir, &file)
}

/// Remove an entry and its PNG. Deleting an absent id is a no-op
/// (idempotent) — but only against an index that could actually be read: a
/// corrupt or unreadable index is an error, not an empty library, because
/// answering `Ok` there would report a deletion that never happened.
pub fn delete(dir: &Path, id: &str) -> Result<()> {
    let png_file = png_path(dir, id)?;
    let mut file = read_index_strict(dir)?;
    let before = file.entries.len();
    file.entries.retain(|e| e.id != id);
    if file.entries.len() != before {
        write_index(dir, &file)?;
    }
    let _ = std::fs::remove_file(png_file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::new(w, h);
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    fn save_one(dir: &Path, name: &str) -> SkinLibraryEntry {
        save(dir, name, SkinVariant::Classic, None, &png_of(64, 64), 1.0).unwrap()
    }

    #[test]
    fn save_then_list_round_trips() {
        use base64::Engine;
        let dir = tempfile::tempdir().unwrap();
        let saved = save(
            dir.path(),
            "  Knight  ",
            SkinVariant::Slim,
            Some("cape-1".into()),
            &png_of(64, 64),
            42.0,
        )
        .unwrap();
        assert_eq!(saved.name, "Knight"); // trimmed
        let listed = list(dir.path());
        assert_eq!(listed.len(), 1);
        let (entry, b64) = &listed[0];
        assert_eq!(entry.id, saved.id);
        assert_eq!(entry.name, "Knight");
        assert_eq!(entry.variant, SkinVariant::Slim);
        assert_eq!(entry.cape_id.as_deref(), Some("cape-1"));
        assert_eq!(entry.created_at, 42.0);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        assert_eq!(bytes, png_of(64, 64));
    }

    #[test]
    fn entries_keep_insertion_order() {
        let dir = tempfile::tempdir().unwrap();
        save_one(dir.path(), "first");
        save_one(dir.path(), "second");
        save_one(dir.path(), "third");
        let names: Vec<String> = list(dir.path()).into_iter().map(|(e, _)| e.name).collect();
        assert_eq!(names, ["first", "second", "third"]);
    }

    #[test]
    fn list_skips_entry_with_missing_png() {
        let dir = tempfile::tempdir().unwrap();
        let keep = save_one(dir.path(), "keep");
        let broken = save_one(dir.path(), "broken");
        std::fs::remove_file(dir.path().join(format!("{}.png", broken.id))).unwrap();
        let listed = list(dir.path());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0.id, keep.id);
    }

    #[test]
    fn corrupt_index_lists_empty_but_refuses_mutations() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("library.json"), b"{not json").unwrap();
        // Listing stays lenient — a broken index must never hard-fail the UI.
        assert!(list(dir.path()).is_empty());
        // But save refuses: rebuilding the index from empty and writing it
        // would silently drop every entry the corrupt file still holds.
        let err = save(
            dir.path(),
            "fresh",
            SkinVariant::Classic,
            None,
            &png_of(64, 64),
            1.0,
        )
        .unwrap_err();
        assert!(matches!(err, Error::SkinLibrary { .. }), "got {err:?}");
        // The corrupt file is left byte-identical for manual recovery…
        assert_eq!(
            std::fs::read(dir.path().join("library.json")).unwrap(),
            b"{not json"
        );
        // …and the refused save left no orphan PNG behind.
        let pngs = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "png").unwrap_or(false))
            .count();
        assert_eq!(pngs, 0, "a refused save must not write a PNG");
    }

    #[test]
    fn delete_on_corrupt_index_errors_instead_of_reporting_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("library.json"), b"{not json").unwrap();
        let err = delete(dir.path(), &uuid::Uuid::new_v4().to_string()).unwrap_err();
        assert!(matches!(err, Error::SkinLibrary { .. }), "got {err:?}");
    }

    #[test]
    fn update_on_unreadable_index_surfaces_io_not_a_not_found_story() {
        let dir = tempfile::tempdir().unwrap();
        // A DIRECTORY where the index file is expected: `fs::read` on it
        // fails with a non-NotFound error on every platform. Read-side twin
        // of the poison-the-destination trick in `l10n/prefill/run/state.rs`.
        std::fs::create_dir_all(dir.path().join("library.json")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let err = update(dir.path(), &id, "x", SkinVariant::Classic, None).unwrap_err();
        // Io, not SkinLibrary("entry not found"): the lenient reader used to
        // collapse "could not read" into "empty library" and then report the
        // entry as missing — a plausible, wrong story.
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");
    }

    #[test]
    fn update_edits_metadata_only() {
        let dir = tempfile::tempdir().unwrap();
        let entry = save(
            dir.path(),
            "old",
            SkinVariant::Classic,
            Some("cape-1".into()),
            &png_of(64, 64),
            1.0,
        )
        .unwrap();
        update(dir.path(), &entry.id, " new ", SkinVariant::Slim, None).unwrap();
        let (updated, b64) = &list(dir.path())[0];
        assert_eq!(updated.name, "new");
        assert_eq!(updated.variant, SkinVariant::Slim);
        assert_eq!(updated.cape_id, None);
        assert_eq!(updated.created_at, 1.0);
        assert!(!b64.is_empty()); // PNG untouched
    }

    #[test]
    fn update_unknown_id_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = uuid::Uuid::new_v4().to_string();
        let err = update(dir.path(), &missing, "x", SkinVariant::Classic, None).unwrap_err();
        assert!(matches!(err, Error::SkinLibrary { .. }));
    }

    #[test]
    fn delete_removes_entry_and_png() {
        let dir = tempfile::tempdir().unwrap();
        let entry = save_one(dir.path(), "gone");
        delete(dir.path(), &entry.id).unwrap();
        assert!(list(dir.path()).is_empty());
        assert!(!dir.path().join(format!("{}.png", entry.id)).exists());
    }

    #[test]
    fn delete_absent_id_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        save_one(dir.path(), "stays");
        delete(dir.path(), &uuid::Uuid::new_v4().to_string()).unwrap();
        assert_eq!(list(dir.path()).len(), 1);
    }

    #[test]
    fn save_rejects_blank_name() {
        let dir = tempfile::tempdir().unwrap();
        let err = save(
            dir.path(),
            "   ",
            SkinVariant::Classic,
            None,
            &png_of(64, 64),
            1.0,
        )
        .unwrap_err();
        assert!(matches!(err, Error::SkinLibrary { .. }));
        assert!(list(dir.path()).is_empty());
    }

    #[test]
    fn save_rejects_bad_png() {
        let dir = tempfile::tempdir().unwrap();
        let err = save(
            dir.path(),
            "bad",
            SkinVariant::Classic,
            None,
            &png_of(32, 32),
            1.0,
        )
        .unwrap_err();
        assert!(matches!(err, Error::CosmeticImageInvalid { .. }));
        assert!(list(dir.path()).is_empty());
    }

    #[test]
    fn path_guard_rejects_non_uuid_id() {
        let dir = tempfile::tempdir().unwrap();
        let err = delete(dir.path(), "../../evil").unwrap_err();
        assert!(matches!(err, Error::SkinLibrary { .. }));
    }
}
