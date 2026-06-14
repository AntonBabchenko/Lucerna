//! Local mod-jar install: read a `.jar`'s mod descriptor, judge
//! loader/MC compatibility with an instance, and install it into
//! `{instance}/.minecraft/mods/` as a manual (`source: None`) mod.

use std::io::{Cursor, Read};
use std::path::Path;

use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;
use sha1::{Digest, Sha1};
use tokio::fs;

use crate::error::Error;
use crate::mods::compat::ModLocalCompat;
use crate::mods::installed;
use crate::mods::platform::{InstalledMod, LoaderKind};
use crate::mods::version_range::RangeFamily;

/// Which loader family a mod jar targets, detected from its descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderFamily {
    /// Fabric or Quilt.
    Fabric,
    /// Forge or NeoForge (including legacy Forge).
    Forge,
}

/// Which side a declared dependency applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepSide {
    Both,
    Client,
    Server,
}

/// One declared dependency with everything the resolver needs.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredDep {
    pub dep_id: String,
    pub range: String,
    pub required: bool,
    pub side: DepSide,
    pub family: RangeFamily,
}

/// A mod id this jar provides, with its own declared version (post
/// `${file.jarVersion}` resolution). Multi-mod jars yield several.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvidedMod {
    pub mod_id: String,
    pub version: Option<String>,
}

/// Everything the pre-flight needs from one jar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManifestDeps {
    /// Own `[[mods]]` / fabric id / quilt id (+ JIJ as providers).
    pub provided: Vec<ProvidedMod>,
    /// Declared dependencies.
    pub deps: Vec<DeclaredDep>,
}

/// Best-effort metadata read from a mod `.jar`.
#[derive(Debug, Clone, Default)]
pub struct JarMeta {
    /// Loader families the jar's descriptor(s) declare. Empty when the jar
    /// has no recognised descriptor (coremod / library — undeterminable).
    /// A multi-loader jar (e.g. one shipping both `fabric.mod.json` and
    /// `META-INF/mods.toml`) lists every family it supports.
    pub families: Vec<LoaderFamily>,
    /// Display loader name — "Fabric" / "Quilt" / "Forge" / "NeoForge".
    pub loader_label: Option<String>,
    /// Declared Minecraft version reduced to `major.minor` (e.g. "1.12"),
    /// or `None` when not declared / not parseable.
    pub mc_version: Option<String>,
    /// The mod's display name from the descriptor, or `None`.
    pub display_name: Option<String>,
}

/// Extract the first `major.minor` (e.g. "1.20") substring from `s`.
/// Returns `None` when there is no `<digits>.<digits>` run (a bare `*`,
/// a snapshot id like "21w13a", an empty string).
fn first_major_minor(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i < b.len() && b[i] == b'.' {
            let mid = i + 1;
            let mut j = mid;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > mid {
                return Some(s[start..j].to_string());
            }
            // a dot with no digits after it — step past the dot
            i += 1;
        }
        // not a major.minor pattern — continue from the current position
    }
    None
}

/// Read a zip entry's text contents, or `None` if absent / unreadable.
fn entry_text(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// `fabric.mod.json` `depends.minecraft` is a string or an array of
/// strings — pull the first string out.
fn json_first_string(v: &serde_json::Value) -> Option<&str> {
    match v {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Array(a) => a.iter().find_map(|x| x.as_str()),
        _ => None,
    }
}

/// Read a mod jar's descriptor. Best-effort: a jar with no recognised
/// descriptor yields an all-`None` `JarMeta` (not an error). Only an
/// unreadable zip is an error.
pub fn read_jar_meta(jar_bytes: &[u8]) -> Result<JarMeta, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).map_err(|e| Error::ModsDecode {
        platform: "local jar".into(),
        details: e.to_string(),
    })?;

    // A jar may ship descriptors for MULTIPLE loaders (e.g. fabric.mod.json
    // AND META-INF/mods.toml). Collect every family present; the label is the
    // highest-priority one, for display only.
    let has_quilt = zip.by_name("quilt.mod.json").is_ok();
    let has_fabric_json = zip.by_name("fabric.mod.json").is_ok();
    let has_neoforge = zip.by_name("META-INF/neoforge.mods.toml").is_ok();
    let has_modstoml = zip.by_name("META-INF/mods.toml").is_ok();
    let has_mcmod = zip.by_name("mcmod.info").is_ok();
    let mut families: Vec<LoaderFamily> = Vec::new();
    if has_quilt || has_fabric_json {
        families.push(LoaderFamily::Fabric);
    }
    if has_neoforge || has_modstoml || has_mcmod {
        families.push(LoaderFamily::Forge);
    }
    let label: Option<&str> = if has_quilt {
        Some("Quilt")
    } else if has_fabric_json {
        Some("Fabric")
    } else if has_neoforge {
        Some("NeoForge")
    } else if has_modstoml || has_mcmod {
        Some("Forge")
    } else {
        None
    };

    // Best-effort MC version + display name — only from the simple JSON
    // descriptors (`fabric.mod.json`, legacy `mcmod.info`). `mods.toml` /
    // `quilt.mod.json` are not parsed for content in v1.
    let mut mc_version = None;
    let mut display_name = None;
    if let Some(txt) = entry_text(&mut zip, "fabric.mod.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            display_name = v.get("name").and_then(|x| x.as_str()).map(String::from);
            mc_version = v
                .get("depends")
                .and_then(|d| d.get("minecraft"))
                .and_then(json_first_string)
                .and_then(first_major_minor);
        }
    } else if let Some(txt) = entry_text(&mut zip, "mcmod.info") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            // mcmod.info is a JSON array of mod objects, or { modList: [...] }.
            let first = v.as_array().and_then(|a| a.first()).or_else(|| {
                v.get("modList")
                    .and_then(|m| m.as_array())
                    .and_then(|a| a.first())
            });
            if let Some(m) = first {
                display_name = m.get("name").and_then(|x| x.as_str()).map(String::from);
                mc_version = m
                    .get("mcversion")
                    .and_then(|x| x.as_str())
                    .and_then(first_major_minor);
            }
        }
    }

    Ok(JarMeta {
        families,
        loader_label: label.map(String::from),
        mc_version,
        display_name,
    })
}

/// Mod-id keys that name a loader or Minecraft itself, not a real mod dependency.
const LOADER_DEP_IDS: &[&str] = &[
    "forge",
    "neoforge",
    "fml",
    "minecraft",
    "fabric",
    "fabricloader",
    "fabric-loader",
    "java",
    "quilt_loader",
    "quilt",
];

fn push_dep(out: &mut Vec<String>, id: String) {
    let trimmed = id.trim();
    if trimmed.is_empty() || LOADER_DEP_IDS.contains(&trimmed.to_ascii_lowercase().as_str()) {
        return;
    }
    if !out.iter().any(|x| x.eq_ignore_ascii_case(trimmed)) {
        out.push(trimmed.to_string());
    }
}

/// Declared MANDATORY dependency mod-ids from a mod jar's descriptors.
/// Best-effort: Forge/NeoForge `mods.toml` / `neoforge.mods.toml` (each
/// `[[dependencies.*]]` that is required → its `modId`) plus Fabric
/// `fabric.mod.json` and Quilt `quilt.mod.json` `depends`. Loader/MC ids are
/// dropped; deduped case-insensitively. Empty for a jar with no recognised
/// descriptor; only an unreadable zip is an error.
///
/// Powers the "disabling X also breaks Y" warning — read from the jar (what FML
/// itself reads at load), not the launcher's `requires` registry, which records
/// only install-time pulls and is frequently empty.
pub fn read_jar_dependency_ids(jar_bytes: &[u8]) -> Result<Vec<String>, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).map_err(|e| Error::ModsDecode {
        platform: "local jar".into(),
        details: e.to_string(),
    })?;
    let mut out: Vec<String> = Vec::new();
    for name in ["META-INF/mods.toml", "META-INF/neoforge.mods.toml"] {
        if let Some(txt) = entry_text(&mut zip, name) {
            for id in parse_forge_mandatory_deps(&txt) {
                push_dep(&mut out, id);
            }
        }
    }
    if let Some(txt) = entry_text(&mut zip, "fabric.mod.json") {
        for id in parse_fabric_depends(&txt) {
            push_dep(&mut out, id);
        }
    }
    if let Some(txt) = entry_text(&mut zip, "quilt.mod.json") {
        for id in parse_quilt_depends(&txt) {
            push_dep(&mut out, id);
        }
    }
    Ok(out)
}

static FORGE_MODID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"modId\s*=\s*"([^"]+)""#).expect("forge modId regex compiles"));
static FORGE_MANDATORY_TRUE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"mandatory\s*=\s*true").expect("mandatory-true regex compiles"));
static FORGE_MANDATORY_ANY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"mandatory\s*=").expect("mandatory-any regex compiles"));
static FORGE_TYPE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"type\s*=\s*"([^"]+)""#).expect("dep-type regex compiles"));

// ── structured manifest readers ────────────────────────────────────────────

static FORGE_VERSION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)^\s*version\s*=\s*"([^"]+)""#).expect("forge version regex"));
static FORGE_VERSIONRANGE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"versionRange\s*=\s*"([^"]*)""#).expect("versionRange regex"));
static FORGE_SIDE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"side\s*=\s*"([^"]+)""#).expect("side regex"));
static MANIFEST_IMPL_VERSION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^Implementation-Version:\s*(.+)$").expect("impl-version regex"));

/// Extract required dependency mod-ids from a Forge/NeoForge `mods.toml`. Each
/// `[[dependencies.<owner>]]` block declares one `modId`; it counts as required
/// when `mandatory=true` (Forge), `type="required"` (NeoForge), or neither
/// marker is present (legacy default). An explicit `mandatory=false` or a
/// non-`required` `type` (optional / incompatible / discouraged) is skipped.
/// Regex-scanned rather than TOML-parsed to avoid a new crate — best-effort, but
/// the FML descriptor schema is regular enough that real jars parse.
fn parse_forge_mandatory_deps(text: &str) -> Vec<String> {
    let marker = "[[dependencies";
    let mut out: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(marker) {
        let block_start = from + rel + marker.len();
        // The block runs until the next TOML header (`[` anywhere later).
        let block_end = text[block_start..]
            .find('[')
            .map(|i| block_start + i)
            .unwrap_or(text.len());
        let block = &text[block_start..block_end];

        let type_val = FORGE_TYPE_RE
            .captures(block)
            .map(|c| c[1].to_ascii_lowercase());
        let required = FORGE_MANDATORY_TRUE_RE.is_match(block)
            || type_val.as_deref() == Some("required")
            || (!FORGE_MANDATORY_ANY_RE.is_match(block) && type_val.is_none());
        if required {
            if let Some(c) = FORGE_MODID_RE.captures(block) {
                out.push(c[1].to_string());
            }
        }
        from = block_end;
    }
    out
}

/// Fabric `fabric.mod.json` `depends` object keys (every entry is required;
/// `recommends`/`suggests` are not).
fn parse_fabric_depends(json_text: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return Vec::new();
    };
    v.get("depends")
        .and_then(|d| d.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// Quilt `quilt.mod.json` `quilt_loader.depends` — an array of mod-id strings or
/// `{ "id": "<modid>", … }` objects.
fn parse_quilt_depends(json_text: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return Vec::new();
    };
    let Some(arr) = v
        .get("quilt_loader")
        .and_then(|q| q.get("depends"))
        .and_then(|d| d.as_array())
    else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|e| match e {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(o) => o.get("id").and_then(|x| x.as_str()).map(String::from),
            _ => None,
        })
        .collect()
}

// ── structured manifest readers (pre-flight) ───────────────────────────────

/// Resolve `${file.jarVersion}` against `META-INF/MANIFEST.MF`. Returns the raw
/// value unchanged if it is not the token; `None` if it is the token and the
/// manifest attribute is absent (caller treats as the dev sentinel → Unknown).
fn resolve_jar_version(raw: &str, manifest: Option<&str>) -> Option<String> {
    if raw.trim() != "${file.jarVersion}" {
        return Some(raw.to_string());
    }
    manifest
        .and_then(|m| MANIFEST_IMPL_VERSION_RE.captures(m))
        .map(|c| c[1].trim().to_string())
}

fn is_loader_or_mc(id: &str) -> bool {
    LOADER_DEP_IDS.contains(&id.trim().to_ascii_lowercase().as_str())
}

/// Structured manifest read for the dependency pre-flight. Best-effort: a jar
/// with no recognised descriptor yields an empty `ManifestDeps`. Only an
/// unreadable zip is an error.
pub fn read_jar_manifest_deps(jar_bytes: &[u8]) -> Result<ManifestDeps, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).map_err(|e| Error::ModsDecode {
        platform: "local jar".into(),
        details: e.to_string(),
    })?;
    let manifest = entry_text(&mut zip, "META-INF/MANIFEST.MF");
    let mut out = ManifestDeps::default();

    for (name, family) in [
        ("META-INF/mods.toml", RangeFamily::Maven),
        ("META-INF/neoforge.mods.toml", RangeFamily::Maven),
    ] {
        if let Some(txt) = entry_text(&mut zip, name) {
            parse_forge_manifest(&txt, manifest.as_deref(), family, &mut out);
        }
    }
    if let Some(txt) = entry_text(&mut zip, "fabric.mod.json") {
        parse_fabric_manifest(&txt, &mut out);
    }
    if let Some(txt) = entry_text(&mut zip, "quilt.mod.json") {
        parse_quilt_manifest(&txt, &mut out);
    }
    out.deps.retain(|d| !is_loader_or_mc(&d.dep_id));
    Ok(out)
}

/// Find the end of a TOML section block starting at `from` in `text`.
/// A new section begins with `[[` or `[` at the start of a line; we
/// stop there. Falls back to `text.len()` when no next section exists.
///
/// Delimits a block by scanning for the next *line-initial* `[`, i.e. a
/// newline immediately followed by `[`. This is the correct delimiter because
/// a bare `find('[')` would falsely end the block inside a quoted value such as
/// `versionRange = "[1.3.51,)"`.
///
/// Known best-effort limitation: a TOML triple-quoted or multi-line string
/// value that begins a line with `[` would also be treated as a new section
/// header. In practice `mods.toml` never uses multi-line strings for
/// `modId`/`version` fields, so this case does not arise.
fn toml_block_end(text: &str, from: usize) -> usize {
    // Scan byte-by-byte for `\n[`; `[[mods]]` and `[[dependencies.*]]` headers
    // both start with `[`, so one check covers both array-of-tables and
    // plain-table headers.
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'\n' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            return i + 1;
        }
        i += 1;
    }
    text.len()
}

fn parse_forge_manifest(
    text: &str,
    manifest: Option<&str>,
    family: RangeFamily,
    out: &mut ManifestDeps,
) {
    // own [[mods]] id + version (resolve ${file.jarVersion})
    let mut mods_from = 0;
    while let Some(rel) = text[mods_from..].find("[[mods]]") {
        let start = mods_from + rel + "[[mods]]".len();
        let end = toml_block_end(text, start);
        let block = &text[start..end];
        if let Some(id) = FORGE_MODID_RE.captures(block) {
            let version = FORGE_VERSION_RE
                .captures(block)
                .and_then(|c| resolve_jar_version(&c[1], manifest));
            out.provided.push(ProvidedMod {
                mod_id: id[1].to_string(),
                version,
            });
        }
        mods_from = end;
    }
    // dependencies (reuse the same block-scan as parse_forge_mandatory_deps)
    let marker = "[[dependencies";
    let mut deps_from = 0;
    while let Some(rel) = text[deps_from..].find(marker) {
        let start = deps_from + rel + marker.len();
        let end = toml_block_end(text, start);
        let block = &text[start..end];
        let type_val = FORGE_TYPE_RE
            .captures(block)
            .map(|c| c[1].to_ascii_lowercase());
        let required = FORGE_MANDATORY_TRUE_RE.is_match(block)
            || type_val.as_deref() == Some("required")
            || (!FORGE_MANDATORY_ANY_RE.is_match(block) && type_val.is_none());
        if let Some(id) = FORGE_MODID_RE.captures(block) {
            let range = FORGE_VERSIONRANGE_RE
                .captures(block)
                .map(|c| c[1].to_string())
                .unwrap_or_default();
            let side = match FORGE_SIDE_RE
                .captures(block)
                .map(|c| c[1].to_ascii_uppercase())
                .as_deref()
            {
                Some("CLIENT") => DepSide::Client,
                Some("SERVER") => DepSide::Server,
                _ => DepSide::Both,
            };
            out.deps.push(DeclaredDep {
                dep_id: id[1].to_string(),
                range,
                required,
                side,
                family,
            });
        }
        deps_from = end;
    }
}

fn parse_fabric_manifest(json_text: &str, out: &mut ManifestDeps) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return;
    };
    if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
        out.provided.push(ProvidedMod {
            mod_id: id.to_string(),
            version: v.get("version").and_then(|x| x.as_str()).map(String::from),
        });
    }
    if let Some(obj) = v.get("depends").and_then(|d| d.as_object()) {
        for (id, val) in obj {
            out.deps.push(DeclaredDep {
                dep_id: id.clone(),
                range: predicate_value(val),
                required: true,
                side: DepSide::Both,
                family: RangeFamily::FabricPredicate,
            });
        }
    }
}

fn parse_quilt_manifest(json_text: &str, out: &mut ManifestDeps) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return;
    };
    let ql = v.get("quilt_loader");
    if let Some(id) = ql.and_then(|q| q.get("id")).and_then(|x| x.as_str()) {
        out.provided.push(ProvidedMod {
            mod_id: id.to_string(),
            version: ql
                .and_then(|q| q.get("version"))
                .and_then(|x| x.as_str())
                .map(String::from),
        });
    }
    let Some(arr) = ql.and_then(|q| q.get("depends")).and_then(|d| d.as_array()) else {
        return;
    };
    for e in arr {
        let (id, range, optional) = match e {
            serde_json::Value::String(s) => (Some(s.clone()), "*".to_string(), false),
            serde_json::Value::Object(o) => (
                o.get("id").and_then(|x| x.as_str()).map(String::from),
                o.get("versions")
                    .map(predicate_value)
                    .unwrap_or_else(|| "*".into()),
                o.get("optional").and_then(|x| x.as_bool()).unwrap_or(false),
            ),
            _ => (None, "*".into(), false),
        };
        if let Some(id) = id {
            out.deps.push(DeclaredDep {
                dep_id: id,
                range,
                required: !optional,
                side: DepSide::Both,
                family: RangeFamily::QuiltPredicate,
            });
        }
    }
}

/// A Fabric/Quilt predicate value: a string, or an array joined with ` || ` (OR).
fn predicate_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(" || "),
        _ => "*".to_string(),
    }
}

/// Read a zip entry's raw bytes, or `None` if absent / unreadable.
fn entry_bytes(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Read the JIJ (Jar-in-Jar) embedded jars from an outer jar's
/// `META-INF/jarjar/` directory. For each `.jar` found there, recursively
/// calls `read_jar_manifest_deps` to get the inner jar's real `[[mods]]`
/// `modId` + version (not the Maven artifact id from `metadata.json`, which
/// is unreliable). Returns empty on any error — best-effort, never fails.
pub fn read_jar_embedded_providers(jar_bytes: &[u8]) -> Vec<ProvidedMod> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(jar_bytes)) else {
        return Vec::new();
    };
    // Collect entry names first to avoid borrow conflicts when reading bytes.
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| {
            let entry = zip.by_index(i).ok()?;
            let name = entry.name().to_string();
            // Match META-INF/jarjar/<something>.jar (no sub-directories)
            if name.starts_with("META-INF/jarjar/")
                && name.ends_with(".jar")
                && name.matches('/').count() == 2
            {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    let mut out = Vec::new();
    for name in &names {
        if let Some(inner_bytes) = entry_bytes(&mut zip, name) {
            if let Ok(inner_manifest) = read_jar_manifest_deps(&inner_bytes) {
                out.extend(inner_manifest.provided);
            }
        }
    }
    out
}

/// Compatibility verdict for a local mod jar against a target instance.
/// Crosses the IPC boundary. A jar is "compatible" iff neither flag is set.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CompatVerdict {
    /// Display loader name detected in the jar ("Fabric" / "Forge" / …),
    /// or `None` when the jar has no recognised descriptor.
    pub detected_loader: Option<String>,
    /// `major.minor` Minecraft version detected in the jar, or `None`.
    pub detected_mc: Option<String>,
    /// The mod's display name from the jar, or `None`.
    pub detected_name: Option<String>,
    /// The jar's loader family is the opposite of the instance's.
    pub loader_mismatch: bool,
    /// The jar's declared MC `major.minor` differs from the instance's.
    pub mc_mismatch: bool,
}

/// Map an instance `LoaderKind` to a loader family. `Vanilla` has no
/// family — callers must not invoke `compat_verdict` for a vanilla
/// instance (the UI disables the dropzone there).
fn instance_family(loader: LoaderKind) -> Option<LoaderFamily> {
    match loader {
        LoaderKind::Fabric | LoaderKind::Quilt => Some(LoaderFamily::Fabric),
        LoaderKind::Forge | LoaderKind::NeoForge => Some(LoaderFamily::Forge),
        LoaderKind::Vanilla => None,
    }
}

/// Judge a jar's loader/MC compatibility with an instance. Conservative:
/// a mismatch is reported only when both sides are confidently known and
/// they differ — absent or ambiguous metadata never produces a warning.
pub fn compat_verdict(
    jar: &JarMeta,
    instance_loader: LoaderKind,
    instance_mc: &str,
) -> CompatVerdict {
    // Mismatch only when the jar declares loader families AND none of them is
    // the instance's family. A multi-loader jar that includes the instance's
    // family is compatible; a descriptor-less jar (empty families) never flags.
    let loader_mismatch = match instance_family(instance_loader) {
        Some(inf) => !jar.families.is_empty() && !jar.families.contains(&inf),
        None => false,
    };
    let mc_mismatch = match (jar.mc_version.as_deref(), first_major_minor(instance_mc)) {
        (Some(jmc), Some(imc)) => jmc != imc,
        _ => false,
    };
    CompatVerdict {
        detected_loader: jar.loader_label.clone(),
        detected_mc: jar.mc_version.clone(),
        detected_name: jar.display_name.clone(),
        loader_mismatch,
        mc_mismatch,
    }
}

/// Offline loader-family compatibility scan of an instance's installed mods.
/// Layer 1 of the proactive incompatibility check: judges both hand-dropped
/// (`source = None`) and platform-installed (`source = Some(...)`) mods by
/// reading their descriptor and comparing loader families against the instance.
/// Pack-bundled mods are trusted (the pack already vetted them for this
/// loader+MC) and are never judged. For platform mods the offline verdict is
/// only a SUSPECT pre-filter — the `live_checkable` flag tells the frontend
/// to auto-run an authoritative live check on those. Network-free. A mod
/// whose jar is missing/unreadable, or has no recognised descriptor, yields
/// `loader_mismatch = false` (conservative — never a false alarm). `mc` is
/// passed to `compat_verdict` (its signature needs it) but only the loader
/// outputs are surfaced.
pub async fn scan_instance(
    instance_root: &Path,
    instance_loader: LoaderKind,
    mc: &str,
) -> Result<Vec<ModLocalCompat>, Error> {
    use crate::mods::updates::{eligible_identity, is_pack_origin_mod};
    let mods = installed::list(instance_root).await?;
    let pack_origin = installed::get_pack_origin(instance_root).await?;
    let dir = installed::mods_dir(instance_root);
    let mut out = Vec::with_capacity(mods.len());
    for m in &mods {
        // Judge loader-family for manual AND platform mods; pack-bundled mods are
        // trusted (the pack vetted them) and never judged. For platform mods the
        // offline verdict is only a SUSPECT pre-filter — the frontend auto-runs an
        // authoritative live check on the platform suspects.
        let is_pack = is_pack_origin_mod(m, pack_origin.as_ref());
        let verdict = if is_pack {
            None
        } else {
            read_jar_for(&dir, &m.filename)
                .await
                .and_then(|bytes| read_jar_meta(&bytes).ok())
                .map(|meta| compat_verdict(&meta, instance_loader, mc))
        };
        out.push(ModLocalCompat {
            sha1: m.sha1.clone(),
            loader_mismatch: verdict.as_ref().map(|v| v.loader_mismatch).unwrap_or(false),
            detected_loader: verdict.and_then(|v| v.detected_loader),
            live_checkable: eligible_identity(m, pack_origin.as_ref()).is_some(),
        });
    }
    Ok(out)
}

/// Read a mod jar's bytes by base filename, trying the `.disabled` variant
/// too. Returns `None` if neither exists or the read fails.
async fn read_jar_for(mods_dir: &Path, filename: &str) -> Option<Vec<u8>> {
    if let Ok(b) = fs::read(mods_dir.join(filename)).await {
        return Some(b);
    }
    fs::read(mods_dir.join(format!("{filename}.disabled")))
        .await
        .ok()
}

/// Install a local mod jar into `{instance}/.minecraft/mods/`: write the
/// bytes, record an `InstalledMod` with `source: None` (a manual mod —
/// no platform). `display_name` becomes the recorded `name`; when absent
/// the filename (without `.jar`) is used.
///
/// Filename-conflict handling mirrors `install::install_one`: a file
/// already present with the same name and the same SHA-1 is an idempotent
/// success; the same name with different bytes is `ModsFilenameConflict`.
pub async fn install_local(
    instance_root: &Path,
    filename: &str,
    bytes: &[u8],
    display_name: Option<&str>,
) -> Result<InstalledMod, Error> {
    // Guard FIRST — before any filesystem I/O — that the filename is a safe
    // single segment. Today's only caller derives this from `Path::file_name()`
    // (which strips parent components), but this `pub` function must be safe by
    // construction regardless of caller, mirroring `install::install_one`.
    if !crate::mods::modpack::path_safety::is_safe_filename(filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: filename.to_string(),
        });
    }

    let sha = hex::encode(Sha1::digest(bytes));

    let dest_dir = installed::mods_dir(instance_root);
    fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dest_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = dest_dir.join(filename);
    if fs::try_exists(&dest)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?
    {
        let existing = fs::read(&dest).await.map_err(|e| Error::ModsInstancePath {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?;
        let existing_sha = hex::encode(Sha1::digest(&existing));
        if !existing_sha.eq_ignore_ascii_case(&sha) {
            return Err(Error::ModsFilenameConflict {
                filename: filename.to_string(),
                existing_sha,
                incoming_sha: sha,
            });
        }
        // same name + same bytes → idempotent: fall through to record.
    } else {
        fs::write(&dest, bytes)
            .await
            .map_err(|e| Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })?;
    }

    let name = display_name.map(str::to_string).unwrap_or_else(|| {
        filename
            .strip_suffix(".jar")
            .unwrap_or(filename)
            .to_string()
    });
    let entry = InstalledMod {
        filename: filename.to_string(),
        sha1: sha,
        source: None,
        project_id: None,
        version_id: None,
        name,
        version_number: None,
        installed_at: Utc::now().to_rfc3339(),
        enabled: true,
        enrich_attempted: false,
        requires: Vec::new(),
    };
    installed::add(instance_root, entry.clone()).await?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    /// Build an in-memory `.jar` (zip) with the given (name, contents) entries.
    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn first_major_minor_extracts_from_common_shapes() {
        assert_eq!(first_major_minor("1.12.2").as_deref(), Some("1.12"));
        assert_eq!(first_major_minor(">=1.20.1 <1.21").as_deref(), Some("1.20"));
        assert_eq!(first_major_minor("~1.19").as_deref(), Some("1.19"));
        assert_eq!(first_major_minor("*"), None);
        assert_eq!(first_major_minor("21w13a"), None);
    }

    // ── jar-declared dependency reading (the "breaks" warning source) ──────────

    #[test]
    fn parse_forge_deps_includes_required_excludes_optional() {
        let toml = "\
[[mods]]
modId=\"evilseagull\"
[[dependencies.evilseagull]]
    modId=\"alexsmobs\"
    mandatory=true
    versionRange=\"[1.22.0,)\"
[[dependencies.evilseagull]]
    modId=\"jei\"
    mandatory=false
[[dependencies.evilseagull]]
    modId=\"citadel\"
    type=\"required\"
[[dependencies.evilseagull]]
    modId=\"curios\"
    type=\"optional\"
";
        let deps = parse_forge_mandatory_deps(toml);
        assert!(deps.contains(&"alexsmobs".to_string()), "{deps:?}");
        assert!(deps.contains(&"citadel".to_string()), "{deps:?}");
        assert!(
            !deps.contains(&"jei".to_string()),
            "mandatory=false: {deps:?}"
        );
        assert!(
            !deps.contains(&"curios".to_string()),
            "type=optional: {deps:?}"
        );
        // The declaring mod's own id (in [[mods]]) is not scanned as a dependency.
        assert!(!deps.contains(&"evilseagull".to_string()), "{deps:?}");
    }

    #[test]
    fn parse_fabric_depends_returns_keys() {
        let json = r#"{"depends":{"minecraft":">=1.20.1","fabricloader":">=0.15","sodium":"*"}}"#;
        let mut deps = parse_fabric_depends(json);
        deps.sort();
        assert_eq!(deps, vec!["fabricloader", "minecraft", "sodium"]);
    }

    #[test]
    fn parse_quilt_depends_strings_and_objects() {
        let json = r#"{"quilt_loader":{"depends":["sodium",{"id":"alexsmobs","versions":"*"}]}}"#;
        assert_eq!(
            parse_quilt_depends(json),
            vec!["sodium".to_string(), "alexsmobs".to_string()]
        );
    }

    #[test]
    fn read_jar_dependency_ids_merges_descriptors_and_drops_loader_ids() {
        let bytes = jar(&[
            (
                "META-INF/mods.toml",
                "[[dependencies.evilseagull]]\nmodId=\"forge\"\nmandatory=true\n\
                 [[dependencies.evilseagull]]\nmodId=\"alexsmobs\"\nmandatory=true\nversionRange=\"[1.22.0,)\"\n",
            ),
            (
                "fabric.mod.json",
                r#"{"depends":{"minecraft":">=1.20.1","citadel":"*"}}"#,
            ),
        ]);
        let deps = read_jar_dependency_ids(&bytes).unwrap();
        assert!(deps.iter().any(|d| d == "alexsmobs"), "{deps:?}");
        assert!(deps.iter().any(|d| d == "citadel"), "{deps:?}");
        assert!(
            !deps.iter().any(|d| d.eq_ignore_ascii_case("forge")),
            "loader id dropped: {deps:?}"
        );
        assert!(
            !deps.iter().any(|d| d.eq_ignore_ascii_case("minecraft")),
            "mc id dropped: {deps:?}"
        );
    }

    #[test]
    fn read_jar_dependency_ids_empty_for_descriptorless_jar() {
        let bytes = jar(&[("foo.txt", "nothing")]);
        assert!(read_jar_dependency_ids(&bytes).unwrap().is_empty());
    }

    #[test]
    fn detects_fabric_jar() {
        let j = jar(&[(
            "fabric.mod.json",
            r#"{"name":"Sodium","depends":{"minecraft":">=1.20.1"}}"#,
        )]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.families, vec![LoaderFamily::Fabric]);
        assert_eq!(m.loader_label.as_deref(), Some("Fabric"));
        assert_eq!(m.mc_version.as_deref(), Some("1.20"));
        assert_eq!(m.display_name.as_deref(), Some("Sodium"));
    }

    #[test]
    fn detects_quilt_jar() {
        let j = jar(&[("quilt.mod.json", r#"{"quilt_loader":{"id":"x"}}"#)]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.families, vec![LoaderFamily::Fabric]);
        assert_eq!(m.loader_label.as_deref(), Some("Quilt"));
    }

    #[test]
    fn quilt_jar_with_both_descriptors_labels_quilt_but_reads_fabric_meta() {
        // Quilt mods commonly ship both descriptors: the label comes from
        // the (priority-first) quilt descriptor, while the MC version and
        // display name are still read from fabric.mod.json.
        let j = jar(&[
            ("quilt.mod.json", r#"{"quilt_loader":{"id":"x"}}"#),
            (
                "fabric.mod.json",
                r#"{"name":"Sodium","depends":{"minecraft":">=1.20.1"}}"#,
            ),
        ]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.loader_label.as_deref(), Some("Quilt"));
        assert_eq!(m.mc_version.as_deref(), Some("1.20"));
        assert_eq!(m.display_name.as_deref(), Some("Sodium"));
    }

    #[test]
    fn detects_modern_forge_jar_without_mc_version() {
        let j = jar(&[("META-INF/mods.toml", "modLoader=\"javafml\"\n")]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.families, vec![LoaderFamily::Forge]);
        assert_eq!(m.loader_label.as_deref(), Some("Forge"));
        assert_eq!(m.mc_version, None); // mods.toml is not parsed in v1
    }

    #[test]
    fn detects_neoforge_jar() {
        let j = jar(&[("META-INF/neoforge.mods.toml", "modLoader=\"javafml\"\n")]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.families, vec![LoaderFamily::Forge]);
        assert_eq!(m.loader_label.as_deref(), Some("NeoForge"));
    }

    #[test]
    fn detects_legacy_forge_jar_with_mc_version() {
        let j = jar(&[(
            "mcmod.info",
            r#"[{"modid":"srparasites","name":"Scape and Run: Parasites","mcversion":"1.12.2"}]"#,
        )]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.families, vec![LoaderFamily::Forge]);
        assert_eq!(m.loader_label.as_deref(), Some("Forge"));
        assert_eq!(m.mc_version.as_deref(), Some("1.12"));
        assert_eq!(m.display_name.as_deref(), Some("Scape and Run: Parasites"));
    }

    #[test]
    fn no_descriptor_yields_empty_meta() {
        let j = jar(&[("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n")]);
        let m = read_jar_meta(&j).unwrap();
        assert!(m.families.is_empty());
        assert_eq!(m.loader_label, None);
        assert_eq!(m.mc_version, None);
    }

    #[test]
    fn invalid_jar_errors() {
        let r = read_jar_meta(b"not a zip at all");
        assert!(r.is_err());
    }

    fn meta(families: Vec<LoaderFamily>, mc: Option<&str>) -> JarMeta {
        let loader_label = families.first().map(|f| match f {
            LoaderFamily::Fabric => "Fabric".into(),
            LoaderFamily::Forge => "Forge".into(),
        });
        JarMeta {
            families,
            loader_label,
            mc_version: mc.map(String::from),
            display_name: None,
        }
    }

    #[test]
    fn verdict_compatible_when_loader_and_mc_match() {
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Forge], Some("1.12")),
            LoaderKind::Forge,
            "1.12.2",
        );
        assert!(!v.loader_mismatch);
        assert!(!v.mc_mismatch);
    }

    #[test]
    fn verdict_flags_loader_mismatch() {
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Fabric], None),
            LoaderKind::Forge,
            "1.20.1",
        );
        assert!(v.loader_mismatch);
    }

    #[test]
    fn verdict_no_loader_mismatch_within_forge_family() {
        // A jar detected as Forge-family on a NeoForge instance — same family.
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Forge], None),
            LoaderKind::NeoForge,
            "1.20.1",
        );
        assert!(!v.loader_mismatch);
    }

    #[test]
    fn verdict_flags_mc_mismatch() {
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Forge], Some("1.20")),
            LoaderKind::Forge,
            "1.12.2",
        );
        assert!(v.mc_mismatch);
    }

    #[test]
    fn verdict_silent_when_metadata_absent() {
        // No descriptor at all — never warn.
        let v = compat_verdict(&meta(vec![], None), LoaderKind::Forge, "1.12.2");
        assert!(!v.loader_mismatch);
        assert!(!v.mc_mismatch);
    }

    #[test]
    fn verdict_silent_when_jar_mc_unknown() {
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Forge], None),
            LoaderKind::Forge,
            "1.12.2",
        );
        assert!(!v.mc_mismatch);
    }

    #[test]
    fn verdict_multiloader_jar_matches_either_family() {
        let jar = JarMeta {
            families: vec![LoaderFamily::Fabric, LoaderFamily::Forge],
            loader_label: Some("Fabric".into()),
            mc_version: None,
            display_name: Some("Collective".into()),
        };
        assert!(!compat_verdict(&jar, LoaderKind::Forge, "1.20.4").loader_mismatch);
        assert!(!compat_verdict(&jar, LoaderKind::Fabric, "1.20.4").loader_mismatch);
    }

    // ── scan_instance tests ────────────────────────────────────────────────

    /// Build a minimal in-memory zip (jar) from (name, bytes) entries.
    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

    #[tokio::test]
    async fn scan_flags_fabric_jar_in_forge_instance() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("fabric.mod.json", br#"{"id":"x","name":"X"}"#)]);
        fs::write(dir.join("x.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].loader_mismatch);
        assert_eq!(out[0].detected_loader.as_deref(), Some("Fabric"));
    }

    #[tokio::test]
    async fn scan_no_mismatch_for_forge_jar_in_forge_instance() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("META-INF/mods.toml", b"modLoader=\"javafml\"")]);
        fs::write(dir.join("f.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
        assert_eq!(out[0].detected_loader.as_deref(), Some("Forge"));
    }

    #[tokio::test]
    async fn scan_no_mismatch_for_descriptorless_jar() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("data/whatever.txt", b"not a mod")]);
        fs::write(dir.join("lib.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Fabric, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
        assert!(out[0].detected_loader.is_none());
    }

    #[tokio::test]
    async fn scan_vanilla_instance_never_flags() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("fabric.mod.json", br#"{"id":"x"}"#)]);
        fs::write(dir.join("x.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Vanilla, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
    }

    #[tokio::test]
    async fn scan_skips_unreadable_jar_but_still_succeeds() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(dir.join("broken.jar"), b"not a zip at all")
            .await
            .unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
        assert!(out[0].detected_loader.is_none());
    }

    #[tokio::test]
    async fn scan_multiloader_fabric_forge_jar_not_flagged_on_forge() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        // A jar that ships BOTH fabric and forge descriptors (like Collective).
        let bytes = zip_with(&[
            (
                "fabric.mod.json",
                br#"{"id":"collective","name":"Collective"}"#,
            ),
            ("META-INF/mods.toml", b"modLoader=\"javafml\""),
        ]);
        fs::write(dir.join("collective.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.20.4")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            !out[0].loader_mismatch,
            "multi-loader jar must NOT be flagged on Forge"
        );
    }

    #[tokio::test]
    async fn scan_multiloader_fabric_forge_jar_not_flagged_on_fabric() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[
            (
                "fabric.mod.json",
                br#"{"id":"collective","name":"Collective"}"#,
            ),
            ("META-INF/mods.toml", b"modLoader=\"javafml\""),
        ]);
        fs::write(dir.join("collective.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Fabric, "1.20.4")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            !out[0].loader_mismatch,
            "multi-loader jar must NOT be flagged on Fabric"
        );
    }

    #[tokio::test]
    async fn scan_marks_platform_mod_as_live_checkable_suspect() {
        use crate::mods::installed::{add, mods_dir};
        use crate::mods::platform::{InstalledMod, ModSource};
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("fabric.mod.json", br#"{"id":"x","name":"X"}"#)]);
        fs::write(dir.join("x.jar"), &bytes).await.unwrap();
        let sha = hex::encode(Sha1::digest(&bytes));
        add(
            td.path(),
            InstalledMod {
                filename: "x.jar".into(),
                sha1: sha.clone(),
                source: Some(ModSource::Modrinth),
                project_id: Some("xxx".into()),
                version_id: Some("vvv".into()),
                name: "X".into(),
                version_number: Some("1.0".into()),
                installed_at: chrono::Utc::now().to_rfc3339(),
                enabled: true,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await
        .unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.20.4")
            .await
            .unwrap();
        let m = out
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        // Platform mod with a mismatching descriptor is a SUSPECT, and is
        // live-checkable so the frontend can auto-confirm it.
        assert!(
            m.loader_mismatch,
            "platform mod with wrong-family descriptor is a suspect"
        );
        assert!(
            m.live_checkable,
            "platform mod must be marked live-checkable"
        );
    }

    // ── install_local tests ────────────────────────────────────────────────

    use tempfile::TempDir;

    #[tokio::test]
    async fn install_local_copies_jar_and_records_manual_mod() {
        let td_inst = TempDir::new().unwrap();
        let body = b"fabric-mod-bytes";
        let installed = install_local(td_inst.path(), "sodium.jar", body, Some("Sodium"))
            .await
            .unwrap();
        assert_eq!(installed.filename, "sodium.jar");
        assert!(crate::mods::installed::mods_dir(td_inst.path())
            .join("sodium.jar")
            .exists());
        let list = crate::mods::installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source, None); // manual mod
        assert_eq!(list[0].name, "Sodium");
        assert_eq!(list[0].sha1, installed.sha1);
    }

    #[tokio::test]
    async fn install_local_rejects_unsafe_filename_before_io() {
        let td_inst = TempDir::new().unwrap();
        let err = install_local(td_inst.path(), "../../evil.jar", b"x", None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::ModsUnsafeFilename { ref filename } if filename == "../../evil.jar"),
            "expected ModsUnsafeFilename, got {err:?}"
        );
        // No directory was created and nothing escaped.
        assert!(!crate::mods::installed::mods_dir(td_inst.path()).exists());
        assert!(!td_inst.path().join("evil.jar").exists());
    }

    #[tokio::test]
    async fn install_local_falls_back_to_filename_when_no_display_name() {
        let td_inst = TempDir::new().unwrap();
        install_local(td_inst.path(), "weird-mod.jar", b"x", None)
            .await
            .unwrap();
        let list = crate::mods::installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list[0].name, "weird-mod");
    }

    #[tokio::test]
    async fn install_local_same_name_same_bytes_is_idempotent() {
        let td_inst = TempDir::new().unwrap();
        install_local(td_inst.path(), "a.jar", b"same", Some("A"))
            .await
            .unwrap();
        install_local(td_inst.path(), "a.jar", b"same", Some("A"))
            .await
            .unwrap();
        let list = crate::mods::installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn install_local_same_name_different_bytes_conflicts() {
        let td_inst = TempDir::new().unwrap();
        install_local(td_inst.path(), "a.jar", b"first", Some("A"))
            .await
            .unwrap();
        let err = install_local(td_inst.path(), "a.jar", b"second", Some("A"))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ModsFilenameConflict { .. }));
    }

    // ── structured manifest reader tests ──────────────────────────────────────

    #[test]
    fn reads_forge_dep_with_versionrange_and_own_version() {
        let toml = "[[mods]]\nmodId=\"backpacks\"\nversion=\"3.20.0\"\n\
            [[dependencies.backpacks]]\nmodId=\"sophisticatedcore\"\nmandatory=true\n\
            versionRange=\"[1.3.51,)\"\nside=\"BOTH\"\n";
        let j = jar(&[("META-INF/mods.toml", toml)]);
        let m = read_jar_manifest_deps(&j).unwrap();
        assert!(
            m.provided
                .iter()
                .any(|p| p.mod_id == "backpacks" && p.version.as_deref() == Some("3.20.0")),
            "provided: {:?}",
            m.provided
        );
        let dep = m
            .deps
            .iter()
            .find(|d| d.dep_id == "sophisticatedcore")
            .unwrap();
        assert_eq!(dep.range, "[1.3.51,)");
        assert!(dep.required);
    }

    #[test]
    fn fabric_provides_id_and_required_depends_with_predicate() {
        let json = r#"{"id":"sodium","version":"0.5.3","depends":{"minecraft":">=1.20.1","fabricloader":">=0.15","fabric-api":">=0.90"}}"#;
        let j = jar(&[("fabric.mod.json", json)]);
        let m = read_jar_manifest_deps(&j).unwrap();
        assert!(
            m.provided
                .iter()
                .any(|p| p.mod_id == "sodium" && p.version.as_deref() == Some("0.5.3")),
            "provided: {:?}",
            m.provided
        );
        // minecraft + fabricloader filtered; fabric-api kept.
        assert_eq!(
            m.deps.iter().map(|d| d.dep_id.as_str()).collect::<Vec<_>>(),
            vec!["fabric-api"]
        );
    }

    #[test]
    fn forge_resolves_file_jar_version_from_manifest() {
        let toml = "[[mods]]\nmodId=\"x\"\nversion=\"${file.jarVersion}\"\n";
        let j = jar(&[
            ("META-INF/mods.toml", toml),
            (
                "META-INF/MANIFEST.MF",
                "Manifest-Version: 1.0\nImplementation-Version: 7.8.9\n",
            ),
        ]);
        let m = read_jar_manifest_deps(&j).unwrap();
        assert_eq!(m.provided[0].version.as_deref(), Some("7.8.9"));
    }

    #[test]
    fn quilt_optional_dep_is_not_required() {
        let json = r#"{"quilt_loader":{"id":"x","version":"1.0.0","depends":[{"id":"sodium","versions":">=0.5","optional":true}]}}"#;
        let j = jar(&[("quilt.mod.json", json)]);
        let m = read_jar_manifest_deps(&j).unwrap();
        assert!(
            !m.deps
                .iter()
                .find(|d| d.dep_id == "sodium")
                .unwrap()
                .required
        );
    }

    #[test]
    fn jij_reader_extracts_embedded_jar_mod_id() {
        // Build inner jar declaring modId="embeddedlib" version="2.1.0"
        let inner_toml = "[[mods]]\nmodId=\"embeddedlib\"\nversion=\"2.1.0\"\n";
        let inner_bytes = jar(&[("META-INF/mods.toml", inner_toml)]);

        // Build outer jar with inner jar at META-INF/jarjar/lib.jar
        let outer = {
            let mut buf = Vec::new();
            {
                let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
                w.start_file("META-INF/jarjar/lib.jar", SimpleFileOptions::default())
                    .unwrap();
                w.write_all(&inner_bytes).unwrap();
                w.finish().unwrap();
            }
            buf
        };

        let providers = read_jar_embedded_providers(&outer);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].mod_id, "embeddedlib");
        assert_eq!(providers[0].version.as_deref(), Some("2.1.0"));
    }

    #[test]
    fn file_jar_version_without_manifest_yields_none() {
        // A jar that declares `version = "${file.jarVersion}"` but ships no
        // META-INF/MANIFEST.MF. The token cannot be resolved, so the caller
        // receives `None` and treats it as the dev/unknown sentinel.
        let toml = "[[mods]]\nmodId=\"x\"\nversion=\"${file.jarVersion}\"\n";
        let j = jar(&[("META-INF/mods.toml", toml)]);
        let m = read_jar_manifest_deps(&j).unwrap();
        assert_eq!(
            m.provided[0].version, None,
            "unresolvable ${{file.jarVersion}} must yield None, got {:?}",
            m.provided[0].version
        );
    }
}
