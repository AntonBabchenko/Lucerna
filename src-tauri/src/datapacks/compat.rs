//! The datapack format an instance's Minecraft actually expects, and the
//! world DataVersion it would save worlds with — both read from
//! `version.json` bundled INSIDE the client jar.
//!
//! This is the only self-consistent source in the repo: the persisted
//! version JSON this launcher writes to `versions/<id>/<id>.json` keeps the
//! raw upstream JSON on the vanilla path but re-serialises a typed struct on
//! the loader path (Fabric/Forge/...), dropping any field that struct does
//! not model. `pack_version` and `world_version` are not modelled anywhere
//! in `versions::version_json`, so a value read from that file would
//! survive for vanilla and silently vanish for every loader. The client
//! jar's own bundled `version.json`, by contrast, is Mojang's untouched
//! original — present for every install, typed or not.
//!
//! Everything here is best-effort by design, exactly like `pack_meta`: a
//! missing jar (fresh instance, loader switched without re-running Install),
//! an unreadable zip, a jar that predates the entry (the real 1.12.2 client
//! jar has no `version.json` at all), or an unrecognised `pack_version` /
//! `world_version` shape all yield `None`, never an error. This must never
//! block an install, a launch, or a migration plan.

use std::io::Read;
use std::path::{Path, PathBuf};

/// Whether this Minecraft version can load data packs at all.
///
/// Data packs were introduced in **1.13**. On a 1.12.2 instance the entire
/// feature is inert: the library would accept files, the world picker would
/// offer worlds, and the game would read none of it — the "folder that
/// silently does nothing" trap this feature exists to avoid.
///
/// **Unparseable or empty ⟹ `true`.** Uncertainty must not hide the feature:
/// a snapshot id (`26w14a`), an odd loader-synth id, or an instance whose
/// version has not been resolved yet all get the kind rather than losing it.
///
/// Deliberately NOT `mods::local::descriptor_era`, even though that answers
/// the same 1.13 question today. That boundary is the Forge FML descriptor
/// rewrite; this one is the data-pack system. They coincide by accident of
/// history, and coupling them means a future change to one silently moves the
/// other. Only the shared version PARSING is reused.
///
/// 2026 versions (`26.1`) parse to major 26 and are `true`, as they must be.
#[must_use]
pub fn supports_datapacks(mc_version: &str) -> bool {
    let Some(mm) = crate::mods::local::first_major_minor(mc_version) else {
        return true;
    };
    let mut parts = mm.split('.');
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return true;
    };
    match (major.parse::<u32>(), minor.parse::<u32>()) {
        (Ok(1), Ok(m)) => m >= 13,
        // Any major other than 1 is newer than the 1.x line (the 2026 scheme),
        // so it is well past 1.13.
        (Ok(_), Ok(_)) => true,
        _ => true,
    }
}

#[cfg(test)]
mod supports_tests {
    use super::supports_datapacks;

    #[test]
    fn pre_1_13_versions_do_not_support_datapacks() {
        assert!(!supports_datapacks("1.12.2"));
        assert!(!supports_datapacks("1.7.10"));
        assert!(!supports_datapacks("1.9"));
    }

    #[test]
    fn one_thirteen_and_later_do() {
        assert!(supports_datapacks("1.13"));
        assert!(supports_datapacks("1.13.2"));
        assert!(supports_datapacks("1.21.4"));
    }

    #[test]
    fn the_2026_scheme_is_supported() {
        // MC 26.x are real releases, not a typo — lexicographic comparison
        // would put "26.1" before "1.13" and get this exactly backwards.
        assert!(supports_datapacks("26.1"));
    }

    #[test]
    fn an_unknown_version_keeps_the_feature_visible() {
        // Uncertainty must not hide the feature — an instance whose version has
        // not resolved yet still gets the datapack surface.
        assert!(supports_datapacks(""));
        assert!(supports_datapacks("26w14a"));
        assert!(supports_datapacks("garbage"));
    }
}

/// `{versions_dir}/{mc_version}/{mc_version}.jar` — the vanilla client jar.
///
/// Derived from `mc_version`, **never** the effective/synth version id: a
/// Fabric or Quilt instance still runs this exact vanilla jar, and modern
/// Forge/NeoForge omit a client jar from the classpath entirely — see
/// `instances::status::ready_status` and `launch::spawn`, which both resolve
/// the same path the same way.
///
/// `pub(crate)`: besides [`expected_data_format`] below, `l10n::pack_format`
/// resolves the SAME jar (to read its `pack_version` for the resource-pack
/// format rather than the datapack format) and reuses this instead of a
/// second copy of the path formula.
#[must_use]
pub(crate) fn client_jar_path(versions_dir: &Path, mc_version: &str) -> PathBuf {
    versions_dir
        .join(mc_version)
        .join(format!("{mc_version}.jar"))
}

/// The whole `version.json` entry of a client jar, parsed as untyped JSON.
/// The single seam every reader of that entry goes through — `pack_version`
/// ([`data_format_from_archive`]) and `world_version`
/// ([`world_version_of_jar`]) — so the two can never drift on how the entry
/// is located, decoded or parsed. `None` when the entry is absent (a jar
/// older than the entry itself — the 1.12.2 client jar has none), is not
/// UTF-8, or is not JSON. Exactly the three fallible steps the old
/// `data_format_from_archive` performed inline, in the same order.
fn version_json_from_archive<R: Read + std::io::Seek>(
    mut zip: zip::ZipArchive<R>,
) -> Option<serde_json::Value> {
    let mut entry = zip.by_name("version.json").ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

/// Extract the datapack format from a `version.json` entry already read out
/// of a zip archive. Shared by the bytes-based and file-backed entry points
/// below so the two can never drift on how they interpret `pack_version`.
fn data_format_from_archive<R: Read + std::io::Seek>(zip: zip::ZipArchive<R>) -> Option<u32> {
    let v = version_json_from_archive(zip)?;
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

/// Open the vanilla client jar for `mc_version` as a zip archive, reading
/// only its central directory — never the whole jar (~31 MB; `version.json`
/// itself is under a kilobyte). `None` when the file cannot be opened (not
/// installed yet, or unreadable) or is not a zip. Shared by both file-backed
/// readers below so they locate and open the jar identically.
///
/// Fallback discipline, stated: "absent" and "could not open" are
/// deliberately the SAME `None` here. Every caller in this module is
/// best-effort and answers "unknown" for both — the restrictive direction:
/// no format claimed, no version claimed, nothing decided on the user's
/// behalf. A caller whose user-facing text must tell the two apart owes its
/// own `try_exists` probe on [`client_jar_path`]; this helper does not
/// pretend to know.
fn open_client_jar(
    versions_dir: &Path,
    mc_version: &str,
) -> Option<zip::ZipArchive<std::fs::File>> {
    let path = client_jar_path(versions_dir, mc_version);
    let file = std::fs::File::open(&path).ok()?;
    zip::ZipArchive::new(file).ok()
}

/// Read the datapack format out of the client jar at
/// `{versions_dir}/{mc_version}/{mc_version}.jar`.
///
/// File-backed and reads only the `version.json` entry — never the whole
/// jar. Sync because the `zip` crate is sync; a caller on the async IPC
/// thread wraps this in `spawn_blocking`.
///
/// Best-effort like everything else here: a missing, truncated, or
/// non-zip jar yields `None` and never panics — every fallible step, in
/// [`open_client_jar`] and [`version_json_from_archive`], goes through
/// `.ok()?`, not `unwrap`/`expect`.
#[must_use]
pub fn expected_data_format(versions_dir: &Path, mc_version: &str) -> Option<u32> {
    data_format_from_archive(open_client_jar(versions_dir, mc_version)?)
}

/// `world_version` is the world **DataVersion** — the integer Minecraft
/// itself writes to `Data.Version.Id` / `Data.DataVersion` in every
/// `level.dat` it saves, and the only key on which two Minecraft versions
/// can be ordered offline (`worlds::migrate` compares it against the world's
/// own; version *names* are never ordered anywhere in this tree). A bare
/// JSON integer in every jar that carries it: `3465` (1.20.1), `3955`
/// (1.21.1), `4903` (26.2), read from the real client jars on 2026-09-05.
/// Any other shape — a string `"3953"`, a float, a number outside `i32` —
/// is not a DataVersion we recognise: `None`, not a guess. `i32` because
/// that is the NBT `TAG_Int` width `level.dat` stores the same number in.
fn parse_world_version(v: &serde_json::Value) -> Option<i32> {
    i32::try_from(v.as_i64()?).ok()
}

/// What the client jar at `{versions_dir}/{mc_version}/{mc_version}.jar` says
/// about the world **DataVersion** it saves worlds with — the target side of a
/// world migration's version verdict (`Same` / `WillUpgrade` / `WorldIsNewer`
/// compare `Version(n)` against the world's `Data.Version.Id`).
///
/// Three answers, because they lead to three different sentences and a
/// collapsed `Option` would make two of them false (Fallback discipline,
/// question 3): the real 1.12.2 client jar is a valid, installed jar with NO
/// `version.json` entry at all (`version.json` arrived with the 1.14
/// snapshots) — telling that user "install Minecraft 1.12.2 first" would be a
/// false statement about an installed version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JarWorldVersion {
    /// No jar at [`client_jar_path`], or it cannot be opened as a zip: absent
    /// or damaged — "install or repair it first". "Absent" and "could not
    /// tell" are collapsed here on purpose (see [`open_client_jar`]); both
    /// call for the same user action.
    JarUnavailable,
    /// A readable jar that records no integer `world_version`: no
    /// `version.json` (pre-1.14 clients), no such field, or a field of another
    /// shape. The target's DataVersion is unknowable from the jar — say so;
    /// never guess.
    NotRecorded,
    Version(i32),
}

/// Same jar, same entry and same decoding as [`expected_data_format`] — only
/// the field differs. Never an error, never a panic.
///
/// Sync because the `zip` crate is sync; the caller on the async IPC path
/// wraps it in `spawn_blocking` exactly as `commands::datapacks` wraps
/// [`expected_data_format`].
///
/// `pub(crate)`: read by `worlds::migrate::plan`, the first caller, which
/// lands with the migration core. Until it does, the tests below are the
/// only callers, so a non-test build would flag the fn as unused (rustc's
/// dead-code lint spares only fully `pub` items) — hence the allow, which
/// is removed together with the first production caller.
#[must_use]
pub(crate) fn world_version_of_jar(versions_dir: &Path, mc_version: &str) -> JarWorldVersion {
    let Some(zip) = open_client_jar(versions_dir, mc_version) else {
        return JarWorldVersion::JarUnavailable;
    };
    let Some(v) = version_json_from_archive(zip) else {
        return JarWorldVersion::NotRecorded;
    };
    match v.get("world_version").and_then(parse_world_version) {
        Some(n) => JarWorldVersion::Version(n),
        None => JarWorldVersion::NotRecorded,
    }
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

    // ---- world_version_of_jar ----

    #[test]
    fn world_version_of_jar_reads_the_top_level_integer() {
        // The real entry's shape: `world_version` is a bare integer beside
        // `pack_version` (verified on the live 1.20.1 / 1.21.1 / 26.2 jars;
        // 3953 is 1.21's DataVersion).
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.21")).unwrap();
        let jar = jar_with_version_json(
            r#"{"id":"1.21","name":"1.21","world_version":3953,"pack_version":{"resource":34,"data":48}}"#,
        );
        std::fs::write(versions_dir.join("1.21/1.21.jar"), jar).unwrap();

        assert_eq!(
            world_version_of_jar(&versions_dir, "1.21"),
            JarWorldVersion::Version(3953)
        );
    }

    #[test]
    fn world_version_of_jar_is_not_recorded_when_the_field_is_absent() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.20.4")).unwrap();
        let jar = jar_with_version_json(r#"{"id":"1.20.4","pack_version":{"data_major":26}}"#);
        std::fs::write(versions_dir.join("1.20.4/1.20.4.jar"), jar).unwrap();

        assert_eq!(
            world_version_of_jar(&versions_dir, "1.20.4"),
            JarWorldVersion::NotRecorded
        );
    }

    #[test]
    fn world_version_of_jar_is_unavailable_for_a_missing_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        assert_eq!(
            world_version_of_jar(&versions_dir, "1.21"),
            JarWorldVersion::JarUnavailable
        );
    }

    #[test]
    fn world_version_of_jar_is_not_recorded_for_a_string_valued_field() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.21")).unwrap();
        let jar = jar_with_version_json(r#"{"id":"1.21","world_version":"3953"}"#);
        std::fs::write(versions_dir.join("1.21/1.21.jar"), jar).unwrap();

        assert_eq!(
            world_version_of_jar(&versions_dir, "1.21"),
            JarWorldVersion::NotRecorded
        );
    }

    #[test]
    fn world_version_of_jar_is_not_recorded_for_a_jar_without_version_json() {
        // The real 1.12.2 client jar: a valid zip full of classes with no
        // `version.json` entry at all (verified on the live jar).
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.12.2")).unwrap();
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("net/minecraft/client/main/Main.class", opts)
            .unwrap();
        zw.write_all(b"\xCA\xFE\xBA\xBE").unwrap();
        let jar = zw.finish().unwrap().into_inner();
        std::fs::write(versions_dir.join("1.12.2/1.12.2.jar"), jar).unwrap();

        assert_eq!(
            world_version_of_jar(&versions_dir, "1.12.2"),
            JarWorldVersion::NotRecorded
        );
    }

    #[test]
    fn world_version_of_jar_is_unavailable_and_does_not_panic_for_a_truncated_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.21")).unwrap();
        std::fs::write(versions_dir.join("1.21/1.21.jar"), b"PK\x03\x04garbage").unwrap();

        assert_eq!(
            world_version_of_jar(&versions_dir, "1.21"),
            JarWorldVersion::JarUnavailable
        );
    }

    #[test]
    fn parse_world_version_accepts_only_integers_that_fit_i32() {
        use serde_json::json;
        assert_eq!(parse_world_version(&json!(3953)), Some(3953));
        assert_eq!(parse_world_version(&json!(0)), Some(0));
        assert_eq!(parse_world_version(&json!("3953")), None);
        assert_eq!(parse_world_version(&json!(3953.0)), None);
        assert_eq!(parse_world_version(&json!(true)), None);
        assert_eq!(parse_world_version(&json!(null)), None);
        assert_eq!(parse_world_version(&json!({"data": 3953})), None);
        assert_eq!(parse_world_version(&json!(i64::from(i32::MAX) + 1)), None);
        assert_eq!(parse_world_version(&json!(u64::MAX)), None);
    }

    #[test]
    fn both_readers_read_the_same_version_json_entry() {
        // One jar answers both questions: the factored entry reader feeds
        // `expected_data_format` (pack_version) and `world_version_of_jar`
        // (world_version) alike, and neither disturbs the other. The shape
        // is the live 26.2 jar's.
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("26.2")).unwrap();
        let jar = jar_with_version_json(
            r#"{"id":"26.2","world_version":4903,"pack_version":{"resource_major":88,"resource_minor":0,"data_major":107,"data_minor":1}}"#,
        );
        std::fs::write(versions_dir.join("26.2/26.2.jar"), jar).unwrap();

        assert_eq!(expected_data_format(&versions_dir, "26.2"), Some(107));
        assert_eq!(
            world_version_of_jar(&versions_dir, "26.2"),
            JarWorldVersion::Version(4903)
        );
    }
}
