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

use crate::datapacks::format::{FormatVersion, PackSide};

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

/// [`supports_datapacks`] as a refusal, for the commands that WRITE datapacks.
/// Same rule, same "unparseable ⟹ allowed": a wrongly-allowed write is inert
/// data, never data loss.
pub fn require_support(mc_version: &str) -> crate::error::Result<()> {
    if supports_datapacks(mc_version) {
        Ok(())
    } else {
        Err(crate::error::Error::DatapacksUnsupportedVersion {
            mc_version: mc_version.to_string(),
        })
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

    #[test]
    fn require_support_refuses_1_12_2_and_passes_1_13_and_unparseable() {
        use crate::error::Error;
        match super::require_support("1.12.2") {
            Err(Error::DatapacksUnsupportedVersion { mc_version }) => {
                assert_eq!(
                    mc_version, "1.12.2",
                    "the refusal names the version it refused"
                );
            }
            other => panic!("1.12.2 must be refused as DatapacksUnsupportedVersion, got {other:?}"),
        }
        assert!(super::require_support("1.13").is_ok());
        assert!(super::require_support("26.1").is_ok());
        // Uncertainty never refuses: the worst case of a wrong allow is inert data.
        assert!(super::require_support("").is_ok());
        assert!(super::require_support("26w14a").is_ok());
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
/// `pub(crate)`: besides [`game_data_format`] below, `l10n::pack_format`
/// resolves the SAME jar (to read its `pack_version` for the resource-pack
/// format rather than the datapack format) and reuses this instead of a
/// second copy of the path formula.
#[must_use]
pub(crate) fn client_jar_path(versions_dir: &Path, mc_version: &str) -> PathBuf {
    versions_dir
        .join(mc_version)
        .join(format!("{mc_version}.jar"))
}

/// `pack_version` of a client jar's `version.json`, for one side — the ONE
/// reader both [`game_data_format`] (data packs) and `l10n::pack_format`
/// (resource packs) go through, so the two can never drift. Shapes read from
/// the real jars: a bare integer (pre-divergence; both sides share it),
/// `{resource, data}` (≤ 1.21.8, minor 0), and `{resource_major,
/// resource_minor, data_major, data_minor}` (1.21.9+). Precedence is
/// l10n's, kept: `<side>_major` (a missing `<side>_minor` reads as 0), then
/// `<side>`, then a bare integer. Anything else — a string, an array, an
/// object with neither key, a number past `u32` — is `None`, never a guess.
pub(crate) fn pack_version(v: &serde_json::Value, side: PackSide) -> Option<FormatVersion> {
    let (major_key, minor_key, plain_key) = match side {
        PackSide::Resource => ("resource_major", "resource_minor", "resource"),
        PackSide::Data => ("data_major", "data_minor", "data"),
    };
    if let Some(major) = v.get(major_key).and_then(serde_json::Value::as_u64) {
        let minor = v
            .get(minor_key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        return Some(FormatVersion::new(
            u32::try_from(major).ok()?,
            u32::try_from(minor).ok()?,
        ));
    }
    if let Some(major) = v.get(plain_key).and_then(serde_json::Value::as_u64) {
        return Some(FormatVersion::new(u32::try_from(major).ok()?, 0));
    }
    Some(FormatVersion::new(u32::try_from(v.as_u64()?).ok()?, 0))
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
fn data_format_from_archive<R: Read + std::io::Seek>(
    zip: zip::ZipArchive<R>,
) -> Option<FormatVersion> {
    let v = version_json_from_archive(zip)?;
    pack_version(v.get("pack_version")?, PackSide::Data)
}

/// Read the datapack format out of an in-memory client jar. Exists
/// separately from [`game_data_format`] purely for testability — tests
/// build a jar in memory rather than on disk.
///
/// `#[cfg(test)]`, not `pub(crate)`: verified against reality (not assumed)
/// that [`game_data_format`] does NOT call this — it reads the shared
/// `data_format_from_archive` helper directly instead — so this has no
/// production caller at all, only its own unit tests below. Compiling it out
/// of a non-test build is what a `pub(crate)` fn with the same zero callers
/// would not get for free: rustc's dead-code lint only spares fully `pub`
/// items on the assumption they may be used externally, so once this drops
/// below `pub` a normal build would otherwise flag it as unused.
#[cfg(test)]
fn data_format_from_jar_bytes(jar: &[u8]) -> Option<FormatVersion> {
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

/// The data-pack format this instance's Minecraft reports, read from its
/// client jar's own `version.json`: `None` for a missing, unreadable or
/// pre-1.14 jar.
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
pub fn game_data_format(versions_dir: &Path, mc_version: &str) -> Option<FormatVersion> {
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

/// Same jar, same entry and same decoding as [`game_data_format`] — only
/// the field differs. Never an error, never a panic.
///
/// Sync because the `zip` crate is sync; the caller on the async IPC path
/// wraps it in `spawn_blocking` exactly as `commands::datapacks` wraps
/// [`game_data_format`].
///
/// `pub(crate)`: read by `worlds::migrate::plan`, the first caller, which
/// lands with the migration core.
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
    fn game_format_reads_every_real_version_json_shape() {
        // Shapes read with `unzip -p` from the real client jars (spec §1).
        let cases: &[(&str, &str, Option<FormatVersion>)] = &[
            (
                "pre-divergence bare int",
                r#"{"pack_version":6}"#,
                Some(FormatVersion::new(6, 0)),
            ),
            (
                "1.20.1",
                r#"{"pack_version":{"resource":15,"data":15}}"#,
                Some(FormatVersion::new(15, 0)),
            ),
            (
                "1.20.6",
                r#"{"pack_version":{"resource":32,"data":41}}"#,
                Some(FormatVersion::new(41, 0)),
            ),
            (
                "1.21.1",
                r#"{"pack_version":{"resource":34,"data":48}}"#,
                Some(FormatVersion::new(48, 0)),
            ),
            (
                "1.21.11",
                r#"{"pack_version":{"resource_major":75,"resource_minor":0,"data_major":94,"data_minor":1}}"#,
                Some(FormatVersion::new(94, 1)),
            ),
            (
                "26.1.1",
                r#"{"pack_version":{"resource_major":84,"resource_minor":0,"data_major":101,"data_minor":1}}"#,
                Some(FormatVersion::new(101, 1)),
            ),
            (
                "26.2",
                r#"{"pack_version":{"resource_major":88,"resource_minor":0,"data_major":107,"data_minor":1}}"#,
                Some(FormatVersion::new(107, 1)),
            ),
        ];
        for (label, body, want) in cases {
            assert_eq!(
                data_format_from_jar_bytes(&jar_with_version_json(body)),
                *want,
                "{label}"
            );
        }
    }

    #[test]
    fn a_missing_data_minor_reads_as_zero() {
        let v: serde_json::Value = serde_json::from_str(r#"{"data_major":82}"#).unwrap();
        assert_eq!(
            pack_version(&v, PackSide::Data),
            Some(FormatVersion::new(82, 0))
        );
    }

    #[test]
    fn the_resource_side_reads_by_the_rules_l10n_always_used() {
        // Twin of l10n::pack_format's parse tests, runnable in this module:
        // l10n now delegates here, so these pin that delegation locally.
        let read = |s: &str| {
            let v: serde_json::Value = serde_json::from_str(s).unwrap();
            pack_version(&v, PackSide::Resource)
        };
        assert_eq!(
            read(r#"{"resource":22,"data":26}"#),
            Some(FormatVersion::new(22, 0))
        );
        assert_eq!(
            read(r#"{"resource_major":75,"resource_minor":0,"data_major":94,"data_minor":1}"#),
            Some(FormatVersion::new(75, 0))
        );
        assert_eq!(
            read(r#"{"resource_major":65,"data_major":82}"#),
            Some(FormatVersion::new(65, 0))
        );
        assert_eq!(read("6"), Some(FormatVersion::new(6, 0)));
        assert_eq!(read(r#"{"data":26}"#), None);
        assert_eq!(read(r#""22""#), None);
        assert_eq!(read("[22,26]"), None);
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
    fn legacy_integer_form_works() {
        let jar = jar_with_version_json(r#"{"id":"1.16.5","pack_version":6}"#);
        assert_eq!(
            data_format_from_jar_bytes(&jar),
            Some(FormatVersion::new(6, 0))
        );
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
    fn game_data_format_reads_the_file_backed_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.20.4")).unwrap();
        let jar = jar_with_version_json(r#"{"pack_version":{"resource":22,"data":26}}"#);
        std::fs::write(versions_dir.join("1.20.4/1.20.4.jar"), jar).unwrap();

        assert_eq!(
            game_data_format(&versions_dir, "1.20.4"),
            Some(FormatVersion::new(26, 0))
        );
    }

    #[test]
    fn game_data_format_is_none_for_a_missing_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        assert_eq!(game_data_format(&versions_dir, "1.20.4"), None);
    }

    #[test]
    fn game_data_format_is_none_and_does_not_panic_for_a_truncated_jar() {
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("1.20.4")).unwrap();
        // A handful of bytes that are not a valid zip end-of-central-directory.
        std::fs::write(versions_dir.join("1.20.4/1.20.4.jar"), b"PK\x03\x04garbage").unwrap();

        assert_eq!(game_data_format(&versions_dir, "1.20.4"), None);
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
        let jar =
            jar_with_version_json(r#"{"id":"1.20.4","pack_version":{"resource":22,"data":26}}"#);
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
        // `game_data_format` (pack_version) and `world_version_of_jar`
        // (world_version) alike, and neither disturbs the other. The shape
        // is the live 26.2 jar's.
        let td = tempfile::tempdir().unwrap();
        let versions_dir = td.path().join("versions");
        std::fs::create_dir_all(versions_dir.join("26.2")).unwrap();
        let jar = jar_with_version_json(
            r#"{"id":"26.2","world_version":4903,"pack_version":{"resource_major":88,"resource_minor":0,"data_major":107,"data_minor":1}}"#,
        );
        std::fs::write(versions_dir.join("26.2/26.2.jar"), jar).unwrap();

        assert_eq!(
            game_data_format(&versions_dir, "26.2"),
            Some(FormatVersion::new(107, 1))
        );
        assert_eq!(
            world_version_of_jar(&versions_dir, "26.2"),
            JarWorldVersion::Version(4903)
        );
    }
}
