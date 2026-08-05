//! Server datapack management. Datapacks live in `runtime/<level>/datapacks/`
//! where `<level>` is `level-name` from `server.properties` (default `world`).
//! A datapack ships as a `.zip` with `pack.mcmeta` at its root AND a top-level
//! `data/` tree — see [`crate::datapacks::pack_meta`] for why `pack.mcmeta`
//! alone is not enough (Minecraft also loads unzipped folders, but the launcher
//! only installs zips). Pure-of-network; I/O around a plain directory,
//! mirroring [`super::quarantine`]'s style.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

pub mod guard;
pub mod listing;
pub mod mutate;
pub mod sidecar;
pub mod update;

/// Serialises every read-modify-write of a server's `level.dat`.
///
/// Mirrors `datapacks::world_link`'s lock and its rationale: two concurrent
/// commands read-modify-writing the same `level.dat` silently lose one side's
/// edit. One global lock, not per-server — the client precedent is one lock
/// for all instances, contention is nil, and a per-id map is complexity with
/// no demonstrated need.
///
/// Same composition rule, enforced the same way: this is private to the
/// module tree, and only entry points defined here take it. A public function
/// that takes it must never call another public function that also takes it —
/// `tokio::sync::Mutex` is not reentrant, so that deadlocks.
pub(super) fn level_dat_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// `level-name` from raw `server.properties` text, defaulting to `world` when
/// the key is absent or blank (matches the vanilla server default).
pub fn level_name(props_raw: &str) -> String {
    let props = crate::servers_runtime::properties::ServerProperties::parse(props_raw);
    props
        .get("level-name")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("world")
        .to_string()
}

/// `runtime/<level>/` for a server — the world dir, and therefore the dir
/// holding `level.dat` and the datapack provenance sidecar.
///
/// Same traversal guard as [`datapacks_dir`]: `level-name` comes from a file
/// the server process (or an import) wrote, so a crafted `../../escape` must
/// not let `Path::join` walk out of the runtime dir.
pub fn world_dir(runtime: &Path, props_raw: &str) -> PathBuf {
    let name = level_name(props_raw);
    let safe = if crate::servers_runtime::runtime::is_safe_mod_name(&name) {
        name
    } else {
        "world".to_string()
    };
    runtime.join(safe)
}

/// `runtime/<level>/datapacks/` for a server, given its `runtime/` dir and the
/// raw `server.properties` (to honour a custom `level-name`).
///
/// `level-name` is written by the server process / carried in from an imported
/// `server.properties`, so it is not fully trusted: a crafted value like
/// `../../escape` would let `Path::join` walk outside the server's runtime dir.
/// Guard it as a single path segment, falling back to the vanilla default
/// `world` when it isn't.
pub fn datapacks_dir(runtime: &Path, props_raw: &str) -> PathBuf {
    world_dir(runtime, props_raw).join("datapacks")
}

/// A datapack ships as a `.zip` with `pack.mcmeta` at its root AND a top-level
/// `data/` tree. The `pack.mcmeta`-only check this used to do accepted every
/// resource pack; classification now lives in one place.
pub fn zip_is_datapack(bytes: &[u8]) -> bool {
    crate::datapacks::pack_meta::classify(bytes) == crate::datapacks::pack_meta::PackKind::Datapack
}

/// List datapack archive filenames in `dir` (sorted). A missing dir yields an
/// empty list. Only `.zip` entries are reported (folder datapacks are left to
/// the user; the launcher manages the ones it installed).
pub fn list_datapacks(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().ends_with(".zip") {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

/// Validate `src_zip` and install it into the world's `datapacks/`, returning
/// the installed filename. Records a provenance-less sidecar row (so the pack
/// lists with real state) and writes through temp-then-rename.
///
/// `world_dir` is `runtime/<level>/` — the sidecar and `level.dat` live there.
pub async fn install_datapack(world_dir: &Path, src_zip: &Path) -> Result<String> {
    let filename = src_zip
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::io("<datapack>", "source path has no filename"))?;
    let bytes = std::fs::read(src_zip).map_err(|e| Error::io(src_zip.display().to_string(), e))?;
    mutate::install_bytes(world_dir, &filename, &bytes, None).await?;
    Ok(filename)
}

/// Remove a datapack archive from `dir` by name. Idempotent (absent → `Ok`).
/// Rejects unsafe filenames / path escapes.
pub fn remove_datapack(dir: &Path, filename: &str) -> Result<()> {
    if !crate::servers_runtime::runtime::is_safe_mod_name(filename) {
        return Err(Error::io("<datapack>", "invalid filename"));
    }
    let path = dir.join(filename);
    if !path.starts_with(dir) {
        return Err(Error::io("<datapack>", "path escapes datapacks dir"));
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

/// One row of a server world's datapack list.
#[derive(Debug, Clone, serde::Serialize, specta::Type, PartialEq)]
pub struct ServerDatapackEntry {
    /// Three provenances, and only the first is persisted: a real sidecar
    /// row; a row synthesized for a foreign on-disk entry (a hand-dropped
    /// `.zip` is adopted and hashed by `sidecar::reconcile`, a directory is
    /// ephemeral); or a row synthesized from a `level.dat` name alone, for a
    /// ghost whose file is gone. Two of the three carry an empty `sha1` —
    /// which is why the UI keys rows on the filename.
    pub record: crate::servers_runtime::installed::ServerInstalledRecord,
    /// `None` ⟹ `level.dat` exists but could not be read, so enabled-ness is
    /// genuinely unknown rather than guessed. An ABSENT `level.dat` is NOT
    /// this case: a world that has never booted reads as two empty lists,
    /// which is a real answer.
    pub state: Option<crate::datapacks::WorldPackState>,
    /// Something is on disk under this name. Independent of `state`, which
    /// can be `None` for a pack that is plainly present.
    pub present: bool,
    /// The on-disk entry is a directory. Minecraft loads folder packs and
    /// `level.dat` does not distinguish them, so they are listed, toggleable
    /// and removable — but they carry no sha1 and no provenance, so the UI
    /// offers them no update or catalog affordance.
    pub is_folder: bool,
}

/// The result of one server datapack update.
#[derive(Debug, Clone, serde::Serialize, specta::Type, PartialEq)]
pub struct ServerDatapackUpdateOutcome {
    /// The new sidecar row, carrying the target version's provenance.
    pub record: crate::servers_runtime::installed::ServerInstalledRecord,
    /// The enabled state carried across to the new name.
    ///
    /// `None` for a same-filename refresh: that path deliberately never reads
    /// or writes `level.dat`, so it does not KNOW the state — reporting `true`
    /// would tell the admin a disabled pack had been switched on. (Exactly why
    /// the client's `WorldMigration::Refreshed` carries no `was_enabled`.)
    pub was_enabled: Option<bool>,
    /// `false` ⟹ the old file was left on disk because its sha1 did not match
    /// its sidecar row — a hand-replaced pack this must not delete. `true` for
    /// a same-name refresh, where the file was replaced in place and nothing
    /// is left behind.
    pub old_removed: bool,
    /// `false` ⟹ something needs attention; the row is not done.
    pub completed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    fn datapack_zip() -> Vec<u8> {
        zip(&[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":15,"description":"x"}}"#,
            ),
            ("data/ns/tags/foo.json", b"{}"),
        ])
    }

    #[test]
    fn level_name_defaults_to_world() {
        assert_eq!(level_name(""), "world");
        assert_eq!(level_name("motd=hi\n"), "world");
        assert_eq!(level_name("level-name=  \n"), "world");
    }

    #[test]
    fn level_name_honours_custom_value() {
        assert_eq!(level_name("level-name=my_realm\n"), "my_realm");
    }

    #[test]
    fn datapacks_dir_joins_level_and_datapacks() {
        let rt = Path::new("/srv/runtime");
        assert_eq!(
            datapacks_dir(rt, "level-name=foo\n"),
            Path::new("/srv/runtime/foo/datapacks")
        );
        assert_eq!(
            datapacks_dir(rt, ""),
            Path::new("/srv/runtime/world/datapacks")
        );
    }

    #[test]
    fn datapacks_dir_rejects_traversal_level_name() {
        // A crafted level-name must not escape the runtime dir.
        let rt = Path::new("/srv/runtime");
        assert_eq!(
            datapacks_dir(rt, "level-name=../../escape\n"),
            Path::new("/srv/runtime/world/datapacks")
        );
        assert_eq!(
            datapacks_dir(rt, "level-name=a/b\n"),
            Path::new("/srv/runtime/world/datapacks")
        );
    }

    #[test]
    fn zip_is_datapack_accepts_root_pack_mcmeta() {
        assert!(zip_is_datapack(&datapack_zip()));
    }

    #[test]
    fn zip_is_datapack_rejects_non_datapack_zip() {
        let plain = zip(&[("readme.txt", b"hello")]);
        assert!(!zip_is_datapack(&plain));
    }

    #[test]
    fn zip_is_datapack_rejects_nested_pack_mcmeta() {
        // pack.mcmeta under a folder is not a loadable datapack zip.
        let nested = zip(&[("MyPack/pack.mcmeta", b"{}")]);
        assert!(!zip_is_datapack(&nested));
    }

    #[test]
    fn zip_is_datapack_false_on_garbage() {
        assert!(!zip_is_datapack(b"not a zip"));
    }

    #[test]
    fn zip_is_datapack_rejects_a_resource_pack() {
        // Regression: the shipped check accepted any zip with a root pack.mcmeta,
        // which every resource pack has.
        use std::io::Write;
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(br#"{"pack":{"pack_format":34}}"#).unwrap();
        zw.start_file("assets/minecraft/textures/x.png", opts)
            .unwrap();
        zw.write_all(b"\x89PNG").unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        assert!(!zip_is_datapack(&bytes));
    }

    #[tokio::test]
    async fn install_datapack_writes_validated_zip() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("CoolPack.zip");
        std::fs::write(&src, datapack_zip()).unwrap();
        let world = td.path().join("world");
        let name = install_datapack(&world, &src).await.unwrap();
        assert_eq!(name, "CoolPack.zip");
        assert!(world.join("datapacks").join("CoolPack.zip").exists());
        assert_eq!(
            list_datapacks(&world.join("datapacks")),
            vec!["CoolPack.zip".to_string()]
        );
    }

    #[tokio::test]
    async fn install_datapack_rejects_non_datapack() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("notapack.zip");
        std::fs::write(&src, zip(&[("readme.txt", b"x")])).unwrap();
        let world = td.path().join("world");
        assert!(install_datapack(&world, &src).await.is_err());
        assert!(!world.join("datapacks").join("notapack.zip").exists());
    }

    #[tokio::test]
    async fn install_datapack_rejects_a_resource_pack_with_an_accurate_message() {
        // Regression intent unchanged from the original: the rejection reason
        // must accurately identify a resource pack, never the false "no
        // pack.mcmeta at the zip root" the shipped code used to say (every
        // resource pack carries one too). The MECHANISM changed: the new path
        // returns a typed `Error::DatapackInvalid { reason }` whose
        // `#[error(...)]` Display is deliberately generic
        // ("{filename} is not a usable datapack") — see `DatapackRejection`'s
        // own doc comment, the reason is typed specifically so the UI can
        // localise it, and a hand-written English sentence baked into the
        // Display would reach a Russian user untranslated. So the accuracy
        // check now reads the TYPED reason rather than grepping the rendered
        // string — matches every other typed-error assertion in this crate.
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("Faithful.zip");
        std::fs::write(
            &src,
            zip(&[
                ("pack.mcmeta", br#"{"pack":{"pack_format":15}}"#),
                ("assets/minecraft/textures/x.png", b"\x89PNG"),
            ]),
        )
        .unwrap();
        let world = td.path().join("world");
        let err = install_datapack(&world, &src).await.unwrap_err();
        assert!(
            matches!(
                err,
                Error::DatapackInvalid {
                    reason: crate::error::DatapackRejection::IsAResourcePack,
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn install_datapack_rejects_non_zip_extension() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("pack.jar");
        std::fs::write(&src, datapack_zip()).unwrap();
        let world = td.path().join("world");
        assert!(install_datapack(&world, &src).await.is_err());
    }

    #[test]
    fn remove_datapack_is_idempotent_and_safe() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        std::fs::write(dir.join("p.zip"), datapack_zip()).unwrap();
        remove_datapack(dir, "p.zip").unwrap();
        assert!(!dir.join("p.zip").exists());
        // absent → Ok
        remove_datapack(dir, "p.zip").unwrap();
        // traversal rejected
        assert!(remove_datapack(dir, "../escape.zip").is_err());
    }

    #[test]
    fn list_datapacks_empty_on_missing_dir() {
        let td = tempfile::tempdir().unwrap();
        assert!(list_datapacks(&td.path().join("nope")).is_empty());
    }
}
