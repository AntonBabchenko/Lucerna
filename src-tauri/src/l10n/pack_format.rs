//! The resource pack format an instance's translation overrides must declare
//! in `pack.mcmeta`, read from the instance's own client jar rather than kept
//! as a hardcoded version → format table.
//!
//! A table would rot the moment a new Minecraft version shipped, and rotting
//! silently is not an option here: unlike the datapack format
//! (`datapacks::compat`, which is purely advisory), getting this number wrong
//! is a hard failure for the player, not a cosmetic one.
//!   - Up to resource format 64 (Minecraft 1.21.8), a wrong `pack_format`
//!     lands the pack in an "Incompatible" bucket behind a BLOCKING
//!     confirmation dialog — the player must manually click through it every
//!     time the pack loads.
//!   - From resource format 65 on, the declaration is validated by shape too:
//!     `min_format`/`max_format` replace the legacy `pack_format`, and a bare
//!     `pack_format` at or above 65 is rejected outright — the pack is
//!     reported broken and never loads.
//!
//! So [`PackFormat::UNKNOWN`] exists as a real state, not a placeholder: when
//! this module cannot read the number, [`pack_mcmeta`] refuses to produce a
//! body at all. There is no fallback constant anywhere in this file, on
//! purpose — see the doc on `UNKNOWN` for why no single number could ever be
//! safe on both sides of the format-65 boundary.
//!
//! `version.json` (bundled inside the client jar, at its root — the same path
//! `datapacks::compat`, `forge::installer::{modern,transitional}`, and
//! `servers_runtime::import::detect` all read it from) carries `pack_version`
//! in one of two verified shapes, plus a third, older, inferred one (see
//! below the table):
//!
//! | Era | `pack_version` shape |
//! |---|---|
//! | ≤ 1.21.8 (resource ≤ 64) | `{"resource": N, "data": M}` |
//! | ≥ 25w31a (resource ≥ 65) | `{"resource_major": N, "resource_minor": m, …}` |
//!
//! Verified against real values: 1.20.4 → 22, 1.21.1 → 34 (old scheme);
//! 1.21.11 → 75.0, 26.2 → 88.0 (new scheme).
//!
//! Only the resource numbers are ever read; the data numbers are for
//! datapacks and diverge from the resource ones (as early as 1.18.2, and for
//! good from 1.20.3 onward) — mixing them in would mark an otherwise-correct
//! pack incompatible.
//!
//! The 65 boundary is not an approximation of "roughly where 1.21.9 landed":
//! it is Minecraft's own cutover point. 25w31a — the snapshot immediately
//! after 1.21.8 (resource format 64) — is where `min_format`/`max_format`
//! became required and a bare `pack_format` became conditional on being
//! below it, at resource format exactly 65. 1.21.9 itself ships as 69.0
//! because further snapshots landed in between; every one of them is already
//! on the new scheme. Because Mojang would never ship the new
//! `resource_major`/`resource_minor` JSON shape under an old-scheme format
//! number, checking the number and checking the shape agree on every real
//! jar — [`PackFormat::uses_range_scheme`] checks the number (not shape)
//! specifically so a bare `PackFormat` value built by hand, with no memory of
//! which JSON shape produced it, still renders correctly.
//!
//! A third shape, older still: pre-divergence jars carry `pack_version` as a
//! bare integer (see `datapacks::compat::parse_pack_version`, which reads
//! this same shape for the *data* side). This module reads it too, as the
//! resource format directly — INFERRED, not verified against a real jar
//! (nothing this old was available to check), but the inference is safe for
//! a reason the two-key shapes above are not: a bare integer is *by
//! construction* from the era when resource and data shared one number, so
//! there is only one number to read and no wrong branch to pick. The moment
//! resource and data can disagree, the jar already carries the two-key
//! object instead (confirmed down to 1.20.1, where the object shape appears
//! carrying two EQUAL numbers, `{resource: 15, data: 15}` — proof the object
//! shape predates the values actually diverging, not the other way round).
//! So a bare integer never has to make the resource/data choice this module
//! exists to refuse — refusing here would only exclude real, heavily-played
//! versions (1.16.5 foremost) from a feature whose own floor already claims
//! them. This inference has not been run against an actual sub-1.20.1 client
//! jar; treat it as provisional until it has (see the PR's manual checklist).
//!
//! Applying a translation pack at all is further restricted to resource
//! format 4+ (Minecraft 1.13): below that, FML/Forge iterates mod domains
//! *after* the resource pack stack, so a mod's own lang file silently
//! overwrites the resource pack's override (MinecraftForge issue #4907,
//! closed stale, never fixed). See [`supports_apply`].

use serde::Serialize;
use specta::Type;

use crate::l10n::scan;

/// A resource pack format version. `minor` is 0 for every version before
/// resource format 65 (Minecraft 1.21.9 and its lead-up snapshots), which had
/// no minor component at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackFormat {
    pub major: u32,
    pub minor: u32,
}

impl PackFormat {
    /// `version.json` absent, unreadable, or in a shape this module does not
    /// recognise (malformed JSON, no `pack_version` key, or a `pack_version`
    /// that is none of the three shapes in the module docs). NOT a value
    /// [`pack_mcmeta`] will ever emit: no fallback number is safe on
    /// both sides of the format-65 scheme change — a number chosen for the
    /// old scheme is meaningless post-65, and a number chosen for the new
    /// scheme would render as a legacy `pack_format` the client rejects
    /// outright if it happens to land at or above 65.
    pub const UNKNOWN: Self = Self { major: 0, minor: 0 };

    /// Whether `pack.mcmeta` must use `min_format`/`max_format` instead of a
    /// bare `pack_format`. See the module docs for why 65 is the game's own
    /// boundary (confirmed against 25w31a's own pack.mcmeta documentation),
    /// not a guessed approximation, and why this checks the NUMBER rather
    /// than re-deriving it from which JSON shape a value happened to be
    /// parsed from: a `PackFormat` is a plain value, and two values with the
    /// same fields must render identically regardless of provenance — the
    /// tests below construct several by hand and expect exactly that.
    fn uses_range_scheme(&self) -> bool {
        self.major >= 65
    }
}

/// Whether a translation pack can be applied to this Minecraft version at
/// all. Format 4 is 1.13 — see the module docs for why FML/Forge below that
/// silently loses the override (MinecraftForge #4907).
///
/// The `fmt != PackFormat::UNKNOWN` half is redundant with `major >= 4` today
/// — `UNKNOWN.major` is 0, which already fails that bound — but it is kept
/// because it costs one cheap comparison and makes the intent readable
/// without the reader having to know `UNKNOWN`'s exact bit pattern: this
/// function excludes the sentinel on purpose, not as a side effect of a
/// numeric threshold that happens to also exclude it.
pub fn supports_apply(fmt: PackFormat) -> bool {
    fmt != PackFormat::UNKNOWN && fmt.major >= 4
}

/// Parse `version.json` from a client jar. Recognises all three shapes
/// described in the module docs — the two two-key objects and the older
/// bare-integer form — and always prefers the resource numbers where there
/// is a choice at all. Any other shape — malformed JSON, no `pack_version`
/// key, a `pack_version` that is a string/array, or a two-key object missing
/// both `resource_major` and `resource` — is unrecognised: `None`, never a
/// guess.
pub fn parse_version_json(body: &[u8]) -> Option<PackFormat> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let pack_version = v.get("pack_version")?;

    if let Some(major) = pack_version.get("resource_major").and_then(|n| n.as_u64()) {
        // ≥ resource format 65: `resource_minor` is absent only if a caller
        // hand-built this JSON — every jar verified against carries it — so
        // defaulting to 0 costs nothing on real input and keeps this branch
        // from rejecting a value it could otherwise read correctly.
        let minor = pack_version
            .get("resource_minor")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);
        return Some(PackFormat {
            major: u32::try_from(major).ok()?,
            minor: u32::try_from(minor).ok()?,
        });
    }

    if let Some(major) = pack_version.get("resource").and_then(|n| n.as_u64()) {
        return Some(PackFormat {
            major: u32::try_from(major).ok()?,
            minor: 0,
        });
    }

    // Pre-divergence jars: `pack_version` is a bare integer that served as
    // BOTH the resource and data format before the two could disagree — see
    // the module docs for why that makes this branch safe despite being
    // unverified against a real jar. `.get(...)` on a bare `Value::Number`
    // already returned `None` for both keys above, so reaching here means
    // `pack_version` is either that bare number or something unrecognised
    // (string/array/object-without-either-key); `.as_u64()` sorts the two.
    let major = pack_version.as_u64()?;
    Some(PackFormat {
        major: u32::try_from(major).ok()?,
        minor: 0,
    })
}

/// Read the format out of an instance's client jar bytes, via `version.json`
/// at the jar root. [`PackFormat::UNKNOWN`] on any failure: missing or
/// unreadable jar, absent `version.json`, or an unrecognised `pack_version`
/// shape — never a panic, never a guess.
pub fn from_client_jar(jar_bytes: &[u8]) -> PackFormat {
    scan::read_entry(jar_bytes, "version.json")
        .and_then(|body| parse_version_json(&body))
        .unwrap_or(PackFormat::UNKNOWN)
}

/// Read the format out of an instance's client jar FILE at `path`, without
/// loading the whole jar into memory — only the central directory and the
/// (tiny) `version.json` entry are read, mirroring
/// `datapacks::compat::expected_data_format` (which reads the same jar for
/// the datapack format). [`PackFormat::UNKNOWN`] on any failure: missing or
/// unreadable file, not a zip, absent `version.json`, or an unrecognised
/// `pack_version` shape — never a panic, never a guess.
///
/// A separate entry point from [`from_client_jar`] (which takes bytes
/// already in memory) rather than a `std::fs::read` + delegate: the client
/// jar is a real Mojang jar (tens of megabytes), and reading it whole just to
/// look at a sub-kilobyte JSON entry would be wasteful on every coverage scan
/// and every Apply.
pub fn from_client_jar_path(path: &std::path::Path) -> PackFormat {
    let Ok(file) = std::fs::File::open(path) else {
        return PackFormat::UNKNOWN;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return PackFormat::UNKNOWN;
    };
    let Ok(mut entry) = zip.by_name("version.json") else {
        return PackFormat::UNKNOWN;
    };
    let mut text = String::new();
    if std::io::Read::read_to_string(&mut entry, &mut text).is_err() {
        return PackFormat::UNKNOWN;
    }
    parse_version_json(text.as_bytes()).unwrap_or(PackFormat::UNKNOWN)
}

/// Whether the override editor's Apply action is available for an instance,
/// and — when it is not — which of the two distinct reasons applies, so the
/// UI can disable the button with an explanation instead of letting the user
/// press it and hit an error. Mirrors [`supports_apply`]'s two-part refusal
/// but names the parts separately: `UnknownFormat` and `TooOld` need
/// different copy ("launch the instance once" vs "this Minecraft version
/// can't load resource-pack overrides at all").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplyGate {
    /// A translation pack can be built and applied.
    Ready,
    /// The client jar is missing, unreadable, or its `version.json` is in a
    /// shape this module does not recognise — most commonly because the
    /// instance has never been launched, so
    /// `versions/<mc_version>/<mc_version>.jar` does not exist yet.
    UnknownFormat,
    /// The format is known but below resource format 4 (Minecraft 1.13):
    /// FML/Forge on these versions loads a mod's own lang file AFTER the
    /// resource pack stack, so an override would silently lose (see the
    /// module docs and MinecraftForge #4907).
    TooOld,
}

/// Classify `fmt` into an [`ApplyGate`]. Built directly on [`supports_apply`]
/// so the two can never disagree about which formats are appliable — this
/// only adds the WHY when they are not.
pub fn apply_gate(fmt: PackFormat) -> ApplyGate {
    if fmt == PackFormat::UNKNOWN {
        ApplyGate::UnknownFormat
    } else if !supports_apply(fmt) {
        ApplyGate::TooOld
    } else {
        ApplyGate::Ready
    }
}

/// Render a `pack.mcmeta` body, or `None` when the format is unknown — the
/// correct answer, not a degraded mode, per the module docs.
///
/// Above the format-65 cutover the legacy `pack_format` field is omitted
/// entirely rather than emitted alongside `min_format`/`max_format`: a bare
/// `pack_format` at or above 65 is rejected by the game, so including one
/// would trade a working pack for a broken one on every version this branch
/// actually targets.
pub fn pack_mcmeta(fmt: PackFormat, description: &str) -> Option<String> {
    if fmt == PackFormat::UNKNOWN {
        return None;
    }
    let body = if fmt.uses_range_scheme() {
        serde_json::json!({
            "pack": {
                "min_format": [fmt.major, fmt.minor],
                "max_format": [fmt.major, fmt.minor],
                "description": description,
            }
        })
    } else {
        serde_json::json!({
            "pack": {
                "pack_format": fmt.major,
                "description": description,
            }
        })
    };
    serde_json::to_string(&body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build an in-memory jar containing a single `version.json` entry at the
    /// root — the layout `from_client_jar` reads.
    fn jar_with_version_json(body: &str) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        zw.start_file("version.json", opts).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
        zw.finish().unwrap().into_inner()
    }

    // -----------------------------------------------------------------
    // parse_version_json
    // -----------------------------------------------------------------

    #[test]
    fn old_scheme_reads_resource_not_data() {
        // resource and data given DIFFERENT numbers so the assertion also
        // proves resource, not data, was the one picked.
        let body = br#"{"id":"1.20.4","pack_version":{"resource":22,"data":26}}"#;
        assert_eq!(
            parse_version_json(body),
            Some(PackFormat {
                major: 22,
                minor: 0
            })
        );
    }

    #[test]
    fn new_scheme_reads_resource_major_minor_not_data_major_minor() {
        // 1.21.11 -> resource 75.0, data 94.1 per the module docs' verified
        // table; again deliberately different so a data-side bug would fail
        // this assertion instead of passing by coincidence.
        let body = br#"{"id":"1.21.11","pack_version":{"resource_major":75,"resource_minor":0,"data_major":94,"data_minor":1}}"#;
        assert_eq!(
            parse_version_json(body),
            Some(PackFormat {
                major: 75,
                minor: 0
            })
        );
    }

    #[test]
    fn new_scheme_defaults_a_missing_resource_minor_to_zero() {
        let body = br#"{"pack_version":{"resource_major":65,"data_major":82}}"#;
        assert_eq!(
            parse_version_json(body),
            Some(PackFormat {
                major: 65,
                minor: 0
            })
        );
    }

    #[test]
    fn missing_pack_version_is_none() {
        assert_eq!(parse_version_json(br#"{"id":"1.20.4"}"#), None);
    }

    #[test]
    fn malformed_json_is_none() {
        assert_eq!(parse_version_json(b"{ not json"), None);
    }

    #[test]
    fn bare_integer_pack_version_is_read_as_the_resource_format() {
        // Pre-divergence jars carry a single shared integer here (same
        // shape `datapacks::compat::parse_pack_version` reads for the DATA
        // side, and the same `"1.16.5"`/`6` pairing that test already uses
        // as precedent). Safe to read directly as the resource format — see
        // the module docs' third paragraph for why a bare integer never
        // presents the resource/data choice this module otherwise refuses
        // to guess at. Inferred, not verified against a real 1.16.5 jar.
        let body = br#"{"id":"1.16.5","pack_version":6}"#;
        assert_eq!(
            parse_version_json(body),
            Some(PackFormat { major: 6, minor: 0 })
        );
    }

    #[test]
    fn bare_integer_pack_version_below_format_4_still_fails_supports_apply() {
        // The 1.13 floor in `supports_apply` is independent of which
        // `pack_version` shape produced the value — a pre-1.13 jar old
        // enough to predate the resource/data split is also old enough to
        // predate FML's mod-domains-last ordering fix, so it must still
        // refuse to apply.
        let body = br#"{"pack_version":3}"#;
        let fmt = parse_version_json(body).expect("bare integer is recognised");
        assert_eq!(fmt, PackFormat { major: 3, minor: 0 });
        assert!(!supports_apply(fmt));
    }

    #[test]
    fn non_object_pack_version_shapes_are_none() {
        assert_eq!(parse_version_json(br#"{"pack_version":"22"}"#), None);
        assert_eq!(parse_version_json(br#"{"pack_version":[22,26]}"#), None);
    }

    #[test]
    fn pack_version_object_missing_both_recognised_keys_is_none() {
        let body = br#"{"pack_version":{"data":26}}"#;
        assert_eq!(parse_version_json(body), None);
    }

    // -----------------------------------------------------------------
    // uses_range_scheme / the format-65 boundary
    // -----------------------------------------------------------------

    #[test]
    fn the_scheme_boundary_sits_exactly_at_format_65() {
        // 1.21.8 (last old-scheme release) vs 25w31a (first new-scheme
        // snapshot, immediately after it) — see module docs.
        assert!(!PackFormat {
            major: 64,
            minor: 0
        }
        .uses_range_scheme());
        assert!(PackFormat {
            major: 65,
            minor: 0
        }
        .uses_range_scheme());
    }

    // -----------------------------------------------------------------
    // pack_mcmeta
    // -----------------------------------------------------------------

    #[test]
    fn old_scheme_mcmeta_emits_a_bare_pack_format_and_no_range_fields() {
        let fmt = PackFormat {
            major: 34,
            minor: 0,
        };
        let raw = pack_mcmeta(fmt, "My Translations").expect("known format");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["pack"]["pack_format"], 34);
        assert_eq!(v["pack"]["description"], "My Translations");
        assert!(v["pack"].get("min_format").is_none());
        assert!(v["pack"].get("max_format").is_none());
    }

    #[test]
    fn new_scheme_mcmeta_emits_min_max_format_and_omits_pack_format() {
        let fmt = PackFormat {
            major: 75,
            minor: 0,
        };
        let raw = pack_mcmeta(fmt, "My Translations").expect("known format");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["pack"]["min_format"], serde_json::json!([75, 0]));
        assert_eq!(v["pack"]["max_format"], serde_json::json!([75, 0]));
        assert_eq!(v["pack"]["description"], "My Translations");
        assert!(v["pack"].get("pack_format").is_none());
    }

    #[test]
    fn new_scheme_mcmeta_carries_a_nonzero_minor_into_both_range_fields() {
        let fmt = PackFormat {
            major: 88,
            minor: 3,
        };
        let raw = pack_mcmeta(fmt, "x").expect("known format");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["pack"]["min_format"], serde_json::json!([88, 3]));
        assert_eq!(v["pack"]["max_format"], serde_json::json!([88, 3]));
    }

    #[test]
    fn unknown_format_mcmeta_is_none() {
        assert_eq!(pack_mcmeta(PackFormat::UNKNOWN, "x"), None);
    }

    // -----------------------------------------------------------------
    // supports_apply
    // -----------------------------------------------------------------

    #[test]
    fn supports_apply_is_false_below_format_4() {
        assert!(!supports_apply(PackFormat { major: 3, minor: 0 }));
    }

    #[test]
    fn supports_apply_is_false_for_unknown() {
        assert!(!supports_apply(PackFormat::UNKNOWN));
    }

    #[test]
    fn supports_apply_is_true_at_format_4() {
        assert!(supports_apply(PackFormat { major: 4, minor: 0 }));
    }

    #[test]
    fn supports_apply_is_true_at_format_34() {
        assert!(supports_apply(PackFormat {
            major: 34,
            minor: 0
        }));
    }

    // -----------------------------------------------------------------
    // from_client_jar
    // -----------------------------------------------------------------

    #[test]
    fn from_client_jar_reads_version_json_at_the_jar_root() {
        let jar =
            jar_with_version_json(r#"{"id":"1.21.1","pack_version":{"resource":34,"data":48}}"#);
        assert_eq!(
            from_client_jar(&jar),
            PackFormat {
                major: 34,
                minor: 0
            }
        );
    }

    #[test]
    fn from_client_jar_is_unknown_for_a_jar_without_version_json() {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        zw.start_file("net/minecraft/Main.class", opts).unwrap();
        zw.write_all(b"\xCA\xFE\xBA\xBE").unwrap();
        let jar = zw.finish().unwrap().into_inner();

        assert_eq!(from_client_jar(&jar), PackFormat::UNKNOWN);
    }

    #[test]
    fn from_client_jar_is_unknown_for_unreadable_bytes() {
        assert_eq!(from_client_jar(b"not a zip"), PackFormat::UNKNOWN);
    }

    // -----------------------------------------------------------------
    // from_client_jar_path
    // -----------------------------------------------------------------

    #[test]
    fn from_client_jar_path_reads_a_file_backed_jar() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("1.21.1.jar");
        let jar =
            jar_with_version_json(r#"{"id":"1.21.1","pack_version":{"resource":34,"data":48}}"#);
        std::fs::write(&path, jar).unwrap();

        assert_eq!(
            from_client_jar_path(&path),
            PackFormat {
                major: 34,
                minor: 0
            }
        );
    }

    #[test]
    fn from_client_jar_path_is_unknown_for_a_missing_file() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            from_client_jar_path(&td.path().join("nope.jar")),
            PackFormat::UNKNOWN
        );
    }

    #[test]
    fn from_client_jar_path_is_unknown_for_a_non_zip_file() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("garbage.jar");
        std::fs::write(&path, b"not a zip at all").unwrap();
        assert_eq!(from_client_jar_path(&path), PackFormat::UNKNOWN);
    }

    #[test]
    fn from_client_jar_path_is_unknown_for_a_jar_without_version_json() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("no-version.jar");
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        zw.start_file("net/minecraft/Main.class", opts).unwrap();
        zw.write_all(b"\xCA\xFE\xBA\xBE").unwrap();
        std::fs::write(&path, zw.finish().unwrap().into_inner()).unwrap();

        assert_eq!(from_client_jar_path(&path), PackFormat::UNKNOWN);
    }

    // -----------------------------------------------------------------
    // apply_gate
    // -----------------------------------------------------------------

    #[test]
    fn apply_gate_is_unknown_format_for_the_unknown_sentinel() {
        assert_eq!(apply_gate(PackFormat::UNKNOWN), ApplyGate::UnknownFormat);
    }

    #[test]
    fn apply_gate_is_too_old_below_format_4() {
        assert_eq!(
            apply_gate(PackFormat { major: 3, minor: 0 }),
            ApplyGate::TooOld
        );
    }

    #[test]
    fn apply_gate_is_ready_at_format_4_and_above() {
        assert_eq!(
            apply_gate(PackFormat { major: 4, minor: 0 }),
            ApplyGate::Ready
        );
        assert_eq!(
            apply_gate(PackFormat {
                major: 75,
                minor: 0
            }),
            ApplyGate::Ready
        );
    }
}
