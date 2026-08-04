//! Server datapack management. Datapacks live in `runtime/<level>/datapacks/`
//! where `<level>` is `level-name` from `server.properties` (default `world`).
//! A datapack ships as a `.zip` with `pack.mcmeta` at its root AND a top-level
//! `data/` tree — see [`crate::datapacks::pack_meta`] for why `pack.mcmeta`
//! alone is not enough (Minecraft also loads unzipped folders, but the launcher
//! only installs zips). Pure-of-network; I/O around a plain directory,
//! mirroring [`super::quarantine`]'s style.

use crate::error::{Error, Result};
use std::io::Read;
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

/// Validate `src_zip` is a datapack and copy it into `dir` (created if absent),
/// returning the installed filename. Rejects an unsafe destination filename and
/// a zip without a root `pack.mcmeta`. Overwrites an existing same-name pack.
pub fn install_datapack(dir: &Path, src_zip: &Path) -> Result<String> {
    let filename = src_zip
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::io("<datapack>", "source path has no filename"))?;
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::io("<datapack>", "invalid filename"));
    }
    if !filename.to_ascii_lowercase().ends_with(".zip") {
        return Err(Error::io("<datapack>", "datapack must be a .zip"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(src_zip)
        .and_then(|mut f| f.read_to_end(&mut bytes).map(|_| ()))
        .map_err(|e| Error::io(src_zip.display().to_string(), e))?;
    if !zip_is_datapack(&bytes) {
        // pack.mcmeta alone isn't the datapack marker (see pack_meta) — name
        // the real kind when that's why it was rejected. The blanket "no
        // pack.mcmeta" wording would be false for a rejected resource pack,
        // which carries one too, and also false for a zip with pack.mcmeta
        // but no data/ or assets/ tree at all (also `Neither`).
        let details = match crate::datapacks::pack_meta::classify(&bytes) {
            crate::datapacks::pack_meta::PackKind::ResourcePack => {
                "this looks like a resource pack, not a datapack"
            }
            crate::datapacks::pack_meta::PackKind::Neither => {
                "not a valid datapack (needs pack.mcmeta and a data/ folder)"
            }
            // Unreachable: this branch only runs when `!zip_is_datapack(&bytes)`,
            // and `zip_is_datapack` is exactly `classify(bytes) == Datapack`.
            // `classify` is a pure function of its byte-slice argument, so a
            // second call on the same `bytes` cannot return `Datapack` here.
            crate::datapacks::pack_meta::PackKind::Datapack => {
                unreachable!("zip_is_datapack already proved classify(&bytes) != Datapack")
            }
        };
        return Err(Error::io("<datapack>", details));
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let dest = dir.join(&filename);
    if !dest.starts_with(dir) {
        return Err(Error::io("<datapack>", "path escapes datapacks dir"));
    }
    std::fs::write(&dest, &bytes).map_err(|e| Error::io(dest.display().to_string(), e))?;
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

    #[test]
    fn install_datapack_writes_validated_zip() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("CoolPack.zip");
        std::fs::write(&src, datapack_zip()).unwrap();
        let dir = td.path().join("world").join("datapacks");
        let name = install_datapack(&dir, &src).unwrap();
        assert_eq!(name, "CoolPack.zip");
        assert!(dir.join("CoolPack.zip").exists());
        assert_eq!(list_datapacks(&dir), vec!["CoolPack.zip".to_string()]);
    }

    #[test]
    fn install_datapack_rejects_non_datapack() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("notapack.zip");
        std::fs::write(&src, zip(&[("readme.txt", b"x")])).unwrap();
        let dir = td.path().join("world").join("datapacks");
        assert!(install_datapack(&dir, &src).is_err());
        assert!(!dir.join("notapack.zip").exists());
    }

    #[test]
    fn install_datapack_rejects_a_resource_pack_with_an_accurate_message() {
        // Regression: this used to say "no pack.mcmeta at the zip root", which
        // is false — a resource pack carries one too.
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
        let dir = td.path().join("world").join("datapacks");
        let msg = install_datapack(&dir, &src).unwrap_err().to_string();
        assert!(msg.contains("resource pack"), "message was: {msg}");
        assert!(!msg.contains("no pack.mcmeta"), "message was: {msg}");
    }

    #[test]
    fn install_datapack_rejects_non_zip_extension() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("pack.jar");
        std::fs::write(&src, datapack_zip()).unwrap();
        let dir = td.path().join("world").join("datapacks");
        assert!(install_datapack(&dir, &src).is_err());
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
