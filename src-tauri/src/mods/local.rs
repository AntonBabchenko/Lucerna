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
use crate::mods::forge_descriptor;
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

/// What a declared dependency means to the loader. Mirrors
/// `IModInfo.DependencyType` (FancyModLoader 1.21.1 `ModInfo.java:78-88`).
/// Fabric/Quilt `depends` entries map onto `Required` / `Optional`; their
/// `breaks` blocks are not read today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// The loader aborts when the dependency is absent or out of range.
    Required,
    /// Absent is fine; the loader aborts when it is present AND out of range
    /// (`ModSorter.java:281` feeds that into the same abort as a missing
    /// requirement).
    Optional,
    /// The loader aborts when the dependency is present AND its version IS
    /// inside the range — the range is inverted relative to `Required`
    /// (`ModSorter.java:286-288`).
    Incompatible,
    /// The loader logs an error and carries on ("Continue at your own risk").
    Discouraged,
}

impl DependencyKind {
    /// True only for `Required` — i.e. "this must be installed at all".
    pub fn is_required(self) -> bool {
        matches!(self, DependencyKind::Required)
    }
}

/// Which descriptor a declared dependency came out of.
///
/// [`RangeFamily`] cannot answer this — `Maven` covers `mods.toml`,
/// `neoforge.mods.toml` and the legacy `@Mod` annotation alike — and the
/// pre-flight needs it to drop declarations from a file the instance's loader
/// never opens. Forge 1.12.2 does not read `mods.toml`; Forge 1.20 does not read
/// `mcmod.info`. A measured 1.12.2 jar shipped both, disagreeing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorSource {
    /// `@Mod(dependencies = "…")` in a class constant pool — Forge ≤ 1.12.2.
    McmodAnnotation,
    /// `META-INF/mods.toml` — MinecraftForge ≥ 1.13, NeoForge ≤ 1.20.4.
    ModsToml,
    /// `META-INF/neoforge.mods.toml` — NeoForge ≥ 1.20.6.
    NeoForgeToml,
    FabricJson,
    QuiltJson,
}

/// One declared dependency with everything the resolver needs.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredDep {
    pub dep_id: String,
    pub range: String,
    pub kind: DependencyKind,
    pub side: DepSide,
    pub family: RangeFamily,
    /// The descriptor this declaration was read out of. Decides whether the
    /// instance's loader enforces it at all — see `preflight::dep_applies_to_instance`.
    pub source: DescriptorSource,
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

/// Which mod-descriptor era an instance's Minecraft version belongs to.
///
/// Forge replaced `mcmod.info` with `META-INF/mods.toml` in the FML rewrite at
/// MC 1.13. A jar may ship both — one measured 1.12.2 jar carries a `mods.toml`
/// stamped `loaderVersion="[24,)"`, i.e. written for its 1.14+ build — so "which
/// file is present" cannot decide; only the instance's version can.
///
/// `forge::installer::Era` is deliberately not reused: it is derived from
/// `install_profile.json`, an installer artefact, not from the MC version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorEra {
    Legacy,
    Modern,
}

/// An unrecognised version reads as `Modern`: that is today's behaviour, and
/// guessing `Legacy` would silence every dependency on a version we misparsed.
pub fn descriptor_era(mc_version: &str) -> DescriptorEra {
    let Some(mm) = first_major_minor(mc_version) else {
        return DescriptorEra::Modern;
    };
    let mut parts = mm.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    if major == 1 && minor < 13 {
        DescriptorEra::Legacy
    } else {
        DescriptorEra::Modern
    }
}

/// Read a zip entry's text contents, or `None` if absent / unreadable.
///
/// Decoding is lossy on purpose: a single non-UTF8 byte — a cp1252 author name
/// in a `credits` field is the usual culprit — previously discarded the whole
/// descriptor, and with it every provider and dependency of that jar.
fn entry_text(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
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

/// Declared runtime side of a mod, from its descriptor. Forge `mods.toml` has
/// no universal mod-level client-only field → `Unknown` for most Forge mods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ModEnvironment {
    Client,
    Server,
    Both,
    Unknown,
}

/// Best-effort declared side of a mod jar. Fabric/Quilt `environment`
/// ("client"/"server"/"*") is reliable; Forge/NeoForge yields `Unknown`.
pub fn read_jar_environment(jar_bytes: &[u8]) -> ModEnvironment {
    let mut zip = match zip::ZipArchive::new(Cursor::new(jar_bytes)) {
        Ok(z) => z,
        Err(_) => return ModEnvironment::Unknown,
    };
    for name in ["fabric.mod.json", "quilt.mod.json"] {
        if let Some(text) = entry_text(&mut zip, name) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let env = v.get("environment").and_then(|e| e.as_str()).or_else(|| {
                    v.get("quilt_loader")
                        .and_then(|q| q.get("metadata"))
                        .and_then(|m| m.get("environment"))
                        .and_then(|e| e.as_str())
                });
                return match env {
                    Some("client") => ModEnvironment::Client,
                    Some("server") => ModEnvironment::Server,
                    Some(_) => ModEnvironment::Both,
                    None => ModEnvironment::Both,
                };
            }
        }
    }
    ModEnvironment::Unknown
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

/// Declared REQUIRED dependency mod-ids from a mod jar's descriptors.
/// Best-effort, over the same reader the pre-flight uses: only
/// [`DependencyKind::Required`] deps count, so an `optional` /
/// `incompatible` / `discouraged` declaration is not reported as something the
/// jar needs. Loader/MC ids are dropped; deduped case-insensitively. Empty for
/// a jar with no recognised descriptor; only an unreadable zip is an error.
///
/// Powers the "disabling X also breaks Y" warning — read from the jar (what FML
/// itself reads at load), not the launcher's `requires` registry, which records
/// only install-time pulls and is frequently empty.
pub fn read_jar_dependency_ids(jar_bytes: &[u8]) -> Result<Vec<String>, Error> {
    // One reader, not two. This used to run a second, weaker block scanner
    // (`find('[')` truncated a block at the bracket inside
    // `versionRange="[1.0,)"`, hiding every key declared after it), which made
    // the two parsers disagree about the same jar.
    let mut out: Vec<String> = Vec::new();
    for dep in read_jar_manifest_deps(jar_bytes)?.deps {
        if dep.kind.is_required() {
            push_dep(&mut out, dep.dep_id);
        }
    }
    Ok(out)
}

static FORGE_MODID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"modId\s*=\s*"([^"]+)""#).expect("forge modId regex compiles"));
static FORGE_MANDATORY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"mandatory\s*=\s*(true|false)").expect("mandatory regex compiles"));
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

/// Parse the value of `@Mod(dependencies = "…")` — the string FML ≤ 1.12.2
/// enforces. Clauses are `;`-separated, each `<prefix>:<modid>[@<versionRange>]`.
///
/// Only `required-*` is a requirement; a bare `after` / `before` is load-order
/// only and must not become one. `*` is the wildcard ordering target, not a mod.
/// Loader and Minecraft ids are dropped for the same reason
/// [`read_jar_manifest_deps`] drops them — nearly every 1.12.2 annotation opens
/// with `required-after:forge` and `required-after:minecraft`, and reporting
/// those as missing mods would put a phantom on every jar of every legacy pack.
pub(crate) fn parse_legacy_dependency_string(raw: &str) -> Vec<DeclaredDep> {
    let mut out = Vec::new();
    for clause in raw.split(';') {
        let Some((prefix, rest)) = clause.trim().split_once(':') else {
            continue;
        };
        if !matches!(
            prefix.trim().to_ascii_lowercase().as_str(),
            "required-after" | "required-before"
        ) {
            continue;
        }
        let (id, range) = rest.split_once('@').unwrap_or((rest, ""));
        let id = id.trim();
        if id.is_empty() || id == "*" || is_loader_or_mc(id) {
            continue;
        }
        out.push(DeclaredDep {
            dep_id: id.to_string(),
            range: range.trim().to_string(),
            kind: DependencyKind::Required,
            // The legacy string carries no side.
            side: DepSide::Both,
            family: RangeFamily::Maven,
            source: DescriptorSource::McmodAnnotation,
        });
    }
    out
}

/// Widest printable-ASCII run containing byte index `at`.
///
/// A `CONSTANT_Utf8` entry is length-prefixed rather than NUL-terminated, so a
/// run can occasionally merge with an adjacent constant. That is harmless here:
/// the clause parser skips anything without a `required-*:` prefix, and the
/// worst case is a trailing character on the last clause's range.
fn printable_run(bytes: &[u8], at: usize) -> &str {
    let printable = |b: u8| (0x20..=0x7e).contains(&b);
    let mut start = at;
    while start > 0 && printable(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < bytes.len() && printable(bytes[end]) {
        end += 1;
    }
    std::str::from_utf8(&bytes[start..end]).unwrap_or("")
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Recover Forge ≤ 1.12.2 dependencies from the `@Mod(dependencies = …)`
/// annotation, which is the only place that era declares them — `mcmod.info`'s
/// own array is cosmetic.
///
/// `own_ids` are the jar's provided mod-ids (from `mcmod.info`). Clauses are
/// accepted only from a class that also carries one of them as a constant: a
/// library that merely mentions the syntax does not produce that pair, while the
/// real `@Mod` class does — verified on `EnhancedVisuals.class`, where
/// `enhancedvisuals` and `required-after:creativecore` sit side by side.
///
/// Stops at the first class in this jar that yields clauses under the guard,
/// including when every clause names the loader or Minecraft and the filtered
/// result is empty: that class IS the annotation, and continuing past it would
/// only find false positives.
///
/// Cost: decompresses class entries until that class is found. Affordable once
/// per jar, which is what the sha1-keyed scan cache exists for; it runs only for
/// `DescriptorEra::Legacy` instances, so modern packs pay nothing.
pub(crate) fn read_jar_legacy_deps(jar_bytes: &[u8], own_ids: &[String]) -> Vec<DeclaredDep> {
    if own_ids.is_empty() {
        return Vec::new();
    }
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(jar_bytes)) else {
        return Vec::new();
    };
    // Collect entry names first to avoid borrow conflicts when reading bytes.
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.ends_with(".class"))
        .collect();
    for name in names {
        let Some(bytes) = entry_bytes(&mut zip, &name) else {
            continue;
        };
        let Some(at) = find_subslice(&bytes, b"required-after:")
            .or_else(|| find_subslice(&bytes, b"required-before:"))
        else {
            continue;
        };
        if !own_ids
            .iter()
            .any(|id| find_subslice(&bytes, id.as_bytes()).is_some())
        {
            continue;
        }
        return parse_legacy_dependency_string(printable_run(&bytes, at));
    }
    Vec::new()
}

/// `mcmod.info` — the Forge ≤ 1.12.2 descriptor. PROVIDERS ONLY.
///
/// Its `dependencies` array is cosmetic on that era: FML enforces the
/// `@Mod(dependencies = …)` annotation instead. Measured on a real 1.12.2 jar
/// that ships `"dependencies": []` alongside a genuine
/// `required-after:creativecore` annotation — reading this array as the
/// requirement list would report the opposite of the truth.
///
/// Two shapes are in the wild: a bare array of mod objects, and
/// `{ "modList": [...] }`.
fn parse_mcmod_info_providers(json_text: &str, out: &mut ManifestDeps) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return;
    };
    let list = v
        .as_array()
        .or_else(|| v.get("modList").and_then(|m| m.as_array()));
    let Some(list) = list else { return };
    for m in list {
        let Some(id) = m.get("modid").and_then(|x| x.as_str()) else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        // `${version}` / `${mcversion}` are build placeholders Gradle was meant
        // to substitute. An unsubstituted one is not a version — recording it
        // would make every range comparison against this provider nonsense.
        let version = m
            .get("version")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with("${"))
            .map(str::to_string);
        out.provided.push(ProvidedMod {
            mod_id: id.to_string(),
            version,
        });
    }
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

    // NeoForge reads `neoforge.mods.toml` when it is present and ignores
    // `mods.toml`; match that instead of merging both, which duplicated every
    // provider and dependency of the ~4% of jars that ship the two files.
    let chosen = entry_text(&mut zip, "META-INF/neoforge.mods.toml")
        .map(|t| (t, forge_descriptor::Descriptor::NeoForge))
        .or_else(|| {
            entry_text(&mut zip, "META-INF/mods.toml")
                .map(|t| (t, forge_descriptor::Descriptor::LegacyForge))
        });
    if let Some((txt, descriptor)) = chosen {
        match forge_descriptor::parse(&txt, descriptor) {
            Some(d) => {
                for p in d.provided {
                    out.provided.push(ProvidedMod {
                        mod_id: p.mod_id,
                        version: p
                            .version
                            .and_then(|v| resolve_jar_version(&v, manifest.as_deref())),
                    });
                }
                out.deps.extend(d.deps);
            }
            // Not valid TOML — recover whatever the regex scan can see.
            None => parse_forge_manifest_regex(
                &txt,
                manifest.as_deref(),
                RangeFamily::Maven,
                descriptor,
                &mut out,
            ),
        }
    }
    if let Some(txt) = entry_text(&mut zip, "fabric.mod.json") {
        parse_fabric_manifest(&txt, &mut out);
    }
    if let Some(txt) = entry_text(&mut zip, "quilt.mod.json") {
        parse_quilt_manifest(&txt, &mut out);
    }
    // A provided mod-id is the same fact whichever descriptor it came from, so
    // this is era-neutral and unconditional. Requirements are not — those come
    // from the annotation, era-scoped, and are read separately.
    if let Some(txt) = entry_text(&mut zip, "mcmod.info") {
        parse_mcmod_info_providers(&txt, &mut out);
    }
    out.deps.retain(|d| !is_loader_or_mc(&d.dep_id));
    Ok(out)
}

/// Find the end of a TOML section block starting at `from` in `text`.
/// A new section begins with `[[` or `[` at the start of a line; we stop there.
/// Falls back to `text.len()` when no next section exists.
///
/// A *commented-out* header (`#[[dependencies…]]`) ends the block too. Without
/// that, a live block swallows the commented-out block below it and reads its
/// fields — which is how `type="incompatible"` came to be reported as a
/// required dependency (a `#mandatory = true` three lines down).
///
/// A bare `find('[')` would be wrong in the other direction: it ends the block
/// inside a quoted value such as `versionRange = "[1.3.51,)"`.
///
/// Known best-effort limitation: a TOML triple-quoted or multi-line string
/// value that begins a line with `[` is also treated as a new section header.
/// The TOML path (`forge_descriptor`) is immune; this fallback is not, and no
/// descriptor in a 128-file corpus does it.
fn toml_block_end(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'\n' && i + 1 < bytes.len() {
            let rest = &text[i + 1..];
            // Only spaces/tabs may sit between the `#` and the `[` — trimming
            // all whitespace would let a lone `#` comment line match a header
            // two lines further down and cut the block short.
            if rest.starts_with('[')
                || rest
                    .strip_prefix('#')
                    .is_some_and(|r| r.trim_start_matches([' ', '\t']).starts_with('['))
            {
                return i + 1;
            }
        }
        i += 1;
    }
    text.len()
}

/// True when the marker at `idx` is the first non-whitespace thing on its line,
/// i.e. a real header rather than a commented-out one. `#[[dependencies…]]`
/// must not open a block: a fully commented block otherwise yields a phantom
/// dependency (a top-50 mod ships exactly that, parking a JEI dep in comments).
fn marker_starts_line(text: &str, idx: usize) -> bool {
    text[..idx]
        .rsplit('\n')
        .next()
        .is_some_and(|prefix| prefix.trim().is_empty())
}

/// Blank out `#`-to-end-of-line comments, honouring TOML string literals so a
/// `#` inside a quoted value survives (a real descriptor ships an unsubstituted
/// `versionRange = "[#compatible#)"` placeholder, and another puts a URL
/// fragment in its `credits`).
///
/// Basic (`"`) and literal (`'`) strings both terminate at a newline as well as
/// at their quote, so a stray quote desyncs at most one line; and this runs per
/// block, so a desync inside a description cannot leak into a dependency block.
fn strip_toml_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut in_basic = false;
    let mut in_literal = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_basic => {
                out.push(c);
                escaped = true;
            }
            '"' if !in_literal => {
                in_basic = !in_basic;
                out.push(c);
            }
            '\'' if !in_basic => {
                in_literal = !in_literal;
                out.push(c);
            }
            '#' if !in_basic && !in_literal => {
                // Drop to end of line, keeping the newline so line-based logic
                // downstream still sees the same line structure.
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '\n' => {
                in_basic = false;
                in_literal = false;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Find the next real (line-initial, correctly delimited) occurrence of a
/// `[[<marker>` header at or after `from`. `[[dependenciesExtra.x]]` is not a
/// `[[dependencies` header, so the character after the marker must be `.` or
/// `]`.
fn next_block_marker(text: &str, marker: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some(rel) = text[cursor..].find(marker) {
        let at = cursor + rel;
        let after = text[at + marker.len()..].chars().next();
        if marker_starts_line(text, at) && matches!(after, Some('.') | Some(']')) {
            return Some(at);
        }
        cursor = at + marker.len();
    }
    None
}

/// Regex fallback for descriptors that are not valid TOML. The TOML reader in
/// [`crate::mods::forge_descriptor`] is the primary path; this recovers what it
/// can rather than losing every dependency of a malformed jar.
fn parse_forge_manifest_regex(
    text: &str,
    manifest: Option<&str>,
    family: RangeFamily,
    descriptor: forge_descriptor::Descriptor,
    out: &mut ManifestDeps,
) {
    // own [[mods]] id + version (resolve ${file.jarVersion})
    let mut mods_from = 0;
    while let Some(at) = next_block_marker(text, "[[mods", mods_from) {
        let start = at + "[[mods".len();
        let end = toml_block_end(text, start);
        let block = strip_toml_comments(&text[start..end]);
        if let Some(id) = FORGE_MODID_RE.captures(&block) {
            let version = FORGE_VERSION_RE
                .captures(&block)
                .and_then(|c| resolve_jar_version(&c[1], manifest));
            out.provided.push(ProvidedMod {
                mod_id: id[1].to_string(),
                version,
            });
        }
        mods_from = end;
    }
    let mut deps_from = 0;
    while let Some(at) = next_block_marker(text, "[[dependencies", deps_from) {
        let start = at + "[[dependencies".len();
        let end = toml_block_end(text, start);
        let block = strip_toml_comments(&text[start..end]);
        if let Some(id) = FORGE_MODID_RE.captures(&block) {
            let range = FORGE_VERSIONRANGE_RE
                .captures(&block)
                .map(|c| c[1].to_string())
                .unwrap_or_default();
            let side = match FORGE_SIDE_RE
                .captures(&block)
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
                kind: regex_dep_kind(&block, descriptor),
                side,
                family,
                source: descriptor.source(),
            });
        }
        deps_from = end;
    }
}

/// Key precedence for the regex path — the same rule the TOML reader applies,
/// documented in [`crate::mods::forge_descriptor`]: NeoForge reads only `type`,
/// MinecraftForge only `mandatory`.
fn regex_dep_kind(block: &str, descriptor: forge_descriptor::Descriptor) -> DependencyKind {
    let ty = FORGE_TYPE_RE.captures(block).map(|c| c[1].to_string());
    match descriptor {
        forge_descriptor::Descriptor::NeoForge => ty
            .as_deref()
            .map_or(DependencyKind::Required, forge_descriptor::kind_from_type),
        forge_descriptor::Descriptor::LegacyForge => {
            let mandatory = FORGE_MANDATORY_RE.captures(block).map(|c| &c[1] == "true");
            match (mandatory, ty.as_deref()) {
                (Some(true), _) => DependencyKind::Required,
                (Some(false), _) => DependencyKind::Optional,
                (None, Some(t)) => forge_descriptor::kind_from_type(t),
                (None, None) => DependencyKind::Required,
            }
        }
    }
}

fn parse_fabric_manifest(json_text: &str, out: &mut ManifestDeps) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return;
    };
    let version = v.get("version").and_then(|x| x.as_str()).map(String::from);
    if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
        out.provided.push(ProvidedMod {
            mod_id: id.to_string(),
            version: version.clone(),
        });
    }
    // `provides`: alias mod-ids this jar satisfies (Fabric: array of strings).
    // The alias inherits the declaring jar's version (best-effort).
    if let Some(arr) = v.get("provides").and_then(|p| p.as_array()) {
        for id in arr.iter().filter_map(|x| x.as_str()) {
            out.provided.push(ProvidedMod {
                mod_id: id.to_string(),
                version: version.clone(),
            });
        }
    }
    if let Some(obj) = v.get("depends").and_then(|d| d.as_object()) {
        for (id, val) in obj {
            out.deps.push(DeclaredDep {
                dep_id: id.clone(),
                range: predicate_value(val),
                kind: DependencyKind::Required,
                side: DepSide::Both,
                family: RangeFamily::FabricPredicate,
                source: DescriptorSource::FabricJson,
            });
        }
    }
}

fn parse_quilt_manifest(json_text: &str, out: &mut ManifestDeps) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return;
    };
    let ql = v.get("quilt_loader");
    let own_version = ql
        .and_then(|q| q.get("version"))
        .and_then(|x| x.as_str())
        .map(String::from);
    if let Some(id) = ql.and_then(|q| q.get("id")).and_then(|x| x.as_str()) {
        out.provided.push(ProvidedMod {
            mod_id: id.to_string(),
            version: own_version.clone(),
        });
    }
    // `provides`: strings (own version) or { id, version? } objects.
    if let Some(arr) = ql
        .and_then(|q| q.get("provides"))
        .and_then(|p| p.as_array())
    {
        for e in arr {
            let (id, ver) = match e {
                serde_json::Value::String(s) => (Some(s.clone()), own_version.clone()),
                serde_json::Value::Object(o) => (
                    o.get("id").and_then(|x| x.as_str()).map(String::from),
                    o.get("version")
                        .and_then(|x| x.as_str())
                        .map(String::from)
                        .or_else(|| own_version.clone()),
                ),
                _ => (None, own_version.clone()),
            };
            if let Some(id) = id {
                out.provided.push(ProvidedMod {
                    mod_id: id,
                    version: ver,
                });
            }
        }
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
                kind: if optional {
                    DependencyKind::Optional
                } else {
                    DependencyKind::Required
                },
                side: DepSide::Both,
                family: RangeFamily::QuiltPredicate,
                source: DescriptorSource::QuiltJson,
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
/// `pub(crate)` because `l10n::scan` also reads named entries out of a jar
/// (language files, discovered by path rather than known up front).
pub(crate) fn entry_bytes(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Read the JIJ (Jar-in-Jar) embedded jars from an outer jar's
/// `META-INF/jarjar/` (Forge) or `META-INF/jars/` (Fabric/Quilt)
/// directory. For each `.jar` found there, recursively
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
            // Match a nested jar one level under either nested-jar dir:
            // META-INF/jarjar/<x>.jar (Forge/NeoForge) or
            // META-INF/jars/<x>.jar (Fabric/Quilt). Both have exactly 2 slashes.
            if (name.starts_with("META-INF/jarjar/") || name.starts_with("META-INF/jars/"))
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
pub(crate) fn instance_family(loader: LoaderKind) -> Option<LoaderFamily> {
    match loader {
        LoaderKind::Fabric | LoaderKind::Quilt => Some(LoaderFamily::Fabric),
        LoaderKind::Forge | LoaderKind::NeoForge => Some(LoaderFamily::Forge),
        LoaderKind::Vanilla => None,
    }
}

/// True when `node_loaders` (a mod version's platform-declared loaders) belong
/// to a loader family the `instance` loader cannot load — i.e. the mod is inert
/// on this instance and its declared deps are never enforced at runtime.
/// Conservative (only ever suppress when confidently disjoint):
/// - empty/unknown loaders never suppress;
/// - a Vanilla *instance* never suppresses (it has no family);
/// - a node tagged `Vanilla` (Modrinth's "minecraft", loader-agnostic — e.g. a
///   datapack) is kept on any instance: it is not inert on a real loader;
/// - a multi-loader set that overlaps the instance family is kept.
/// This is the dependency-graph analogue of `preflight`'s `dep_applies_to_loader`
/// scoping (PR #154), applied one level up at declaring-node granularity —
/// platform metadata carries per-version loaders, not per-dependency families.
pub(crate) fn loaders_disjoint_from_instance(
    node_loaders: &[LoaderKind],
    instance: LoaderKind,
) -> bool {
    let Some(inf) = instance_family(instance) else {
        return false;
    };
    if node_loaders.is_empty() {
        return false;
    }
    // A Vanilla-tagged node is loader-agnostic (not inert on any loader) → keep.
    // Otherwise the node is inert here only if NO declared loader shares the
    // instance's family.
    !node_loaders
        .iter()
        .any(|l| *l == LoaderKind::Vanilla || instance_family(*l) == Some(inf))
}

/// True when `node_loaders` names at least one loader of a DIFFERENT family
/// than the instance — i.e. this release is a merged multi-loader build.
///
/// Such a release necessarily ships ONE platform dependency list covering every
/// loader it targets, because neither Modrinth nor CurseForge can scope a
/// dependency to a loader. That flat union is the whole cause of the phantom
/// "requires Fabric API" row on a NeoForge instance, and this predicate is how
/// the dependency graph recognises the situation.
///
/// It exists to BOUND a weaker signal. Judging a child by its project-level
/// loader union is a proxy, and a proxy is only safe where the metadata is
/// provably ambiguous. A single-family release declares unambiguous
/// dependencies; adjudicating those with a coarse union could only invent false
/// negatives. Measured on a 140-mod instance: gated, 2 of 115 dependency rows
/// are adjudicated (both genuine phantoms); ungated, all 115 are — to remove
/// the same 2.
///
/// A `Vanilla` tag is loader-agnostic and never counts as foreign, and a
/// Vanilla *instance* has no family, so nothing is ever ambiguous there.
pub(crate) fn spans_foreign_family(node_loaders: &[LoaderKind], instance: LoaderKind) -> bool {
    let Some(inf) = instance_family(instance) else {
        return false;
    };
    node_loaders
        .iter()
        .any(|l| instance_family(*l).is_some_and(|f| f != inf))
}

/// The mod-id Sinytra Connector declares.
const CONNECTOR_MOD_ID: &str = "connector";

/// Cheap pre-filter deciding WHICH jar is worth opening to look for Sinytra
/// Connector. Only a candidate selector — [`jar_is_connector`] is the proof.
/// Matches the filename and the registry display name, so it catches the
/// platform installs ("Sinytra Connector") and the hand-dropped file
/// (`connector-2.0.0-beta.14+1.21.1-full.jar`) alike. A jar renamed to contain
/// neither word is missed, which fails CLOSED — we keep reporting exactly what
/// we report today — rather than silently absolving everything.
pub(crate) fn looks_like_connector(filename: &str, display_name: &str) -> bool {
    let hit = |s: &str| {
        let low = s.to_ascii_lowercase();
        low.contains("connector") || low.contains("sinytra")
    };
    hit(filename) || hit(display_name)
}

/// True when these jar bytes ARE Sinytra Connector, proven by the `connector`
/// mod-id rather than by a filename.
///
/// Both the jar's own descriptor and its Jar-in-Jar payload are consulted, and
/// the JIJ half is not optional: Connector ships as `connector-<v>-full.jar`,
/// a container with **no descriptor of its own** whose real mod sits at
/// `META-INF/jarjar/org.sinytra.connector-<v>-mod.jar`. Checking only the top
/// level finds nothing at all (verified against the shipping 2.0.0-beta.14 jar).
pub(crate) fn jar_is_connector(jar_bytes: &[u8]) -> bool {
    let is_connector = |p: &ProvidedMod| p.mod_id.eq_ignore_ascii_case(CONNECTOR_MOD_ID);
    if read_jar_manifest_deps(jar_bytes)
        .map(|d| d.provided)
        .unwrap_or_default()
        .iter()
        .any(is_connector)
    {
        return true;
    }
    read_jar_embedded_providers(jar_bytes)
        .iter()
        .any(is_connector)
}

/// Judge a jar's loader/MC compatibility with an instance. Conservative:
/// a mismatch is reported only when both sides are confidently known and
/// they differ — absent or ambiguous metadata never produces a warning.
///
/// `connector_present` is that conservatism applied to one real exception.
/// Sinytra Connector remaps Fabric mods and loads them on a Forge-family
/// instance, so there a Fabric-family jar is NOT dead weight. Measured on a
/// real NeoForge 1.21.1 profile: FML logs
/// `Skipping jar. File mods\eg-inventory-blur-1.0.1.jar is a Fabric mod and
/// cannot be loaded`, and eight seconds later `ConnectorLocator` picks the very
/// same file up remapped, after which the mod appears in the loaded-mod list
/// and contributes resources. Without this flag the launcher put an
/// «Несовместим» badge on every Fabric jar of such a profile — the loudest
/// false positive in the whole detection surface.
///
/// The exception is deliberately one-directional and narrow: only a
/// Fabric-family jar, only on a Forge-family instance, only when Connector is
/// actually installed. Connector cannot remap everything, so its presence means
/// we are no longer confident the jar is inert — and this module's rule for
/// "not confident" is to stay silent, not to assert the opposite.
pub fn compat_verdict(
    jar: &JarMeta,
    instance_loader: LoaderKind,
    instance_mc: &str,
    connector_present: bool,
) -> CompatVerdict {
    // Mismatch only when the jar declares loader families AND none of them is
    // the instance's family. A multi-loader jar that includes the instance's
    // family is compatible; a descriptor-less jar (empty families) never flags.
    let loader_mismatch = match instance_family(instance_loader) {
        Some(inf) => {
            !jar.families.is_empty()
                && !jar.families.contains(&inf)
                && !(connector_present
                    && inf == LoaderFamily::Forge
                    && jar.families.contains(&LoaderFamily::Fabric))
        }
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
/// Layer 1 of the proactive incompatibility check: judges EVERY mod —
/// hand-dropped (`source = None`), platform-installed, and pack-bundled — by
/// reading its descriptor and comparing loader families against the instance.
/// Pack membership does not guarantee the right family (FCAP: a Forge pack
/// shipped a pure-Fabric jar the loader silently ignores), so pack mods are
/// judged too; they are not live-checkable (`eligible_identity` excludes
/// them), which makes their descriptor verdict final — the same evidence
/// standard as import-time inert-jar detection. For platform mods the offline
/// verdict is only a SUSPECT pre-filter — the `live_checkable` flag tells the
/// frontend to auto-run an authoritative live check on those. Network-free.
/// A mod whose jar is missing/unreadable, or has no recognised descriptor,
/// yields `loader_mismatch = false` (conservative — never a false alarm).
/// `mc` is passed to `compat_verdict` (its signature needs it) but only the
/// loader outputs are surfaced.
pub async fn scan_instance(
    instance_root: &Path,
    instance_loader: LoaderKind,
    mc: &str,
) -> Result<Vec<ModLocalCompat>, Error> {
    use crate::mods::updates::eligible_identity;
    let mods = installed::list(instance_root).await?;
    let pack_origin = installed::get_pack_origin(instance_root).await?;
    let dir = installed::mods_dir(instance_root);
    // Once for the whole scan, not per jar: on a Connector profile every Fabric
    // jar's verdict depends on this one fact.
    let connector = connector_installed(&dir, &mods).await;
    let mut out = Vec::with_capacity(mods.len());
    for m in &mods {
        // Judge loader-family for ALL mods, pack-bundled included — the
        // conservative verdict (descriptor-less / family-inclusive jars never
        // flag) is the false-positive guard, not pack membership.
        let verdict = read_jar_for(&dir, &m.filename)
            .await
            .and_then(|bytes| read_jar_meta(&bytes).ok())
            .map(|meta| compat_verdict(&meta, instance_loader, mc, connector));
        out.push(ModLocalCompat {
            sha1: m.sha1.clone(),
            loader_mismatch: verdict.as_ref().map(|v| v.loader_mismatch).unwrap_or(false),
            detected_loader: verdict.and_then(|v| v.detected_loader),
            live_checkable: eligible_identity(m, pack_origin.as_ref()).is_some(),
        });
    }
    Ok(out)
}

/// True when an ENABLED jar in `mods_dir` is Sinytra Connector.
///
/// Only candidate jars are opened (see [`looks_like_connector`]) — proving this
/// by scanning all ~140 jars' Jar-in-Jar payloads would cost more than the
/// entire compat scan. A DISABLED Connector does not count: it does not load,
/// so it remaps nothing.
pub(crate) async fn connector_installed(mods_dir: &Path, mods: &[InstalledMod]) -> bool {
    for m in mods.iter().filter(|m| m.enabled) {
        if !looks_like_connector(&m.filename, &m.name) {
            continue;
        }
        if let Some(bytes) = read_jar_for(mods_dir, &m.filename).await {
            if jar_is_connector(&bytes) {
                return true;
            }
        }
    }
    false
}

/// [`connector_installed`] for a caller that holds only the instance root.
pub async fn instance_has_connector(instance_root: &Path) -> bool {
    let Ok(mods) = installed::list(instance_root).await else {
        return false;
    };
    connector_installed(&installed::mods_dir(instance_root), &mods).await
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
        // A hand-dropped jar exists nowhere else, so it is never linked:
        // corruption of shared bytes would be unrecoverable, where a
        // platform-sourced mod is only a re-download away. `place_bytes` still
        // gives the temp-then-rename guarantee.
        crate::mods::store::place_bytes(&dest, bytes)
            .await
            .map_err(|e| Error::ModsInstancePath {
                path: e.path.display().to_string(),
                details: e.details(),
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

    #[test]
    fn loader_scope_matrix() {
        use LoaderKind::*;
        // Forge instance: a Fabric-family declaring node is inert → suppress.
        // `spans_foreign_family` — the ambiguity gate. True only when the node
        // targets a family this instance does not run, i.e. a merged build.
        assert!(
            !spans_foreign_family(&[NeoForge], NeoForge),
            "single family"
        );
        assert!(
            !spans_foreign_family(&[Forge, NeoForge], NeoForge),
            "same family"
        );
        assert!(
            spans_foreign_family(&[Fabric, NeoForge], NeoForge),
            "merged"
        );
        assert!(spans_foreign_family(&[Fabric], NeoForge), "wholly foreign");
        assert!(
            !spans_foreign_family(&[], NeoForge),
            "no tags, nothing ambiguous"
        );
        assert!(
            !spans_foreign_family(&[Vanilla], NeoForge),
            "Vanilla is agnostic"
        );
        assert!(
            !spans_foreign_family(&[Vanilla, NeoForge], NeoForge),
            "Vanilla never makes a same-family node ambiguous"
        );
        assert!(
            !spans_foreign_family(&[Fabric, Forge], Vanilla),
            "no instance family"
        );
        assert!(
            !spans_foreign_family(&[Fabric], Quilt),
            "Quilt and Fabric are one family"
        );
        assert!(spans_foreign_family(&[Forge], Quilt));

        assert!(loaders_disjoint_from_instance(&[Fabric], Forge));
        assert!(loaders_disjoint_from_instance(&[Quilt], Forge));
        // Forge instance: Forge / multi-loader incl. Forge / NeoForge (same family) → keep.
        assert!(!loaders_disjoint_from_instance(&[Forge], Forge));
        assert!(!loaders_disjoint_from_instance(&[Forge, Fabric], Forge));
        assert!(!loaders_disjoint_from_instance(&[NeoForge], Forge));
        // Fabric instance.
        assert!(!loaders_disjoint_from_instance(&[Fabric], Fabric));
        assert!(loaders_disjoint_from_instance(&[Forge], Fabric));
        // Quilt runs Fabric mods → keep Fabric; reject Forge.
        assert!(!loaders_disjoint_from_instance(&[Fabric], Quilt));
        assert!(!loaders_disjoint_from_instance(&[Quilt], Quilt));
        assert!(loaders_disjoint_from_instance(&[Forge], Quilt));
        // Unknown/empty loaders → conservative keep.
        assert!(!loaders_disjoint_from_instance(&[], Forge));
        assert!(!loaders_disjoint_from_instance(&[], Fabric));
        // Vanilla instance has no family → never suppress.
        assert!(!loaders_disjoint_from_instance(&[Fabric], Vanilla));
        assert!(!loaders_disjoint_from_instance(&[Forge], Vanilla));
        // A node tagged Vanilla (Modrinth "minecraft", loader-agnostic) is kept
        // on any instance — it is not inert on a real loader.
        assert!(!loaders_disjoint_from_instance(&[Vanilla], Forge));
        assert!(!loaders_disjoint_from_instance(&[Vanilla], Fabric));
        assert!(!loaders_disjoint_from_instance(&[Vanilla, Fabric], Forge));
    }

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

    /// A jar with one entry whose bytes are not necessarily valid UTF-8.
    fn jar_raw(name: &str, body: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            w.start_file(name, SimpleFileOptions::default()).unwrap();
            w.write_all(body).unwrap();
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn descriptor_era_splits_at_the_fml_rewrite() {
        use DescriptorEra::*;
        assert_eq!(descriptor_era("1.12.2"), Legacy);
        assert_eq!(descriptor_era("1.7.10"), Legacy);
        assert_eq!(descriptor_era("1.13"), Modern);
        assert_eq!(descriptor_era("1.20.1"), Modern);
        assert_eq!(descriptor_era("1.21.1"), Modern);
        // A future major is modern by construction, not by luck — MC 26.x is real.
        assert_eq!(descriptor_era("26.1"), Modern);
        // Unparseable → today's behaviour, never a silent downgrade to legacy.
        assert_eq!(descriptor_era(""), Modern);
        assert_eq!(descriptor_era("21w13a"), Modern);
    }

    #[test]
    fn legacy_dependency_string_parses_only_real_requirements() {
        let out = parse_legacy_dependency_string(
            "required-after:creativecore@[2.0,);after:jei;required-before:foo;before:*",
        );
        assert_eq!(out.len(), 2, "only the two required-* clauses: {out:?}");
        assert_eq!(out[0].dep_id, "creativecore");
        assert_eq!(out[0].range, "[2.0,)");
        assert_eq!(out[0].kind, DependencyKind::Required);
        assert_eq!(out[0].family, RangeFamily::Maven);
        assert_eq!(out[0].source, DescriptorSource::McmodAnnotation);
        assert_eq!(out[1].dep_id, "foo");
        assert_eq!(out[1].range, "", "no @ means no range");
    }

    #[test]
    fn legacy_dependency_string_ignores_junk() {
        assert!(parse_legacy_dependency_string("").is_empty());
        assert!(parse_legacy_dependency_string("garbage").is_empty());
        assert!(parse_legacy_dependency_string("required-after:*").is_empty());
        assert!(parse_legacy_dependency_string("required-after:").is_empty());
    }

    /// Nearly every 1.12.2 `@Mod` annotation opens with `required-after:forge` and
    /// `required-after:minecraft`. Those are the loader and the game, not mods —
    /// letting them through would put a "forge is not installed" violation on
    /// every jar of every legacy pack, which is the inverse of the bug being fixed.
    #[test]
    fn legacy_dependency_string_drops_the_loader_and_minecraft() {
        let out = parse_legacy_dependency_string(
            "required-after:forge@[14.23.5.2768,);required-after:minecraft@[1.12.2];\
             required-after:creativecore",
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].dep_id, "creativecore");
        assert!(
            parse_legacy_dependency_string("required-after:FML;required-after:forge").is_empty()
        );
    }

    #[test]
    fn legacy_jar_scan_finds_the_annotation_and_guards_against_mentions() {
        // A class constant pool is just bytes; the annotation value survives as a
        // contiguous printable run, which is how `required-after:creativecore` was
        // extracted from a real EnhancedVisuals.class.
        let own = b"\x00\x0fenhancedvisuals\x00\x1brequired-after:creativecore\x00";
        let deps = read_jar_legacy_deps(
            &jar_raw("team/creative/EV.class", own),
            &["enhancedvisuals".to_string()],
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].dep_id, "creativecore");
        assert_eq!(deps[0].source, DescriptorSource::McmodAnnotation);

        // A class that merely MENTIONS the syntax without carrying the jar's own
        // mod-id is not the @Mod class — e.g. a bundled copy of a loader utility.
        let foreign = b"\x00\x1brequired-after:creativecore\x00";
        assert!(read_jar_legacy_deps(
            &jar_raw("some/lib/Parser.class", foreign),
            &["enhancedvisuals".to_string()]
        )
        .is_empty());

        // Non-class entries are never scanned.
        assert!(read_jar_legacy_deps(
            &jar_raw("assets/readme.txt", own),
            &["enhancedvisuals".to_string()]
        )
        .is_empty());

        // A jar that declares no mod-id of its own cannot be guarded, so it is
        // never scanned at all.
        assert!(read_jar_legacy_deps(&jar_raw("team/creative/EV.class", own), &[]).is_empty());

        // The @Mod class whose only clauses are loader/MC ends the scan and
        // contributes nothing — it must not fall through to another class.
        let loader_only = b"\x00\x0fenhancedvisuals\x00\x14required-after:forge\x00";
        assert!(read_jar_legacy_deps(
            &jar_raw("team/creative/EV.class", loader_only),
            &["enhancedvisuals".to_string()]
        )
        .is_empty());
    }

    /// Every declared dependency must carry the file it came out of. `RangeFamily`
    /// cannot answer this — `Maven` covers `mods.toml`, `neoforge.mods.toml` and
    /// the legacy annotation alike — and the pre-flight needs it to drop a
    /// declaration from a descriptor the instance's loader never opens.
    #[test]
    fn declared_deps_carry_their_descriptor_source() {
        let toml = "modLoader=\"javafml\"\n[[mods]]\nmodId=\"a\"\n\
                    [[dependencies.a]]\nmodId=\"b\"\nmandatory=true\nversionRange=\"[1,)\"\n";

        let d = read_jar_manifest_deps(&jar(&[("META-INF/mods.toml", toml)])).unwrap();
        assert_eq!(d.deps[0].source, DescriptorSource::ModsToml);

        let d = read_jar_manifest_deps(&jar(&[("META-INF/neoforge.mods.toml", toml)])).unwrap();
        assert_eq!(d.deps[0].source, DescriptorSource::NeoForgeToml);

        let d = read_jar_manifest_deps(&jar(&[(
            "fabric.mod.json",
            r#"{"id":"a","depends":{"b":">=1.0.0"}}"#,
        )]))
        .unwrap();
        assert_eq!(d.deps[0].source, DescriptorSource::FabricJson);

        let d = read_jar_manifest_deps(&jar(&[(
            "quilt.mod.json",
            r#"{"quilt_loader":{"id":"a","depends":[{"id":"b","versions":"*"}]}}"#,
        )]))
        .unwrap();
        assert_eq!(d.deps[0].source, DescriptorSource::QuiltJson);
    }

    /// `mcmod.info` is the Forge ≤ 1.12.2 descriptor. 44 of the 50 jars in a
    /// measured 1.12.2 pack ship it and nothing else, so without this the
    /// pre-flight sees no provider for any of them.
    #[test]
    fn mcmod_info_registers_its_providers() {
        // The shape CreativeCore_v1.10.71_mc1.12.2.jar actually ships.
        let bare = r#"[{"modid":"creativecore","name":"CreativeCore","version":"1.10"}]"#;
        let d = read_jar_manifest_deps(&jar(&[("mcmod.info", bare)])).unwrap();
        assert_eq!(d.provided.len(), 1);
        assert_eq!(d.provided[0].mod_id, "creativecore");
        assert_eq!(d.provided[0].version.as_deref(), Some("1.10"));

        // The other shape in the wild.
        let wrapped = r#"{"modList":[{"modid":"x","version":"2.0"}]}"#;
        let d = read_jar_manifest_deps(&jar(&[("mcmod.info", wrapped)])).unwrap();
        assert_eq!(d.provided[0].mod_id, "x");

        // An unresolved build placeholder is not a version — recording it would
        // make every range comparison against this provider nonsense.
        let ph = r#"[{"modid":"y","version":"${version}"}]"#;
        let d = read_jar_manifest_deps(&jar(&[("mcmod.info", ph)])).unwrap();
        assert_eq!(d.provided[0].version, None);

        // mcmod.info's own `dependencies` array is cosmetic on this era: FML
        // enforces the @Mod annotation instead, and a measured jar ships
        // `"dependencies": []` alongside a genuine `required-after:` clause.
        let deps = r#"[{"modid":"z","dependencies":["something"]}]"#;
        let d = read_jar_manifest_deps(&jar(&[("mcmod.info", deps)])).unwrap();
        assert!(
            d.deps.is_empty(),
            "mcmod.info declares no enforced dependency"
        );

        // Junk must not panic or invent a provider.
        let junk = read_jar_manifest_deps(&jar(&[("mcmod.info", "not json")])).unwrap();
        assert!(junk.provided.is_empty());
        let noid = read_jar_manifest_deps(&jar(&[("mcmod.info", r#"[{"name":"a"}]"#)])).unwrap();
        assert!(noid.provided.is_empty());
    }

    /// The regex fallback runs on descriptors that are not valid TOML, and must
    /// stamp the same provenance the TOML path would.
    #[test]
    fn the_regex_fallback_stamps_the_descriptor_source_too() {
        let broken = "[[mods]]\nlogoFile=\"assets\\icon.png\"\n\
                      [[dependencies.a]]\nmodId=\"b\"\nmandatory=true\n";
        let d = read_jar_manifest_deps(&jar(&[("META-INF/mods.toml", broken)])).unwrap();
        assert_eq!(d.deps[0].source, DescriptorSource::ModsToml);

        let d = read_jar_manifest_deps(&jar(&[("META-INF/neoforge.mods.toml", broken)])).unwrap();
        assert_eq!(d.deps[0].source, DescriptorSource::NeoForgeToml);
    }

    // ── regex fallback (only reached when a descriptor is not valid TOML) ──

    fn regex_parse(text: &str, descriptor: forge_descriptor::Descriptor) -> ManifestDeps {
        let mut out = ManifestDeps::default();
        parse_forge_manifest_regex(text, None, RangeFamily::Maven, descriptor, &mut out);
        out
    }

    #[test]
    fn regex_fallback_ignores_a_fully_commented_dependency_block() {
        // A top-50 mod parks a JEI dependency in comments exactly like this.
        // `find("[[dependencies")` matches inside `#[[dependencies…`, so the
        // scan has to refuse a header that is not line-initial.
        let toml = "[[mods]]\nmodId=\"ars_nouveau\"\n\n\
            #[[dependencies.ars_nouveau]]\n#    modId=\"jei\"\n#    mandatory=true\n\
            #    versionRange=\"[1.19-5.0.7.1,)\"\n";
        let out = regex_parse(toml, forge_descriptor::Descriptor::NeoForge);
        assert!(
            out.deps.is_empty(),
            "a commented block must not produce a dep: {:?}",
            out.deps
        );
    }

    #[test]
    fn regex_fallback_does_not_let_a_comment_leak_into_the_live_block_above_it() {
        let toml = "[[dependencies.ap]]\nmodId=\"create\"\ntype=\"incompatible\"\n\
            versionRange=\"(,6.0.9]\"\n\n#[[dependencies.ap]]\n#modId=\"mixinextras\"\n\
            #mandatory=true\n";
        let out = regex_parse(toml, forge_descriptor::Descriptor::NeoForge);
        assert_eq!(out.deps.len(), 1);
        assert_eq!(out.deps[0].dep_id, "create");
        assert_eq!(out.deps[0].kind, DependencyKind::Incompatible);
    }

    #[test]
    fn regex_fallback_keeps_a_hash_inside_a_quoted_value() {
        // A real descriptor ships an unsubstituted "[#compatible#)" placeholder.
        let toml = "[[dependencies.m]]\nmodId=\"a\"\nversionRange=\"[#PLACEHOLDER#,)\"\n";
        let out = regex_parse(toml, forge_descriptor::Descriptor::NeoForge);
        assert_eq!(out.deps[0].range, "[#PLACEHOLDER#,)");
    }

    #[test]
    fn regex_fallback_survives_the_mdk_trailing_comment_style() {
        // The Forge MDK template annotates nearly every line with #mandatory.
        let toml = "[[dependencies.m]] #mandatory\n    modId=\"alexsmobs\" #mandatory\n\
                mandatory=false #optional here\n";
        let out = regex_parse(toml, forge_descriptor::Descriptor::LegacyForge);
        assert_eq!(out.deps.len(), 1);
        assert_eq!(out.deps[0].kind, DependencyKind::Optional);
    }

    #[test]
    fn regex_fallback_keeps_reading_a_block_past_a_plain_comment_line() {
        // A lone `#` line must not be mistaken for a commented header and cut
        // the block short — `side` is declared after it and must survive.
        let toml = "[[dependencies.m]]\nmodId=\"a\"\n# just explaining things\nside=\"SERVER\"\n";
        let out = regex_parse(toml, forge_descriptor::Descriptor::NeoForge);
        assert_eq!(out.deps[0].side, DepSide::Server);
    }

    #[test]
    fn regex_fallback_does_not_open_a_block_on_a_similar_prefix() {
        let toml = "[[dependenciesExtra.m]]\nmodId=\"ghost\"\n";
        let out = regex_parse(toml, forge_descriptor::Descriptor::NeoForge);
        assert!(out.deps.is_empty());
    }

    #[test]
    fn regex_fallback_applies_legacy_precedence() {
        // MinecraftForge reads only `mandatory`; a pasted NeoForge `type` must
        // not downgrade a dependency the loader still enforces.
        let toml = "[[dependencies.m]]\nmodId=\"curios\"\nmandatory=true\ntype=\"optional\"\n";
        let legacy = regex_parse(toml, forge_descriptor::Descriptor::LegacyForge);
        assert_eq!(legacy.deps[0].kind, DependencyKind::Required);
        let neo = regex_parse(toml, forge_descriptor::Descriptor::NeoForge);
        assert_eq!(neo.deps[0].kind, DependencyKind::Optional);
    }

    #[test]
    fn jar_with_both_descriptors_yields_each_dep_once() {
        // ~4% of real jars ship both files. NeoForge reads only its own, so we
        // must not merge them and report everything twice.
        let bytes = jar(&[
            (
                "META-INF/mods.toml",
                "[[mods]]\nmodId=\"m\"\nversion=\"1.0\"\n\
                 [[dependencies.m]]\nmodId=\"curios\"\nmandatory=true\n",
            ),
            (
                "META-INF/neoforge.mods.toml",
                "[[mods]]\nmodId=\"m\"\nversion=\"1.0\"\n\
                 [[dependencies.m]]\nmodId=\"curios\"\ntype=\"optional\"\n",
            ),
        ]);
        let out = read_jar_manifest_deps(&bytes).unwrap();
        assert_eq!(out.deps.len(), 1, "deps: {:?}", out.deps);
        assert_eq!(
            out.deps[0].kind,
            DependencyKind::Optional,
            "neoforge.mods.toml must win"
        );
        assert_eq!(out.provided.len(), 1);
    }

    #[test]
    fn descriptor_with_a_non_utf8_byte_still_yields_its_deps() {
        let mut bytes = b"[[mods]]\nmodId=\"m\"\nauthors=\"".to_vec();
        bytes.push(0xE9); // cp1252 'e-acute'
        bytes.extend_from_slice(b"\"\n\n[[dependencies.m]]\nmodId=\"curios\"\n");
        let j = jar_raw("META-INF/neoforge.mods.toml", &bytes);
        let out = read_jar_manifest_deps(&j).unwrap();
        assert_eq!(
            out.deps.len(),
            1,
            "one bad byte must not discard the descriptor"
        );
    }

    #[test]
    fn invalid_toml_descriptor_falls_back_to_the_regex_scan() {
        // An unescaped backslash in a Windows path is a real authoring mistake:
        // the strict parse rejects the file, the regex recovers the dep.
        let bytes = jar(&[(
            "META-INF/neoforge.mods.toml",
            "[[mods]]\nmodId=\"m\"\nlogoFile=\"assets\\icon.png\"\n\n\
             [[dependencies.m]]\nmodId=\"curios\"\ntype=\"required\"\nversionRange=\"[9.0,)\"\n",
        )]);
        let out = read_jar_manifest_deps(&bytes).unwrap();
        assert_eq!(out.deps.len(), 1);
        assert_eq!(out.deps[0].dep_id, "curios");
        assert_eq!(out.deps[0].range, "[9.0,)");
    }

    #[test]
    fn dependency_ids_exclude_a_non_required_declaration() {
        let bytes = jar(&[(
            "META-INF/neoforge.mods.toml",
            "[[mods]]\nmodId=\"ap\"\n\n[[dependencies.ap]]\nmodId=\"create\"\n\
             type=\"incompatible\"\nversionRange=\"(,6.0.9]\"\n",
        )]);
        let ids = read_jar_dependency_ids(&bytes).unwrap();
        assert!(
            ids.is_empty(),
            "an incompatible dep is not something the jar needs: {ids:?}"
        );
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
        let deps = read_jar_dependency_ids(&jar(&[("META-INF/mods.toml", toml)])).unwrap();
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
    fn fabric_depends_become_required_deps() {
        let json = r#"{"depends":{"minecraft":">=1.20.1","fabricloader":">=0.15","sodium":"*"}}"#;
        let out = read_jar_manifest_deps(&jar(&[("fabric.mod.json", json)])).unwrap();
        // Loader/MC ids are dropped by read_jar_manifest_deps itself.
        assert_eq!(out.deps.len(), 1);
        assert_eq!(out.deps[0].dep_id, "sodium");
        assert_eq!(out.deps[0].kind, DependencyKind::Required);
    }

    #[test]
    fn quilt_depends_strings_and_objects() {
        let json = r#"{"quilt_loader":{"depends":["sodium",{"id":"alexsmobs","versions":"*"}]}}"#;
        let out = read_jar_manifest_deps(&jar(&[("quilt.mod.json", json)])).unwrap();
        let ids: Vec<&str> = out.deps.iter().map(|d| d.dep_id.as_str()).collect();
        assert_eq!(ids, vec!["sodium", "alexsmobs"]);
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

    /// Named so a reader of the assertions below knows which world they are in:
    /// every one of these cases is a plain profile with no Sinytra Connector.
    const NO_CONNECTOR: bool = false;

    #[test]
    fn verdict_compatible_when_loader_and_mc_match() {
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Forge], Some("1.12")),
            LoaderKind::Forge,
            "1.12.2",
            NO_CONNECTOR,
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
            NO_CONNECTOR,
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
            NO_CONNECTOR,
        );
        assert!(!v.loader_mismatch);
    }

    #[test]
    fn verdict_flags_mc_mismatch() {
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Forge], Some("1.20")),
            LoaderKind::Forge,
            "1.12.2",
            NO_CONNECTOR,
        );
        assert!(v.mc_mismatch);
    }

    #[test]
    fn verdict_silent_when_metadata_absent() {
        // No descriptor at all — never warn.
        let v = compat_verdict(
            &meta(vec![], None),
            LoaderKind::Forge,
            "1.12.2",
            NO_CONNECTOR,
        );
        assert!(!v.loader_mismatch);
        assert!(!v.mc_mismatch);
    }

    #[test]
    fn verdict_silent_when_jar_mc_unknown() {
        let v = compat_verdict(
            &meta(vec![LoaderFamily::Forge], None),
            LoaderKind::Forge,
            "1.12.2",
            NO_CONNECTOR,
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
        assert!(!compat_verdict(&jar, LoaderKind::Forge, "1.20.4", NO_CONNECTOR).loader_mismatch);
        assert!(!compat_verdict(&jar, LoaderKind::Fabric, "1.20.4", NO_CONNECTOR).loader_mismatch);
    }

    // ── Sinytra Connector ──────────────────────────────────────────────────

    #[test]
    fn connector_absolves_a_fabric_jar_on_a_forge_family_instance() {
        // The measured case: two Fabric-only jars in a NeoForge 1.21.1 profile
        // that FML logs as "Skipping jar … cannot be loaded" and ConnectorLocator
        // then loads anyway. Flagging them was the loudest false positive in the
        // detection surface.
        let jar = meta(vec![LoaderFamily::Fabric], None);
        assert!(compat_verdict(&jar, LoaderKind::NeoForge, "1.21.1", NO_CONNECTOR).loader_mismatch);
        assert!(!compat_verdict(&jar, LoaderKind::NeoForge, "1.21.1", true).loader_mismatch);
        assert!(!compat_verdict(&jar, LoaderKind::Forge, "1.21.1", true).loader_mismatch);
    }

    #[test]
    fn connector_does_not_absolve_the_other_direction() {
        // Connector runs Fabric mods on NeoForge, never the reverse. A Forge
        // jar in a Fabric profile stays a mismatch even with Connector present
        // (which cannot itself load there, but the flag must not be a blanket
        // amnesty regardless).
        let forge_jar = meta(vec![LoaderFamily::Forge], None);
        assert!(compat_verdict(&forge_jar, LoaderKind::Fabric, "1.21.1", true).loader_mismatch);
        assert!(compat_verdict(&forge_jar, LoaderKind::Quilt, "1.21.1", true).loader_mismatch);
    }

    #[test]
    fn connector_changes_nothing_when_the_jar_already_matches() {
        // Not a blanket "everything is fine" switch: the flag only ever removes
        // the Fabric-on-Forge-family verdict, and MC mismatch is untouched.
        let forge_jar = meta(vec![LoaderFamily::Forge], Some("1.20"));
        let v = compat_verdict(&forge_jar, LoaderKind::NeoForge, "1.21.1", true);
        assert!(!v.loader_mismatch);
        assert!(v.mc_mismatch, "MC mismatch is a separate axis");
    }

    #[test]
    fn looks_like_connector_selects_candidates_and_fails_closed() {
        assert!(looks_like_connector(
            "connector-2.0.0-beta.14+1.21.1-full.jar",
            ""
        ));
        assert!(looks_like_connector("whatever.jar", "Sinytra Connector"));
        // A near neighbour is a candidate too — cheap, and `jar_is_connector`
        // rejects it on the mod-id.
        assert!(looks_like_connector(
            "ConnectorExtras-1.12.1+1.21.1.jar",
            ""
        ));
        // Unrelated jars are never opened.
        assert!(!looks_like_connector("sodium-0.6.0.jar", "Sodium"));
        // Renamed beyond recognition → missed → we keep today's behaviour.
        assert!(!looks_like_connector("aaa.jar", ""));
    }

    #[test]
    fn jar_is_connector_finds_the_id_inside_the_jarjar_container() {
        // THE shipping shape: `connector-<v>-full.jar` has NO descriptor of its
        // own; the real mod is at META-INF/jarjar/…-mod.jar. A top-level check
        // finds nothing, so this is the case that has to work.
        let inner = zip_with(&[(
            "META-INF/neoforge.mods.toml",
            b"modLoader=\"javafml\"\nloaderVersion=\"*\"\n[[mods]]\nmodId=\"connector\"\n"
                as &[u8],
        )]);
        let container = zip_with(&[(
            "META-INF/jarjar/org.sinytra.connector-2.0.0-mod.jar",
            inner.as_slice(),
        )]);
        assert!(jar_is_connector(&container));

        // Declared directly, for a non-fat build.
        let direct = zip_with(&[(
            "META-INF/neoforge.mods.toml",
            b"modLoader=\"javafml\"\nloaderVersion=\"*\"\n[[mods]]\nmodId=\"connector\"\n"
                as &[u8],
        )]);
        assert!(jar_is_connector(&direct));
    }

    #[test]
    fn jar_is_connector_rejects_a_lookalike() {
        // ConnectorExtras ships alongside Connector and passes the filename
        // filter; only the mod-id separates them.
        let extras = zip_with(&[(
            "META-INF/neoforge.mods.toml",
            b"modLoader=\"javafml\"\nloaderVersion=\"*\"\n[[mods]]\nmodId=\"connectorextras\"\n"
                as &[u8],
        )]);
        assert!(!jar_is_connector(&extras));
        assert!(!jar_is_connector(&zip_with(&[(
            "fabric.mod.json",
            br#"{"id":"sodium"}"# as &[u8]
        )])));
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

    /// The Connector jar as it actually ships: a JIJ container with no
    /// descriptor of its own.
    fn connector_container_bytes() -> Vec<u8> {
        let inner = zip_with(&[(
            "META-INF/neoforge.mods.toml",
            b"modLoader=\"javafml\"\nloaderVersion=\"*\"\n[[mods]]\nmodId=\"connector\"\n"
                as &[u8],
        )]);
        zip_with(&[(
            "META-INF/jarjar/org.sinytra.connector-2.0.0-mod.jar",
            inner.as_slice(),
        )])
    }

    /// End-to-end for the Connector exception: the flag has to travel from the
    /// jars on disk, through `connector_installed`, into `compat_verdict`.
    /// Without this the six pure-helper tests all pass while `scan_instance`
    /// hard-codes `false` — the whole fix deleted, `cargo test` green.
    #[tokio::test]
    async fn scan_does_not_flag_a_fabric_jar_when_connector_is_installed() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let fabric = zip_with(&[("fabric.mod.json", br#"{"id":"x","name":"X"}"#)]);
        fs::write(dir.join("x.jar"), &fabric).await.unwrap();

        // Without Connector the same jar IS flagged — that is
        // `scan_flags_fabric_jar_in_forge_instance` above, same fixture.
        fs::write(
            dir.join("connector-2.0.0-full.jar"),
            &connector_container_bytes(),
        )
        .await
        .unwrap();

        let out = scan_instance(td.path(), LoaderKind::NeoForge, "1.21.1")
            .await
            .unwrap();
        let x = out
            .iter()
            .find(|m| m.sha1 == hex::encode(<sha1::Sha1 as sha1::Digest>::digest(&fabric)))
            .expect("the fabric jar is in the scan");
        assert!(
            !x.loader_mismatch,
            "Connector remaps and loads it, so it is not dead weight"
        );
    }

    #[tokio::test]
    async fn scan_still_flags_when_connector_is_disabled() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let fabric = zip_with(&[("fabric.mod.json", br#"{"id":"x","name":"X"}"#)]);
        fs::write(dir.join("x.jar"), &fabric).await.unwrap();
        // A disabled Connector loads nothing, so it remaps nothing.
        fs::write(
            dir.join("connector-2.0.0-full.jar.disabled"),
            &connector_container_bytes(),
        )
        .await
        .unwrap();

        let out = scan_instance(td.path(), LoaderKind::NeoForge, "1.21.1")
            .await
            .unwrap();
        let x = out
            .iter()
            .find(|m| m.sha1 == hex::encode(<sha1::Sha1 as sha1::Digest>::digest(&fabric)))
            .expect("the fabric jar is in the scan");
        assert!(x.loader_mismatch);
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

    /// Register `filename`+`bytes` as a PACK-BUNDLED mod: add the installed
    /// record and a pack origin whose files list carries the jar's sha1 under
    /// `mods/`.
    async fn add_pack_mod(root: &Path, filename: &str, bytes: &[u8]) -> String {
        use crate::mods::installed::{add, set_pack_origin, PackOrigin, PackOriginFile};
        use crate::mods::modpack::schema::EnvSupport;
        use crate::mods::platform::{InstalledMod, ModSource};
        let sha = hex::encode(Sha1::digest(bytes));
        add(
            root,
            InstalledMod {
                filename: filename.into(),
                sha1: sha.clone(),
                source: Some(ModSource::Modrinth),
                project_id: Some("packmod".into()),
                version_id: Some("packv".into()),
                name: filename.trim_end_matches(".jar").into(),
                version_number: Some("1.0".into()),
                installed_at: chrono::Utc::now().to_rfc3339(),
                enabled: true,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await
        .unwrap();
        set_pack_origin(
            root,
            PackOrigin {
                project_id: None,
                source: ModSource::Curseforge,
                project_name: "FCAP-like Pack".into(),
                version: "1.0".into(),
                files: vec![PackOriginFile {
                    sha1: sha.clone(),
                    name: filename.trim_end_matches(".jar").into(),
                    filename: filename.into(),
                    install_path: format!("mods/{filename}"),
                    url: String::new(),
                    size: 1.0,
                    project_id: String::new(),
                    version_id: String::new(),
                    env_client: EnvSupport::Required,
                    source: ModSource::Curseforge,
                }],
                missing_mods: vec![],
                skipped_overrides: vec![],
                resolved_missing: vec![],
                inert_loader_jars: vec![],
            },
        )
        .await
        .unwrap();
        sha
    }

    /// FCAP C2: a pack-bundled pure-Fabric jar on a Forge instance IS judged
    /// (the pack-trust exemption is gone) and its descriptor verdict is final
    /// (pack mods are not live-checkable).
    #[tokio::test]
    async fn scan_flags_wrong_family_pack_bundled_jar() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("fabric.mod.json", br#"{"id":"fcap","name":"FCAP"}"#)]);
        fs::write(dir.join("fcap.jar"), &bytes).await.unwrap();
        let sha = add_pack_mod(td.path(), "fcap.jar", &bytes).await;

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.20.1")
            .await
            .unwrap();
        let m = out
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert!(m.loader_mismatch, "wrong-family pack jar must be flagged");
        assert!(!m.live_checkable, "pack mods stay non-live-checkable");
        assert_eq!(m.detected_loader.as_deref(), Some("Fabric"));
    }

    /// A pack-bundled jar whose family matches the instance is not flagged.
    #[tokio::test]
    async fn scan_pack_bundled_matching_family_not_flagged() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("META-INF/mods.toml", b"modLoader=\"javafml\"" as &[u8])]);
        fs::write(dir.join("ok.jar"), &bytes).await.unwrap();
        let sha = add_pack_mod(td.path(), "ok.jar", &bytes).await;

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.20.1")
            .await
            .unwrap();
        let m = out
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert!(!m.loader_mismatch);
        assert!(!m.live_checkable);
    }

    /// A descriptor-less pack-bundled jar never flags (conservatism preserved).
    #[tokio::test]
    async fn scan_pack_bundled_descriptorless_not_flagged() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("data/blob.txt", b"not a mod" as &[u8])]);
        fs::write(dir.join("lib.jar"), &bytes).await.unwrap();
        let sha = add_pack_mod(td.path(), "lib.jar", &bytes).await;

        let out = scan_instance(td.path(), LoaderKind::Fabric, "1.20.1")
            .await
            .unwrap();
        let m = out
            .iter()
            .find(|m| m.sha1.eq_ignore_ascii_case(&sha))
            .unwrap();
        assert!(!m.loader_mismatch);
        assert!(m.detected_loader.is_none());
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
        assert_eq!(dep.kind, DependencyKind::Required);
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
        assert_eq!(
            m.deps.iter().find(|d| d.dep_id == "sodium").unwrap().kind,
            DependencyKind::Optional
        );
    }

    #[test]
    fn fabric_manifest_registers_provides_aliases() {
        let json =
            r#"{"id":"sodium","version":"0.6.0","provides":["indium-compat","sodium-extra-shim"]}"#;
        let j = jar(&[("fabric.mod.json", json)]);
        let m = read_jar_manifest_deps(&j).unwrap();
        let ids: Vec<&str> = m.provided.iter().map(|p| p.mod_id.as_str()).collect();
        assert!(ids.contains(&"sodium"), "{ids:?}");
        assert!(ids.contains(&"indium-compat"), "{ids:?}");
        assert!(ids.contains(&"sodium-extra-shim"), "{ids:?}");
        // alias inherits the declaring jar's version
        let alias = m
            .provided
            .iter()
            .find(|p| p.mod_id == "indium-compat")
            .unwrap();
        assert_eq!(alias.version.as_deref(), Some("0.6.0"));
    }

    #[test]
    fn quilt_manifest_registers_provides_strings_and_objects() {
        let json = r#"{"quilt_loader":{"id":"qsl","version":"5.0.0","provides":["qsl-base",{"id":"qfapi","version":"7.1.0"}]}}"#;
        let j = jar(&[("quilt.mod.json", json)]);
        let m = read_jar_manifest_deps(&j).unwrap();
        let base = m.provided.iter().find(|p| p.mod_id == "qsl-base").unwrap();
        assert_eq!(base.version.as_deref(), Some("5.0.0")); // string → own version
        let qfapi = m.provided.iter().find(|p| p.mod_id == "qfapi").unwrap();
        assert_eq!(qfapi.version.as_deref(), Some("7.1.0")); // object version wins
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
    fn jij_reader_extracts_fabric_jars_dir_mod_id() {
        // Fabric/Quilt bundle nested modules under META-INF/jars/, not jarjar.
        let inner_bytes = jar(&[(
            "fabric.mod.json",
            r#"{"id":"fabric-renderer-api-v1","version":"3.2.0"}"#,
        )]);
        let outer = {
            let mut buf = Vec::new();
            {
                let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
                w.start_file(
                    "META-INF/jars/fabric-renderer-api-v1.jar",
                    SimpleFileOptions::default(),
                )
                .unwrap();
                w.write_all(&inner_bytes).unwrap();
                w.finish().unwrap();
            }
            buf
        };
        let providers = read_jar_embedded_providers(&outer);
        assert!(
            providers
                .iter()
                .any(|p| p.mod_id == "fabric-renderer-api-v1"
                    && p.version.as_deref() == Some("3.2.0")),
            "providers: {providers:?}"
        );
    }

    #[test]
    fn jij_reader_reads_both_jarjar_and_jars_dirs() {
        // Forge jarjar AND Fabric jars in one outer jar — both inner ids captured.
        let forge_inner = jar(&[("META-INF/mods.toml", "[[mods]]\nmodId=\"forgelib\"\n")]);
        let fabric_inner = jar(&[("fabric.mod.json", r#"{"id":"fabriclib"}"#)]);
        let outer = {
            let mut buf = Vec::new();
            {
                let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
                w.start_file("META-INF/jarjar/a.jar", SimpleFileOptions::default())
                    .unwrap();
                w.write_all(&forge_inner).unwrap();
                w.start_file("META-INF/jars/b.jar", SimpleFileOptions::default())
                    .unwrap();
                w.write_all(&fabric_inner).unwrap();
                w.finish().unwrap();
            }
            buf
        };
        let providers = read_jar_embedded_providers(&outer);
        assert!(
            providers.iter().any(|p| p.mod_id == "forgelib"),
            "{providers:?}"
        );
        assert!(
            providers.iter().any(|p| p.mod_id == "fabriclib"),
            "{providers:?}"
        );
    }

    // ── ModEnvironment / read_jar_environment tests ───────────────────────────

    #[test]
    fn jar_env_fabric_client_only() {
        let j = jar(&[("fabric.mod.json", r#"{"id":"etf","environment":"client"}"#)]);
        assert_eq!(read_jar_environment(&j), ModEnvironment::Client);
    }

    #[test]
    fn jar_env_fabric_both_star() {
        let j = jar(&[("fabric.mod.json", r#"{"id":"x","environment":"*"}"#)]);
        assert_eq!(read_jar_environment(&j), ModEnvironment::Both);
    }

    #[test]
    fn jar_env_fabric_missing_env_is_both() {
        let j = jar(&[("fabric.mod.json", r#"{"id":"x"}"#)]);
        assert_eq!(read_jar_environment(&j), ModEnvironment::Both);
    }

    #[test]
    fn jar_env_forge_unknown() {
        let j = jar(&[("META-INF/mods.toml", "[[mods]]\nmodId=\"x\"\n")]);
        assert_eq!(read_jar_environment(&j), ModEnvironment::Unknown);
    }

    #[test]
    fn jar_env_no_descriptor_unknown() {
        let j = jar(&[("a.txt", "hi")]);
        assert_eq!(read_jar_environment(&j), ModEnvironment::Unknown);
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
