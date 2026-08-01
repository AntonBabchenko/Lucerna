//! The datapack format an instance's Minecraft actually expects, read from
//! `version.json` bundled INSIDE the client jar.
//!
//! This is the only self-consistent source in the repo: the persisted
//! version JSON this launcher writes to `versions/<id>/<id>.json` keeps the
//! raw upstream JSON on the vanilla path but re-serialises a typed struct on
//! the loader path (Fabric/Forge/...), dropping any field that struct does
//! not model. `pack_version` is not modelled anywhere in
//! `versions::version_json`, so a value read from that file would survive
//! for vanilla and silently vanish for every loader. The client jar's own
//! bundled `version.json`, by contrast, is Mojang's untouched original —
//! present for every install, typed or not.
//!
//! Everything here is best-effort by design, exactly like `pack_meta`: a
//! missing jar (fresh instance, loader switched without re-running Install),
//! an unreadable zip, or an unrecognised `pack_version` shape all yield
//! `None`, never an error. This must never block an install or a launch.

use std::io::Read;
use std::path::{Path, PathBuf};

/// `{versions_dir}/{mc_version}/{mc_version}.jar` — the vanilla client jar.
///
/// Derived from `mc_version`, **never** the effective/synth version id: a
/// Fabric or Quilt instance still runs this exact vanilla jar, and modern
/// Forge/NeoForge omit a client jar from the classpath entirely — see
/// `instances::status::ready_status` and `launch::spawn`, which both resolve
/// the same path the same way.
///
/// Private: the only caller is [`expected_data_format`] below, in this same
/// file; nothing else in the crate or in `tests/datapacks_integration.rs`
/// needs it.
#[must_use]
fn client_jar_path(versions_dir: &Path, mc_version: &str) -> PathBuf {
    versions_dir
        .join(mc_version)
        .join(format!("{mc_version}.jar"))
}

/// Extract the datapack format from a `version.json` entry already read out
/// of a zip archive. Shared by the bytes-based and file-backed entry points
/// below so the two can never drift on how they interpret `pack_version`.
fn data_format_from_archive<R: Read + std::io::Seek>(mut zip: zip::ZipArchive<R>) -> Option<u32> {
    let mut entry = zip.by_name("version.json").ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse_pack_version(v.get("pack_version")?)
}

/// `pack_version` is an object `{resource_major, resource_minor, data_major,
/// data_minor}` in modern jars — the datapack format is `data_major`. Older
/// jars shipped a bare integer instead. Any other shape (string, array,
/// missing `data_major`, out-of-range number) is not a format we recognise —
/// `None`, not a guess.
fn parse_pack_version(v: &serde_json::Value) -> Option<u32> {
    let raw = match v {
        serde_json::Value::Object(_) => v.get("data_major")?.as_u64()?,
        serde_json::Value::Number(_) => v.as_u64()?,
        _ => return None,
    };
    u32::try_from(raw).ok()
}

/// Read the datapack format out of an in-memory client jar. Exists
/// separately from [`expected_data_format`] purely for testability — tests
/// build a jar in memory rather than on disk.
///
/// `#[cfg(test)]`, not `pub(crate)`: verified against reality (not assumed)
/// that [`expected_data_format`] does NOT call this — it reads the shared
/// `data_format_from_archive` helper directly instead — so this has no
/// production caller at all, only its own unit tests below. Compiling it out
/// of a non-test build is what a `pub(crate)` fn with the same zero callers
/// would not get for free: rustc's dead-code lint only spares fully `pub`
/// items on the assumption they may be used externally, so once this drops
/// below `pub` a normal build would otherwise flag it as unused.
#[cfg(test)]
fn data_format_from_jar_bytes(jar: &[u8]) -> Option<u32> {
    let zip = zip::ZipArchive::new(std::io::Cursor::new(jar)).ok()?;
    data_format_from_archive(zip)
}

/// Read the datapack format out of the client jar at
/// `{versions_dir}/{mc_version}/{mc_version}.jar`.
///
/// File-backed and reads only the `version.json` entry — never the whole
/// jar (~31 MB; `version.json` itself is under a kilobyte). Sync because the
/// `zip` crate is sync; a caller on the async IPC thread wraps this in
/// `spawn_blocking`.
///
/// Best-effort like everything else here: a missing, truncated, or
/// non-zip jar yields `None` and never panics — every fallible step below
/// goes through `.ok()?`, not `unwrap`/`expect`.
#[must_use]
pub fn expected_data_format(versions_dir: &Path, mc_version: &str) -> Option<u32> {
    let path = client_jar_path(versions_dir, mc_version);
    let file = std::fs::File::open(&path).ok()?;
    let zip = zip::ZipArchive::new(file).ok()?;
    data_format_from_archive(zip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn jar_with_version_json(body: &str) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("version.json", opts).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn client_jar_path_is_versions_dir_mc_version_mc_version_jar() {
        let dir = Path::new("/inst/.minecraft/versions");
        assert_eq!(
            client_jar_path(dir, "1.20.4"),
            Path::new("/inst/.minecraft/versions/1.20.4/1.20.4.jar")
        );
    }

    #[test]
    fn object_form_yields_data_major() {
        let jar = jar_with_version_json(
            r#"{"id":"1.21.5","pack_version":{"resource_major":34,"resource_minor":0,"data_major":48,"data_minor":0}}"#,
        );
        assert_eq!(data_format_from_jar_bytes(&jar), Some(48));
    }

    #[test]
    fn legacy_integer_form_works() {
        let jar = jar_with_version_json(r#"{"id":"1.16.5","pack_version":6}"#);
        assert_eq!(data_format_from_jar_bytes(&jar), Some(6));
    }

    #[test]
    fn absent_pack_version_is_none() {
        let jar = jar_with_version_json(r#"{"id":"1.20.4"}"#);
        assert_eq!(data_format_from_jar_bytes(&jar), None);
    }

    #[test]
    fn a_jar_without_version_json_is_none() {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("net/minecraft/Main.class", opts).unwrap();
        zw.write_all(b"\xCA\xFE\xBA\xBE").unwrap();
        let jar = zw.finish().unwrap().into_inner();

        assert_eq!(data_format_from_jar_bytes(&jar), None);
    }

    #[test]
    fn garbage_bytes_are_none() {
        assert_eq!(data_format_from_jar_bytes(b"not a zip at all"), None);
    }

    #[test]
    fn a_non_object_non_number_pack_version_is_none() {
        let jar = jar_with_version_json(r#"{"pack_version":"48"}"#);
        assert_eq!(data_format_from_jar_bytes(&jar), None);
    }

    #[test]
    fn expected_data_format_reads_the_file_backed_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.20.4")).unwrap();
        let jar = jar_with_version_json(r#"{"pack_version":{"data_major":26}}"#);
        std::fs::write(versions_dir.join("1.20.4/1.20.4.jar"), jar).unwrap();

        assert_eq!(expected_data_format(&versions_dir, "1.20.4"), Some(26));
    }

    #[test]
    fn expected_data_format_is_none_for_a_missing_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        assert_eq!(expected_data_format(&versions_dir, "1.20.4"), None);
    }

    #[test]
    fn expected_data_format_is_none_and_does_not_panic_for_a_truncated_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.20.4")).unwrap();
        // A handful of bytes that are not a valid zip end-of-central-directory.
        std::fs::write(versions_dir.join("1.20.4/1.20.4.jar"), b"PK\x03\x04garbage").unwrap();

        assert_eq!(expected_data_format(&versions_dir, "1.20.4"), None);
    }
}
