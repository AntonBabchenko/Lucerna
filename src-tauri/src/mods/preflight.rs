//! Pure dependency pre-flight resolver. No network, no disk. Given each
//! installed mod's parsed manifest and an index of available providers,
//! returns the required-dependency violations the loader would hit.
//!
//! The module also exposes `dependency_preflight_for_root` — the testable
//! core of the `instance_dependency_preflight` Tauri command — along with
//! the `ViolationKind`, `DepViolation`, and `PreflightReport` IPC types.

use std::collections::HashMap;

use crate::mods::local::{DepSide, DependencyKind, ManifestDeps};
use crate::mods::version_range::{satisfies, Satisfaction};

/// One installed mod joined with its parsed manifest.
#[derive(Debug, Clone)]
pub struct ParsedMod {
    pub sha1: String,
    pub name: String,
    pub manifest: ManifestDeps,
}

/// Canonical id form for provider matching: lowercase, `-` → `_`. Mod ecosystems
/// use the two interchangeably (`fabric-api` vs `fabric_api`).
fn canon_id(id: &str) -> String {
    id.trim().to_ascii_lowercase().replace('-', "_")
}

/// Umbrella/loader-bundled libraries: a required id (canon) is satisfied if the
/// instance provides ANY of the listed canon ids. Keep tiny + well-commented;
/// additive only — can clear a false "missing", never create a new one.
const PROVIDES_ALIASES: &[(&str, &[&str])] = &[
    // Sinytra Connector ships fabric-api as forgified-fabric-api (+ its JIJ
    // submodules); a Fabric mod's `fabric-api` dep is satisfied by either.
    ("fabric_api", &["forgified_fabric_api"]),
];

/// What a provider id maps to: its declared version (if known).
#[derive(Debug, Clone, Default)]
pub struct ProviderIndex {
    by_id: HashMap<String, Option<String>>, // canon mod_id -> version
}

impl ProviderIndex {
    /// Build from all enabled mods' provided ids (own + JIJ).
    ///
    /// Presence is a UNION over every descriptor: a jar declaring an id in a
    /// file this loader does not read is still that mod, physically installed,
    /// and dropping it would turn a real mod into a phantom "not installed".
    /// Only the VERSION has an author — see [`effective_rank`] — because the
    /// version is what every declared range is measured against.
    pub fn build(
        mods: &[ParsedMod],
        jij: &[(String, Option<String>)],
        loader: crate::instances::schema::LoaderKind,
        era: crate::mods::local::DescriptorEra,
    ) -> Self {
        // canon id -> the best-ranked version the loader actually reads.
        let mut best: HashMap<String, (u8, String)> = HashMap::new();
        // canon id -> a version from a file the loader does NOT read: `Some` while
        // every such declaration agrees, `None` once two of them differ.
        let mut fallback: HashMap<String, Option<String>> = HashMap::new();
        let mut present: HashMap<String, ()> = HashMap::new();

        for m in mods {
            for p in &m.manifest.provided {
                let key = canon_id(&p.mod_id);
                present.insert(key.clone(), ());
                let Some(version) = p.version.clone() else {
                    continue;
                };
                match effective_rank(p.source, &m.manifest.sources_present, loader, era) {
                    Some(rank) => {
                        if best.get(&key).is_none_or(|(r, _)| rank < *r) {
                            best.insert(key, (rank, version));
                        }
                    }
                    None => match fallback.get(&key) {
                        // Two files the loader does not read, disagreeing: there
                        // is no ground to prefer either, so the honest answer is
                        // "unknown" — which `resolve` reads as "skip the range
                        // check", never as a violation.
                        Some(Some(seen)) if *seen != version => {
                            fallback.insert(key, None);
                        }
                        Some(_) => {}
                        None => {
                            fallback.insert(key, Some(version));
                        }
                    },
                }
            }
        }

        let mut by_id: HashMap<String, Option<String>> = HashMap::new();
        for key in present.into_keys() {
            let version = match best.get(&key) {
                Some((_, v)) => Some(v.clone()),
                None => fallback.get(&key).cloned().flatten(),
            };
            by_id.insert(key, version);
        }
        // JIJ providers are the last resort and carry no descriptor: an embedded
        // library only answers for an id nothing top-level claims.
        for (id, ver) in jij {
            by_id.entry(canon_id(id)).or_insert(ver.clone());
        }
        Self { by_id }
    }

    fn get(&self, id: &str) -> Option<&Option<String>> {
        self.by_id.get(&canon_id(id))
    }

    /// True iff `dep_id` is provided directly or via a known umbrella alias.
    fn is_provided(&self, dep_id: &str) -> bool {
        let key = canon_id(dep_id);
        if self.by_id.contains_key(&key) {
            return true;
        }
        PROVIDES_ALIASES
            .iter()
            .find(|(name, _)| *name == key)
            .is_some_and(|(_, aliases)| aliases.iter().any(|a| self.by_id.contains_key(*a)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    MissingRequired {
        dependent_sha1: String,
        dependent_name: String,
        dep_id: String,
    },
    VersionOutOfRange {
        dependent_sha1: String,
        dependent_name: String,
        dep_id: String,
        needed: String,
        installed: String,
        family: crate::mods::version_range::RangeFamily,
    },
    /// An `optional` dependency that IS installed but out of range. The loader
    /// aborts on this exactly as it does on a missing requirement
    /// (`ModSorter.java:281` feeds both into `versionResolution`).
    OptionalOutOfRange {
        dependent_sha1: String,
        dependent_name: String,
        dep_id: String,
        needed: String,
        installed: String,
        family: crate::mods::version_range::RangeFamily,
    },
    /// An `incompatible` declaration whose range the installed version falls
    /// INSIDE — the inverted check (`ModSorter.java:286-288`).
    IncompatibleInstalled {
        dependent_sha1: String,
        dependent_name: String,
        dep_id: String,
        needed: String,
        installed: String,
        family: crate::mods::version_range::RangeFamily,
    },
    /// The jar declares a platform — a Minecraft or loader version range — this
    /// instance does not provide. The loader refuses such a jar outright; this
    /// is the `ModLoadingException` this whole feature exists to prevent.
    PlatformMismatch {
        dependent_sha1: String,
        dependent_name: String,
        /// `"minecraft"` for the MC axis; the loader's CANONICAL id for the
        /// loader axis — normalized, not necessarily the descriptor's own
        /// spelling. `mc_compat::actual_for` accepts both `fabricloader` and
        /// `fabric-loader`, and either is reported here as `fabricloader`.
        /// Label only: nothing downstream matches this back to a declaration.
        dep_id: String,
        needed: String,
        /// What the instance provides on that axis.
        installed: String,
        family: crate::mods::version_range::RangeFamily,
    },
}

/// Where a descriptor sits in the order this instance's loader reads files.
///
/// `None` — the loader never opens this file on this instance.
/// `Some(n)` — it does; lower is read first and is therefore authoritative.
///
/// Supersedes the old boolean loader-family test: `RangeFamily::Maven` covers
/// `mods.toml`, `neoforge.mods.toml` and the legacy annotation alike, so family
/// alone cannot tell a 1.12.2 instance that its jars' `mods.toml` is inert — and
/// a measured 1.12.2 jar ships exactly that.
///
/// Instance-level only. Whether a file the loader *would* open is actually read
/// for a PARTICULAR jar additionally depends on what else that jar ships — see
/// [`effective_rank`], which layers shadowing on top of this.
///
/// Wildcard-free on purpose: a new `DescriptorSource` cannot be added without
/// answering for every loader.
fn descriptor_rank(
    source: crate::mods::local::DescriptorSource,
    loader: crate::instances::schema::LoaderKind,
    era: crate::mods::local::DescriptorEra,
) -> Option<u8> {
    use crate::instances::schema::LoaderKind as L;
    use crate::mods::local::{DescriptorEra as E, DescriptorSource as S};
    match loader {
        L::Forge => match era {
            // Complementary, not competing: `mcmod.info` is the provider list,
            // the annotation is the requirement list. Equal rank, both read.
            E::Legacy => match source {
                S::McmodInfo | S::McmodAnnotation => Some(0),
                S::ModsToml | S::NeoForgeToml | S::FabricJson | S::QuiltJson => None,
            },
            E::Modern => match source {
                S::ModsToml => Some(0),
                S::McmodInfo
                | S::McmodAnnotation
                | S::NeoForgeToml
                | S::FabricJson
                | S::QuiltJson => None,
            },
        },
        // NeoForge has no legacy era (it starts at MC 1.20.1) and falls back to
        // `mods.toml` only for a jar that ships no `neoforge.mods.toml` — the
        // per-jar half of that rule lives in `effective_rank`. The reverse is
        // NOT true: MinecraftForge has no knowledge of the NeoForge filename.
        L::NeoForge => match source {
            S::NeoForgeToml => Some(0),
            S::ModsToml => Some(1),
            S::McmodInfo | S::McmodAnnotation | S::FabricJson | S::QuiltJson => None,
        },
        // Fabric reads only fabric.mod.json — a Quilt mod on a Fabric instance is
        // a loader-compat issue (handled elsewhere), not a missing-dependency one.
        L::Fabric => match source {
            S::FabricJson => Some(0),
            S::QuiltJson | S::ModsToml | S::NeoForgeToml | S::McmodInfo | S::McmodAnnotation => {
                None
            }
        },
        // Quilt runs Fabric mods, but a jar shipping `quilt.mod.json` never
        // reaches Quilt Loader's Fabric plugin at all: `QuiltPluginManagerImpl`
        // registers the quilt plugin first and `scanZip` breaks as soon as it
        // claims the file. Again the per-jar half is `effective_rank`'s.
        L::Quilt => match source {
            S::QuiltJson => Some(0),
            S::FabricJson => Some(1),
            S::ModsToml | S::NeoForgeToml | S::McmodInfo | S::McmodAnnotation => None,
        },
        // Vanilla loads no mods → no declared descriptor applies.
        L::Vanilla => None,
    }
}

/// [`descriptor_rank`] narrowed to one jar: a descriptor is inert when that same
/// jar also ships one of a **strictly better** rank, because the loader reads
/// only the better one and ignores this file for this jar.
///
/// "Strictly better", not "the single best": equal ranks are complementary
/// rather than alternative (see the Forge-legacy arm above), and a
/// keep-only-the-top rule would drop half of what a 1.12.2 jar declares.
/// `pub(crate)` so `mods::mc_compat` shares this authority rather than
/// reimplementing it.
pub(crate) fn effective_rank(
    source: crate::mods::local::DescriptorSource,
    present: &[crate::mods::local::DescriptorSource],
    loader: crate::instances::schema::LoaderKind,
    era: crate::mods::local::DescriptorEra,
) -> Option<u8> {
    let rank = descriptor_rank(source, loader, era)?;
    let shadowed = present
        .iter()
        .any(|other| descriptor_rank(*other, loader, era).is_some_and(|r| r < rank));
    if shadowed {
        return None;
    }
    Some(rank)
}

/// The launcher launches a client; a SERVER-only dep is not enforced.
pub fn resolve(
    mods: &[ParsedMod],
    index: &ProviderIndex,
    loader: crate::instances::schema::LoaderKind,
    era: crate::mods::local::DescriptorEra,
    instance_mc: &str,
    loader_version: Option<&str>,
) -> Vec<Violation> {
    let mut out = Vec::new();
    for m in mods {
        for dep in &m.manifest.deps {
            // Side is filtered BEFORE the kind, matching `ModSorter.java:275`
            // — a side-filtered dep is invisible to every check.
            if dep.side == DepSide::Server {
                continue;
            }
            // Only enforce deps from the descriptor the instance's loader opens:
            // a Forge instance never loads fabric.mod.json, and a 1.12.2 one
            // never loads mods.toml. Anything else is a declaration the loader
            // cannot enforce, so it is not a launch-readiness problem.
            if effective_rank(dep.source, &m.manifest.sources_present, loader, era).is_none() {
                continue;
            }
            // The loader only logs a warning for `discouraged` and carries on.
            if dep.kind == DependencyKind::Discouraged {
                continue;
            }
            if !index.is_provided(&dep.dep_id) {
                // Absent: only a requirement is a problem. An optional or an
                // incompatible declaration is satisfied by absence.
                if dep.kind.is_required() {
                    out.push(Violation::MissingRequired {
                        dependent_sha1: m.sha1.clone(),
                        dependent_name: m.name.clone(),
                        dep_id: dep.dep_id.clone(),
                    });
                }
                continue;
            }
            // Present — range-check only when a concrete version is known via a
            // direct lookup. Version None, or satisfied-via-alias-only, means
            // the provider is there but its version is unknown => stay silent.
            let Some(Some(v)) = index.get(&dep.dep_id) else {
                continue;
            };
            let sat = satisfies(v, &dep.range, dep.family);
            let violation = match (dep.kind, sat) {
                (DependencyKind::Required, Satisfaction::Violated) => {
                    Violation::VersionOutOfRange {
                        dependent_sha1: m.sha1.clone(),
                        dependent_name: m.name.clone(),
                        dep_id: dep.dep_id.clone(),
                        needed: dep.range.clone(),
                        installed: v.clone(),
                        family: dep.family,
                    }
                }
                (DependencyKind::Optional, Satisfaction::Violated) => {
                    Violation::OptionalOutOfRange {
                        dependent_sha1: m.sha1.clone(),
                        dependent_name: m.name.clone(),
                        dep_id: dep.dep_id.clone(),
                        needed: dep.range.clone(),
                        installed: v.clone(),
                        family: dep.family,
                    }
                }
                // Inverted: an incompatibility fires when the installed version
                // IS inside the declared range.
                (DependencyKind::Incompatible, Satisfaction::Satisfied) => {
                    Violation::IncompatibleInstalled {
                        dependent_sha1: m.sha1.clone(),
                        dependent_name: m.name.clone(),
                        dep_id: dep.dep_id.clone(),
                        needed: dep.range.clone(),
                        installed: v.clone(),
                        family: dep.family,
                    }
                }
                _ => continue,
            };
            out.push(violation);
        }

        // The platform axes. The enforcement rules live in `mc_compat`, which
        // reuses this function's own (side, effective_rank, kind, satisfaction)
        // chain, so there is exactly one implementation of them.
        if let crate::mods::mc_compat::PlatformVerdict::Violated {
            axis,
            declared,
            actual,
            family,
            ..
        } = crate::mods::mc_compat::platform_verdict(
            &m.manifest,
            instance_mc,
            loader,
            loader_version,
            era,
        ) {
            out.push(Violation::PlatformMismatch {
                dependent_sha1: m.sha1.clone(),
                dependent_name: m.name.clone(),
                dep_id: match axis {
                    crate::mods::mc_compat::PlatformAxis::Minecraft => "minecraft".to_string(),
                    crate::mods::mc_compat::PlatformAxis::Loader => loader_dep_id(loader),
                },
                needed: declared,
                installed: actual,
                family,
            });
        }
    }
    out
}

/// The dependency id a loader is declared under. Used only to label a platform
/// violation for the UI — the verdict itself is decided in `mc_compat`.
fn loader_dep_id(loader: crate::instances::schema::LoaderKind) -> String {
    use crate::instances::schema::LoaderKind as L;
    match loader {
        L::Forge => "forge",
        L::NeoForge => "neoforge",
        L::Fabric => "fabricloader",
        L::Quilt => "quilt_loader",
        // Vanilla loads no mods, so `mc_compat` never returns a loader-axis
        // verdict here; the arm exists to keep the match exhaustive.
        L::Vanilla => "minecraft",
    }
    .to_string()
}

// ── IPC types & testable command core ─────────────────────────────────────

/// What kind of dependency violation was detected.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ViolationKind {
    /// A required dependency mod is absent from the installed set.
    MissingRequired,
    /// A required dependency is present but its version does not satisfy
    /// the declared version range.
    VersionOutOfRange,
    /// An optional dependency is installed but out of range. The loader treats
    /// this exactly as it treats a missing requirement: it aborts.
    OptionalOutOfRange,
    /// A mod declares itself incompatible with the installed version of
    /// another mod. The range is inverted: it names the versions that clash.
    IncompatibleInstalled,
    /// The jar was built for a Minecraft or loader version this instance does
    /// not provide.
    PlatformMismatch,
}

/// One resolved dependency violation, enriched with enough context for the
/// UI to show an actionable error row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DepViolation {
    /// SHA-1 of the mod that declared the dependency.
    pub dependent_sha1: String,
    /// Display name of the mod that declared the dependency.
    pub dependent_name: String,
    /// Mod-id of the missing / out-of-range dependency.
    pub dep_id: String,
    /// `MissingRequired` or `VersionOutOfRange`.
    pub kind: ViolationKind,
    /// The version that is actually installed (`None` for `MissingRequired`).
    pub installed_version: Option<String>,
    /// The version range the dependent declared (empty string for
    /// `MissingRequired`), verbatim from the jar. Kept for remediation
    /// (`mods_filter_satisfying` evaluates it) and for the log line — the UI
    /// renders `needed_desc` instead, because raw Maven bracket notation is
    /// unreadable.
    pub needed: String,
    /// `needed`, decomposed into displayable clauses. The UI formats these
    /// through i18n and only falls back to the raw string when
    /// `needed_desc.unparseable`.
    pub needed_desc: crate::mods::range_describe::RangeDescription,
    /// Platform project reference for the provider, if we could link it.
    /// Powers a "View on Modrinth / CurseForge" link in the UI.
    pub provider_project: Option<crate::mods::platform::DepProjectRef>,
    /// SHA-1 of the installed jar that currently provides `dep_id`.
    /// Present only for `VersionOutOfRange` violations where the provider
    /// is a tracked installed mod. Used by the UI to route "Обновить"
    /// through `mods_update_one` (remove-old + install-new) instead of a
    /// bare `mods_install_with_deps` that would leave duplicate jars.
    pub provider_sha1: Option<String>,
    /// Range grammar for `needed` (Maven / Fabric / Quilt). `None` for
    /// `MissingRequired` (no range to interpret). Lets the UI pick a version
    /// that actually satisfies `needed` via `mods_filter_satisfying`.
    pub family: Option<crate::mods::version_range::RangeFamily>,
}

/// Aggregated result of the dependency pre-flight scan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PreflightReport {
    /// All detected violations. Empty means no problems found.
    pub violations: Vec<DepViolation>,
    /// What a self-completing pack has yet to fetch, when this instance has one.
    /// While `outstanding` is non-empty the violations are advisory: the pack is
    /// designed to arrive incomplete and fill itself in on first launch.
    ///
    /// `#[serde(default)]` so specta emits it OPTIONAL — without it every
    /// existing `PreflightReport` literal in the frontend stops type-checking.
    #[serde(default)]
    pub pack_completion: Option<crate::mods::pack_completion::PackCompletion>,
}

/// Map a `ModSource` + `project_id` to a `DepProjectRef` for the
/// "view on platform" link. Returns `None` for pack-managed sources (FTB,
/// ATLauncher) that have no per-mod browser.
fn dep_project_ref(
    source: crate::mods::platform::ModSource,
    pid: &str,
) -> Option<crate::mods::platform::DepProjectRef> {
    use crate::mods::platform::{DepProjectRef, ModSource};
    match source {
        ModSource::Modrinth => Some(DepProjectRef::Modrinth {
            project_id: pid.into(),
            version_id: None,
        }),
        ModSource::Curseforge => pid
            .parse::<u32>()
            .ok()
            .map(|mod_id| DepProjectRef::Curseforge {
                mod_id,
                file_id: None,
            }),
        // FTB and ATLauncher are pack-only sources with no per-mod browser.
        // Hangar plugins never reach this dep-violation path either (plugins have no Java
        // dependency graph) — no per-mod browser link for it here.
        // Vanilla Tweaks is a datapack builder — datapacks have no dependency
        // graph, so a VT pack never reaches a dep violation either.
        ModSource::Ftb | ModSource::Atlauncher | ModSource::Hangar | ModSource::VanillaTweaks => {
            None
        }
    }
}

/// Convert a raw `Violation` into a `DepViolation`, enriching the
/// `provider_project` and `provider_sha1` fields from the maps built earlier.
fn enrich(
    v: Violation,
    provider_owner: &std::collections::HashMap<String, crate::mods::platform::DepProjectRef>,
    provider_sha1_map: &std::collections::HashMap<String, String>,
) -> DepViolation {
    match v {
        Violation::MissingRequired {
            dependent_sha1,
            dependent_name,
            dep_id,
        } => DepViolation {
            dependent_sha1,
            dependent_name,
            dep_id,
            kind: ViolationKind::MissingRequired,
            installed_version: None,
            needed: String::new(),
            // Nothing is installed, so there is no range to read against — an
            // empty range describes as "any version".
            needed_desc: crate::mods::range_describe::describe(
                "",
                crate::mods::version_range::RangeFamily::Maven,
            ),
            provider_project: None,
            provider_sha1: None,
            family: None,
        },
        Violation::VersionOutOfRange {
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
            family,
        } => ranged(
            ViolationKind::VersionOutOfRange,
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
            family,
            provider_owner,
            provider_sha1_map,
        ),
        Violation::OptionalOutOfRange {
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
            family,
        } => ranged(
            ViolationKind::OptionalOutOfRange,
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
            family,
            provider_owner,
            provider_sha1_map,
        ),
        Violation::IncompatibleInstalled {
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
            family,
        } => ranged(
            ViolationKind::IncompatibleInstalled,
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
            family,
            provider_owner,
            provider_sha1_map,
        ),
        Violation::PlatformMismatch {
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
            family,
        } => DepViolation {
            dependent_sha1,
            dependent_name,
            dep_id,
            kind: ViolationKind::PlatformMismatch,
            installed_version: Some(installed),
            needed_desc: crate::mods::range_describe::describe(&needed, family),
            needed,
            // Not remediated through the dependency-update path: there is no
            // "provider" to update, the instance itself is the wrong platform.
            provider_project: None,
            provider_sha1: None,
            family: Some(family),
        },
    }
}

/// Shared enrichment for the three violations that carry a range and an
/// installed version.
#[allow(clippy::too_many_arguments)]
fn ranged(
    kind: ViolationKind,
    dependent_sha1: String,
    dependent_name: String,
    dep_id: String,
    needed: String,
    installed: String,
    family: crate::mods::version_range::RangeFamily,
    provider_owner: &std::collections::HashMap<String, crate::mods::platform::DepProjectRef>,
    provider_sha1_map: &std::collections::HashMap<String, String>,
) -> DepViolation {
    // Normalize '-'/'_' + lowercase on BOTH sides so a `fabric-api` dep routes
    // to a `fabric_api` provider (the two are used interchangeably by the
    // ecosystem). Matches `canon_id`, which the provider index already uses.
    let key = canon_id(&dep_id);
    DepViolation {
        dependent_sha1,
        dependent_name,
        kind,
        installed_version: Some(installed),
        needed_desc: crate::mods::range_describe::describe(&needed, family),
        needed,
        provider_project: provider_owner.get(&key).cloned(),
        provider_sha1: provider_sha1_map.get(&key).cloned(),
        family: Some(family),
        dep_id,
    }
}

/// Everything the pre-flight reads out of one jar, complete for the era it was
/// built for. Distinct from the cache's [`crate::mods::jar_scan_cache::CachedScan`],
/// which may be partial: [`usable_hit`] is the one place a partial record is
/// turned into a complete answer, or refused.
struct JarScan {
    manifest: ManifestDeps,
    /// `@Mod(dependencies = …)` requirements. Empty on a modern-era instance,
    /// where the reader never runs — and deliberately NOT stored as a fact
    /// there; see [`usable_hit`].
    legacy_deps: Vec<crate::mods::local::DeclaredDep>,
    jij_provided: Vec<crate::mods::local::ProvidedMod>,
}

/// A cached record turned into a complete [`JarScan`], or `None` when it does
/// not answer everything this era asks.
///
/// The `legacy_deps` branch is the point of the whole `Option` shape. A record
/// written while scanning a MODERN instance carries `legacy_deps: None` —
/// "never scanned", not "this jar declares none" — and a 1.12.2 instance that
/// believed it would report zero requirements for every mod in the pack,
/// because the annotation is the ONLY place that era declares them. A modern
/// instance, symmetrically, does not care what the annotation says and must not
/// be sent back to the jar to find out.
fn usable_hit(
    hit: Option<&crate::mods::jar_scan_cache::CachedScan>,
    want_legacy: bool,
) -> Option<JarScan> {
    let hit = hit?;
    let manifest = hit.manifest.clone()?;
    let jij_provided = hit.jij_provided.clone()?;
    let legacy_deps = if want_legacy {
        hit.legacy_deps.clone()?
    } else {
        Vec::new()
    };
    Some(JarScan {
        manifest,
        legacy_deps,
        jij_provided,
    })
}

/// Read every descriptor this era needs out of one jar's bytes, OFF the async
/// executor.
///
/// `zip` is sync, and this is not a small sync call: `read_jar_manifest_deps`
/// inflates several entries, `read_jar_embedded_providers` recursively unzips
/// every nested jar, and on the legacy era `read_jar_legacy_deps` decompresses
/// class entries until it finds the annotation. Run inline on a tokio worker, a
/// 140-mod pack starves every other task on that worker — including the
/// download and launch pipelines. One blocking task for the WHOLE jar rather
/// than one per reader, mirroring `datapacks::library::install_local_at`, which
/// offloads classify + read_meta + sha1 together for the same reason. The
/// caller's loop stays sequential, so at most one blocking task exists at a
/// time and the blocking pool is never flooded.
///
/// `None` is "could not tell" — an unreadable zip, or a blocking task that
/// panicked or was cancelled — and it is never written to the cache: a failure
/// to read must not be frozen into "this jar declares nothing".
async fn scan_jar(bytes: Vec<u8>, want_legacy: bool) -> Option<JarScan> {
    use crate::mods::local::{
        read_jar_embedded_providers, read_jar_legacy_deps, read_jar_manifest_deps,
    };
    let joined = tokio::task::spawn_blocking(move || {
        let manifest = read_jar_manifest_deps(&bytes).ok()?;
        // The legacy guard needs the jar's own mod-ids, which the manifest read
        // has just collected.
        let legacy_deps = if want_legacy {
            let own: Vec<String> = manifest.provided.iter().map(|p| p.mod_id.clone()).collect();
            read_jar_legacy_deps(&bytes, &own)
        } else {
            Vec::new()
        };
        let jij_provided = read_jar_embedded_providers(&bytes);
        Some(JarScan {
            manifest,
            legacy_deps,
            jij_provided,
        })
    })
    .await;
    match joined {
        Ok(scan) => scan,
        Err(e) => {
            crate::diag!("[mods] pre-flight jar scan task failed: {e}");
            None
        }
    }
}

/// The testable core of the `instance_dependency_preflight` Tauri command.
/// Accepts a resolved `instance_root` path so integration tests can call it
/// without a `tauri::AppHandle`.
///
/// `cache_path` is the shared jar-scan cache (`paths::jar_scan_cache_file`).
/// `None` runs the scan uncached: the same answer, only slower. That is the
/// deliberate direction — a data root that cannot be resolved right now must
/// not fail a check sitting in the launch chokepoint.
pub async fn dependency_preflight_for_root(
    root: &std::path::Path,
    cache_path: Option<&std::path::Path>,
    loader: crate::instances::schema::LoaderKind,
    mc: &str,
    loader_version: Option<&str>,
) -> crate::error::Result<PreflightReport> {
    use crate::mods::jar_scan_cache::{CachedScan, ScanCache};
    use crate::mods::local::{descriptor_era, DescriptorEra};
    use std::collections::HashMap;

    // Which descriptor this instance's loader actually opens. Decided by the
    // instance's MC version, never by which files a jar happens to ship.
    let era = descriptor_era(mc);
    let want_legacy = era == DescriptorEra::Legacy;

    let installed = crate::mods::installed::list(root).await?;
    let mods_dir = crate::mods::installed::mods_dir(root);

    // One read of a small JSON for the whole scan, then a map lookup per jar —
    // the shape `l10n::coverage::scan_instance` uses.
    let cached = cache_path.map(ScanCache::load).unwrap_or_default();
    let mut fresh: Vec<(String, CachedScan)> = Vec::new();

    // Map lowercased provided mod_id → DepProjectRef for violation enrichment
    // (powers the "view on platform" link). Built from mods with source identity.
    let mut provider_owner: HashMap<String, crate::mods::platform::DepProjectRef> = HashMap::new();
    // Map lowercased provided mod_id → SHA-1 of the jar that provides it.
    // Used to route "Обновить" through mods_update_one (remove-old + install-new).
    let mut provider_sha1: HashMap<String, String> = HashMap::new();

    let mut parsed: Vec<ParsedMod> = Vec::new();
    let mut jij: Vec<(String, Option<String>)> = Vec::new();

    for m in &installed {
        if !m.enabled {
            continue;
        }
        // The cache key is the digest of the bytes on disk RIGHT NOW, never
        // `m.sha1`: the registry deliberately keeps a record's EXPECTED digest
        // when the file under that name was replaced, so keying on it would
        // serve the previous jar's dependencies for the current one. `None` —
        // no file, or a digest we could not compute — means no cache
        // participation for this jar in either direction.
        let cache_key = crate::mods::installed::on_disk_sha1(&mods_dir, &m.filename).await;
        let hit = cache_key.as_deref().and_then(|k| cached.get(k));

        let scan = match usable_hit(hit, want_legacy) {
            Some(s) => s,
            None => {
                let Some(bytes) = crate::mods::local::read_jar_for(&mods_dir, &m.filename).await
                else {
                    continue; // jar missing from disk — skip gracefully
                };
                let Some(s) = scan_jar(bytes, want_legacy).await else {
                    continue; // unreadable zip — skip, never fail the whole scan
                };
                // Only a key computed from the real bytes may be written:
                // without one we do not know WHICH jar this record describes.
                if let Some(k) = cache_key.as_deref() {
                    fresh.push((
                        k.to_string(),
                        CachedScan {
                            // Not read on this path. `None` says exactly that,
                            // leaving `local::scan_instance`'s half of the
                            // record for whoever measures it.
                            meta: None,
                            manifest: Some(s.manifest.clone()),
                            // `Some(vec![])` only when the reader actually ran.
                            // A modern-era scan stores `None`, so a later
                            // legacy-era scan re-reads instead of believing an
                            // emptiness nobody measured.
                            legacy_deps: want_legacy.then(|| s.legacy_deps.clone()),
                            jij_provided: Some(s.jij_provided.clone()),
                        },
                    ));
                }
                s
            }
        };

        let mut manifest = scan.manifest;
        // On the legacy era the requirements live nowhere else: `mcmod.info`'s
        // own `dependencies` array is cosmetic and FML enforces the
        // `@Mod(dependencies = …)` annotation instead. Empty on every other era.
        manifest.deps.extend(scan.legacy_deps);

        // Collect JIJ (Jar-in-Jar) providers so an embedded lib is not
        // falsely flagged as a missing dependency.
        for p in scan.jij_provided {
            jij.push((p.mod_id, p.version));
        }

        // Register provided mod-ids → platform identity and SHA-1 for enrichment.
        // provider_sha1 is populated regardless of source so that even FTB/ATL
        // mods (which yield no DepProjectRef) can still route updates correctly.
        // Deliberately `m.sha1`, the REGISTRY digest, not the on-disk one above:
        // this value routes UI actions (`mods_update_one`, row identity) against
        // `installed-mods.json`, so it must be the record's own key.
        for p in &manifest.provided {
            // Canonicalize ('-'/'_' equivalent, lowercase) so the enrichment
            // lookup in `enrich` — which uses the same `canon_id` — matches a
            // `fabric-api` dep against a `fabric_api` provider.
            let key = canon_id(&p.mod_id);
            provider_sha1.entry(key).or_insert_with(|| m.sha1.clone());
        }
        // Only insert a DepProjectRef when dep_project_ref returns Some;
        // FTB/ATLauncher sources return None (no per-mod browser) and must
        // not create a spurious link.
        if let (Some(source), Some(project_id)) = (m.source, m.project_id.as_deref()) {
            if let Some(ref_) = dep_project_ref(source, project_id) {
                for p in &manifest.provided {
                    provider_owner
                        .entry(canon_id(&p.mod_id))
                        .or_insert_with(|| ref_.clone());
                }
            }
        }

        parsed.push(ParsedMod {
            sha1: m.sha1.clone(),
            name: m.name.clone(),
            manifest,
        });
    }

    // One save for the whole scan, under the cache's disk lock — the shape
    // `l10n::coverage::scan_instance` uses for its own `fresh` vector.
    if let Some(path) = cache_path {
        if !fresh.is_empty() {
            ScanCache::update(path, |c| {
                for (sha, entry) in fresh {
                    c.merge(&sha, entry);
                }
            });
        }
    }

    let index = ProviderIndex::build(&parsed, &jij, loader, era);
    let raw = resolve(&parsed, &index, loader, era, mc, loader_version);
    let violations = raw
        .into_iter()
        .map(|v| enrich(v, &provider_owner, &provider_sha1))
        .collect();
    Ok(PreflightReport {
        violations,
        pack_completion: crate::mods::pack_completion::read(root),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;
    use crate::mods::local::{
        DeclaredDep, DependencyKind, DescriptorEra, DescriptorSource, ManifestDeps, ProvidedMod,
    };
    use crate::mods::version_range::RangeFamily;

    /// The instance decides which descriptor is authoritative, not the jar. A
    /// measured 1.12.2 jar ships BOTH `mcmod.info` and a `mods.toml` stamped
    /// `loaderVersion="[24,)"` — written for its 1.14+ build — so "which file is
    /// present" cannot answer this.
    ///
    /// The old boolean `dep_applies_to_instance` is now a projection of the
    /// ranking: `is_some()`. Every assertion it made is kept verbatim, so a
    /// refactor that changes admissibility fails here rather than in production.
    #[test]
    fn only_the_descriptor_the_loader_opens_is_admitted() {
        use crate::mods::local::{DescriptorEra as E, DescriptorSource as S};
        use LoaderKind as L;
        let ok = |s, l, e| descriptor_rank(s, l, e).is_some();

        // Forge 1.12.2 reads the annotation; its mods.toml is written for 1.14+.
        assert!(ok(S::McmodAnnotation, L::Forge, E::Legacy));
        assert!(ok(S::McmodInfo, L::Forge, E::Legacy));
        assert!(!ok(S::ModsToml, L::Forge, E::Legacy));
        // Forge 1.13+ reads mods.toml, never neoforge.mods.toml — the asymmetry
        // is deliberate: NeoForge falls back to mods.toml, Forge has no
        // knowledge of the NeoForge filename at all.
        assert!(ok(S::ModsToml, L::Forge, E::Modern));
        assert!(!ok(S::McmodAnnotation, L::Forge, E::Modern));
        assert!(!ok(S::McmodInfo, L::Forge, E::Modern));
        assert!(!ok(S::NeoForgeToml, L::Forge, E::Modern));
        // NeoForge prefers its own file and falls back to mods.toml.
        assert!(ok(S::NeoForgeToml, L::NeoForge, E::Modern));
        assert!(ok(S::ModsToml, L::NeoForge, E::Modern));
        assert!(!ok(S::FabricJson, L::NeoForge, E::Modern));
        assert!(!ok(S::McmodAnnotation, L::NeoForge, E::Modern));
        // Fabric reads only its own; Quilt reads both.
        assert!(ok(S::FabricJson, L::Fabric, E::Modern));
        assert!(!ok(S::QuiltJson, L::Fabric, E::Modern));
        assert!(!ok(S::ModsToml, L::Fabric, E::Modern));
        assert!(ok(S::QuiltJson, L::Quilt, E::Modern));
        assert!(ok(S::FabricJson, L::Quilt, E::Modern));
        assert!(!ok(S::ModsToml, L::Quilt, E::Modern));
        // Vanilla loads no mods.
        for s in [
            S::McmodAnnotation,
            S::McmodInfo,
            S::ModsToml,
            S::NeoForgeToml,
            S::FabricJson,
            S::QuiltJson,
        ] {
            assert!(!ok(s, L::Vanilla, E::Modern));
            assert!(!ok(s, L::Vanilla, E::Legacy));
        }
    }

    /// NeoForge reads `neoforge.mods.toml` and ignores that jar's `mods.toml`.
    /// Admitting both by rank alone would enforce dependencies the loader never
    /// reads — inventing exactly the kind of phantom this whole line of work
    /// exists to remove.
    #[test]
    fn a_better_descriptor_in_the_same_jar_shadows_the_worse_one() {
        use crate::mods::local::{DescriptorEra as E, DescriptorSource as S};
        use LoaderKind as L;

        let both = [S::NeoForgeToml, S::ModsToml];
        assert!(effective_rank(S::NeoForgeToml, &both, L::NeoForge, E::Modern).is_some());
        assert!(
            effective_rank(S::ModsToml, &both, L::NeoForge, E::Modern).is_none(),
            "neoforge.mods.toml shadows mods.toml inside one jar"
        );
        // Alone, it is read.
        assert!(effective_rank(S::ModsToml, &[S::ModsToml], L::NeoForge, E::Modern).is_some());

        // Same rule on Quilt: a jar shipping quilt.mod.json never reaches Quilt
        // Loader's Fabric plugin.
        let qf = [S::QuiltJson, S::FabricJson];
        assert!(effective_rank(S::QuiltJson, &qf, L::Quilt, E::Modern).is_some());
        assert!(effective_rank(S::FabricJson, &qf, L::Quilt, E::Modern).is_none());
        assert!(effective_rank(S::FabricJson, &[S::FabricJson], L::Quilt, E::Modern).is_some());

        // MinecraftForge does not read neoforge.mods.toml at all, so it cannot
        // shadow anything there — this is the coverage gap being fixed.
        assert!(
            effective_rank(S::ModsToml, &both, L::Forge, E::Modern).is_some(),
            "on MinecraftForge the mods.toml of a dual-descriptor jar IS read"
        );

        // Equal ranks never shadow each other.
        let legacy = [S::McmodInfo];
        assert!(effective_rank(S::McmodInfo, &legacy, L::Forge, E::Legacy).is_some());
        assert!(effective_rank(S::McmodAnnotation, &legacy, L::Forge, E::Legacy).is_some());
    }

    /// Which descriptor names a provider's VERSION decides what every range is
    /// measured against. A 1.12.2 jar can ship a `mods.toml` written for its
    /// 1.14+ build; believing that version is the same mistake as believing its
    /// dependencies.
    #[test]
    fn the_provider_version_comes_from_the_descriptor_the_loader_reads() {
        use crate::mods::local::{DescriptorEra as E, DescriptorSource as S};

        let pv = |id: &str, ver: Option<&str>, source| ProvidedMod {
            mod_id: id.into(),
            version: ver.map(str::to_string),
            source,
        };

        // Authoritative wins over a disagreeing non-authoritative one.
        let mods = vec![modz_from(
            "aa",
            vec![
                pv("core", Some("1.10"), S::McmodInfo),
                pv("core", Some("2.0"), S::ModsToml),
            ],
            vec![],
            vec![S::McmodInfo, S::ModsToml],
        )];
        let legacy = ProviderIndex::build(&mods, &[], LoaderKind::Forge, E::Legacy);
        assert_eq!(legacy.get("core"), Some(&Some("1.10".to_string())));
        let modern = ProviderIndex::build(&mods, &[], LoaderKind::Forge, E::Modern);
        assert_eq!(modern.get("core"), Some(&Some("2.0".to_string())));

        // Authoritative is silent, the rest agree -> use it rather than lose it.
        let agree = vec![modz_from(
            "bb",
            vec![
                pv("core", None, S::ModsToml),
                pv("core", Some("3.0"), S::McmodInfo),
            ],
            vec![],
            vec![S::ModsToml, S::McmodInfo],
        )];
        let idx = ProviderIndex::build(&agree, &[], LoaderKind::Forge, E::Modern);
        assert_eq!(idx.get("core"), Some(&Some("3.0".to_string())));

        // Authoritative is silent and the rest disagree -> unknown, never a guess.
        let disagree = vec![modz_from(
            "cc",
            vec![
                pv("core", Some("3.0"), S::McmodInfo),
                pv("core", Some("4.0"), S::FabricJson),
            ],
            vec![],
            vec![S::McmodInfo, S::FabricJson],
        )];
        let idx = ProviderIndex::build(&disagree, &[], LoaderKind::Forge, E::Modern);
        assert_eq!(
            idx.get("core"),
            Some(&None),
            "present, version unknown - not one of the two guessed"
        );

        // Presence is a UNION: a provider declared only in a file this loader
        // never opens is still installed.
        let unread = vec![modz_from(
            "dd",
            vec![pv("somelib", Some("1.0"), S::ModsToml)],
            vec![],
            vec![S::ModsToml],
        )];
        let idx = ProviderIndex::build(&unread, &[], LoaderKind::Forge, E::Legacy);
        assert!(
            idx.is_provided("somelib"),
            "the jar is installed; only its version is unreadable here"
        );
    }

    /// End of the chain: a shadowed dependency produces no violation even though
    /// its source is admitted at instance level.
    #[test]
    fn resolve_ignores_a_dependency_from_a_shadowed_descriptor() {
        use crate::mods::local::{DescriptorEra as E, DescriptorSource as S};
        let mods = vec![modz_from(
            "aa",
            vec![],
            vec![dep_from(
                "ghost",
                "",
                RangeFamily::Maven,
                DependencyKind::Required,
                S::ModsToml,
            )],
            vec![S::NeoForgeToml, S::ModsToml],
        )];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, E::Modern);
        assert!(
            resolve(
                &mods,
                &index,
                LoaderKind::NeoForge,
                E::Modern,
                "1.20.1",
                None
            )
            .is_empty(),
            "mods.toml is shadowed by neoforge.mods.toml in this jar"
        );

        // The very same jar on MinecraftForge: mods.toml is what the loader
        // reads, so the dependency IS enforced.
        let forge = resolve(&mods, &index, LoaderKind::Forge, E::Modern, "1.20.1", None);
        assert_eq!(forge.len(), 1, "{forge:?}");
    }

    /// Ordering, not just membership: a loader that reads two files still reads
    /// one of them FIRST, and that is what decides a provider's version.
    #[test]
    fn descriptor_rank_orders_the_files_a_loader_reads() {
        use crate::mods::local::{DescriptorEra as E, DescriptorSource as S};
        use LoaderKind as L;

        assert!(
            descriptor_rank(S::NeoForgeToml, L::NeoForge, E::Modern)
                < descriptor_rank(S::ModsToml, L::NeoForge, E::Modern)
        );
        assert!(
            descriptor_rank(S::QuiltJson, L::Quilt, E::Modern)
                < descriptor_rank(S::FabricJson, L::Quilt, E::Modern)
        );
        // The one tie in the table, and it is deliberate: on the legacy era
        // these two are complementary, not competing — `mcmod.info` contributes
        // providers and never dependencies, the annotation the reverse.
        assert_eq!(
            descriptor_rank(S::McmodInfo, L::Forge, E::Legacy),
            descriptor_rank(S::McmodAnnotation, L::Forge, E::Legacy)
        );
    }

    // `dep_family_matches_instance_loader` was superseded by
    // `only_the_descriptor_the_loader_opens_is_admitted` above: the family test
    // could not distinguish `mods.toml` from the legacy annotation, which is the
    // whole point of the provenance tag.

    #[test]
    fn incompatible_fires_only_when_the_installed_version_is_inside_the_range() {
        // `ModSorter.java:286-288` — an incompatibility fires when the mod is
        // PRESENT and its version IS contained in the declared range.
        let ap = modz(
            "a",
            vec![prov("asyncparticles", "21.1.0")],
            vec![dep_of(
                "create",
                "(,6.0.9]",
                RangeFamily::Maven,
                DependencyKind::Incompatible,
            )],
        );
        let create_new = modz("b", vec![prov("create", "6.0.10")], vec![]);
        let mods = vec![ap.clone(), create_new];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(
            resolve(
                &mods,
                &index,
                LoaderKind::NeoForge,
                DescriptorEra::Modern,
                "1.20.1",
                None
            )
            .is_empty(),
            "6.0.10 is outside (,6.0.9] — the incompatibility does not apply"
        );

        let create_old = modz("c", vec![prov("create", "6.0.5")], vec![]);
        let clashing = vec![ap, create_old];
        let index =
            ProviderIndex::build(&clashing, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(matches!(
            resolve(&clashing, &index, LoaderKind::NeoForge, DescriptorEra::Modern, "1.20.1", None).as_slice(),
            [Violation::IncompatibleInstalled { dep_id, .. }] if dep_id == "create"
        ));
    }

    #[test]
    fn incompatible_never_fires_when_the_mod_is_absent() {
        let mods = vec![modz(
            "a",
            vec![],
            vec![dep_of(
                "create",
                "(,6.0.9]",
                RangeFamily::Maven,
                DependencyKind::Incompatible,
            )],
        )];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(resolve(
            &mods,
            &index,
            LoaderKind::NeoForge,
            DescriptorEra::Modern,
            "1.20.1",
            None
        )
        .is_empty());
    }

    #[test]
    fn optional_is_checked_only_when_present_and_then_it_can_block() {
        // `ModSorter.java:281` puts an installed-but-out-of-range OPTIONAL into
        // versionResolution, which aborts startup at `ModSorter.java:72`.
        let declaring = modz(
            "a",
            vec![],
            vec![dep_of(
                "curios",
                "[9.0,)",
                RangeFamily::Maven,
                DependencyKind::Optional,
            )],
        );
        let absent = vec![declaring.clone()];
        let index = ProviderIndex::build(&absent, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(
            resolve(
                &absent,
                &index,
                LoaderKind::NeoForge,
                DescriptorEra::Modern,
                "1.20.1",
                None
            )
            .is_empty(),
            "an absent optional dependency is not a problem"
        );

        let present = vec![declaring, modz("b", vec![prov("curios", "5.4.0")], vec![])];
        let index =
            ProviderIndex::build(&present, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(matches!(
            resolve(&present, &index, LoaderKind::NeoForge, DescriptorEra::Modern, "1.20.1", None).as_slice(),
            [Violation::OptionalOutOfRange { dep_id, .. }] if dep_id == "curios"
        ));
    }

    #[test]
    fn discouraged_never_produces_a_violation() {
        // FML logs "Issues may arise. Continue at your own risk." and carries on.
        let mods = vec![
            modz(
                "a",
                vec![],
                vec![dep_of(
                    "create",
                    "(,6.0.9]",
                    RangeFamily::Maven,
                    DependencyKind::Discouraged,
                )],
            ),
            modz("b", vec![prov("create", "6.0.5")], vec![]),
        ];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(resolve(
            &mods,
            &index,
            LoaderKind::NeoForge,
            DescriptorEra::Modern,
            "1.20.1",
            None
        )
        .is_empty());
    }

    #[test]
    fn bare_maven_range_no_longer_reports_a_violation() {
        // The reported bug: MyNethersDelight declares a bare "1.21-1.3" and
        // Farmer's Delight declares version "1.3.2".
        let mods = vec![
            modz(
                "a",
                vec![prov("mynethersdelight", "1.10.2")],
                vec![dep("farmersdelight", "1.21-1.3", RangeFamily::Maven)],
            ),
            modz("b", vec![prov("farmersdelight", "1.3.2")], vec![]),
        ];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(resolve(
            &mods,
            &index,
            LoaderKind::NeoForge,
            DescriptorEra::Modern,
            "1.20.1",
            None
        )
        .is_empty());
    }

    #[test]
    fn resolve_ignores_inactive_loader_deps() {
        // One mod declaring a missing Maven (Forge) dep AND a missing Fabric dep
        // — the real "All Of Create · Forge" false-positive shape.
        let mods = vec![modz(
            "aa",
            vec![],
            vec![
                dep("create", "", RangeFamily::Maven),
                dep("fabric-api", "*", RangeFamily::FabricPredicate),
            ],
        )];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        // Forge instance: only the Maven dep is a real violation.
        let forge = resolve(
            &mods,
            &index,
            LoaderKind::Forge,
            DescriptorEra::Modern,
            "1.20.1",
            None,
        );
        assert_eq!(forge.len(), 1);
        assert!(
            matches!(&forge[0], Violation::MissingRequired { dep_id, .. } if dep_id == "create")
        );
        // Fabric instance: only the fabric dep is real.
        let fabric = resolve(
            &mods,
            &index,
            LoaderKind::Fabric,
            DescriptorEra::Modern,
            "1.20.1",
            None,
        );
        assert_eq!(fabric.len(), 1);
        assert!(
            matches!(&fabric[0], Violation::MissingRequired { dep_id, .. } if dep_id == "fabric-api")
        );
    }

    #[test]
    fn quilt_enforces_fabric_deps() {
        // Quilt runs Fabric mods, so a Fabric-family dep IS real on a Quilt
        // instance and must still be flagged when missing.
        let mods = vec![modz(
            "aa",
            vec![],
            vec![dep("fabric-api", "*", RangeFamily::FabricPredicate)],
        )];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        let quilt = resolve(
            &mods,
            &index,
            LoaderKind::Quilt,
            DescriptorEra::Modern,
            "1.20.1",
            None,
        );
        assert_eq!(quilt.len(), 1);
        assert!(
            matches!(&quilt[0], Violation::MissingRequired { dep_id, .. } if dep_id == "fabric-api")
        );
    }

    fn dep(id: &str, range: &str, family: RangeFamily) -> DeclaredDep {
        dep_of(id, range, family, DependencyKind::Required)
    }
    fn dep_of(id: &str, range: &str, family: RangeFamily, kind: DependencyKind) -> DeclaredDep {
        // Every Maven case in this module stands for a modern Forge/NeoForge
        // `mods.toml`; the legacy annotation has its own dedicated fixtures.
        let source = match family {
            RangeFamily::Maven => DescriptorSource::ModsToml,
            RangeFamily::FabricPredicate => DescriptorSource::FabricJson,
            RangeFamily::QuiltPredicate => DescriptorSource::QuiltJson,
        };
        dep_from(id, range, family, kind, source)
    }
    fn dep_from(
        id: &str,
        range: &str,
        family: RangeFamily,
        kind: DependencyKind,
        source: DescriptorSource,
    ) -> DeclaredDep {
        DeclaredDep {
            dep_id: id.into(),
            range: range.into(),
            kind,
            side: DepSide::Both,
            family,
            source,
        }
    }
    fn modz(sha: &str, provided: Vec<ProvidedMod>, deps: Vec<DeclaredDep>) -> ParsedMod {
        // Derived from DEPS ONLY. Providers must not contribute: these fixtures
        // pair a `ModsToml` provider with a `ModsToml` dep, and folding both in
        // would make a single-descriptor fixture look dual-descriptor and start
        // shadowing itself once `effective_rank` lands.
        let mut sources: Vec<DescriptorSource> = deps.iter().map(|d| d.source).collect();
        sources.dedup();
        modz_from(sha, provided, deps, sources)
    }
    fn modz_from(
        sha: &str,
        provided: Vec<ProvidedMod>,
        deps: Vec<DeclaredDep>,
        sources_present: Vec<DescriptorSource>,
    ) -> ParsedMod {
        ParsedMod {
            sha1: sha.into(),
            name: sha.to_uppercase(),
            manifest: ManifestDeps {
                provided,
                deps,
                sources_present,
                // This fixture builder feeds compatibility tests, not the
                // platform-lifting behaviour itself — no fixture here needs a
                // `platform` declaration of its own.
                platform: vec![],
            },
        }
    }
    fn prov(id: &str, ver: &str) -> ProvidedMod {
        // Pinned to `ModsToml` so it agrees with `dep_of`'s `RangeFamily::Maven`
        // default: a fixture must not become accidentally dual-descriptor once
        // `sources_present` starts driving shadowing.
        ProvidedMod {
            mod_id: id.into(),
            version: Some(ver.into()),
            source: DescriptorSource::ModsToml,
        }
    }

    #[test]
    fn headline_too_low_core_is_out_of_range() {
        let backpacks = modz(
            "a",
            vec![prov("backpacks", "3.20")],
            vec![dep("sophisticatedcore", "[1.3.51,)", RangeFamily::Maven)],
        );
        let core = modz("b", vec![prov("sophisticatedcore", "1.3.50.2005")], vec![]);
        let mods = vec![backpacks, core];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        let v = resolve(
            &mods,
            &index,
            LoaderKind::Forge,
            DescriptorEra::Modern,
            "1.20.1",
            None,
        );
        assert_eq!(v.len(), 1);
        assert!(
            matches!(&v[0], Violation::VersionOutOfRange { dep_id, installed, .. }
            if dep_id == "sophisticatedcore" && installed == "1.3.50.2005")
        );
    }

    #[test]
    fn missing_required_when_provider_absent() {
        let mods = vec![modz(
            "a",
            vec![prov("backpacks", "3.20")],
            vec![dep("sophisticatedcore", "[1.3.51,)", RangeFamily::Maven)],
        )];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(matches!(
            resolve(
                &mods,
                &index,
                LoaderKind::Forge,
                DescriptorEra::Modern,
                "1.20.1",
                None
            )[0],
            Violation::MissingRequired { .. }
        ));
    }

    #[test]
    fn jij_provider_suppresses_missing() {
        let mods = vec![modz(
            "a",
            vec![prov("backpacks", "3.20")],
            vec![dep("sophisticatedcore", "*", RangeFamily::Maven)],
        )];
        let index = ProviderIndex::build(
            &mods,
            &[("sophisticatedcore".into(), None)],
            LoaderKind::NeoForge,
            DescriptorEra::Modern,
        );
        assert!(resolve(
            &mods,
            &index,
            LoaderKind::Forge,
            DescriptorEra::Modern,
            "1.20.1",
            None
        )
        .is_empty());
        // present via JIJ, version unknown => silent
    }

    #[test]
    fn server_side_dep_not_enforced_on_client() {
        let mut d = dep("servercore", "[2,)", RangeFamily::Maven);
        d.side = DepSide::Server;
        let mods = vec![modz("a", vec![prov("x", "1")], vec![d])];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(resolve(
            &mods,
            &index,
            LoaderKind::Forge,
            DescriptorEra::Modern,
            "1.20.1",
            None
        )
        .is_empty());
    }

    #[test]
    fn satisfied_version_produces_no_violation() {
        let mods = vec![
            modz(
                "a",
                vec![prov("backpacks", "3.20")],
                vec![dep("sophisticatedcore", "[1.3.51,)", RangeFamily::Maven)],
            ),
            modz("b", vec![prov("sophisticatedcore", "1.3.55")], vec![]),
        ];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(resolve(
            &mods,
            &index,
            LoaderKind::Forge,
            DescriptorEra::Modern,
            "1.20.1",
            None
        )
        .is_empty());
    }

    /// FTB and ATLauncher sources have no per-mod browser; `dep_project_ref`
    /// must return `None` so pack-sourced mods never emit a bogus Modrinth link.
    #[test]
    fn ftb_and_atl_sources_yield_no_dep_project_ref() {
        use crate::mods::platform::ModSource;
        assert!(dep_project_ref(ModSource::Ftb, "some-opaque-pack-id").is_none());
        assert!(dep_project_ref(ModSource::Atlauncher, "some-opaque-pack-id").is_none());
        // Modrinth and CurseForge sources do produce a ref.
        assert!(dep_project_ref(ModSource::Modrinth, "aaabbb").is_some());
        assert!(dep_project_ref(ModSource::Curseforge, "12345").is_some());
    }

    /// IPC types must round-trip through JSON (Deserialize was added alongside Serialize).
    #[test]
    fn preflight_report_deserialize_round_trip() {
        use crate::mods::platform::DepProjectRef;
        let report = PreflightReport {
            violations: vec![DepViolation {
                dependent_sha1: "abc".into(),
                dependent_name: "Backpacks".into(),
                dep_id: "sophisticatedcore".into(),
                kind: ViolationKind::VersionOutOfRange,
                installed_version: Some("1.3.50".into()),
                needed: "[1.3.51,)".into(),
                needed_desc: crate::mods::range_describe::describe(
                    "[1.3.51,)",
                    crate::mods::version_range::RangeFamily::Maven,
                ),
                provider_project: Some(DepProjectRef::Modrinth {
                    project_id: "sc".into(),
                    version_id: None,
                }),
                provider_sha1: Some("abc123".into()),
                family: Some(crate::mods::version_range::RangeFamily::Maven),
            }],
            pack_completion: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: PreflightReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.violations.len(), 1);
        assert_eq!(back.violations[0].dep_id, "sophisticatedcore");
        assert!(matches!(
            back.violations[0].kind,
            ViolationKind::VersionOutOfRange
        ));
    }

    /// Build an in-memory `.jar` (zip) from (name, raw-bytes) entries — needed
    /// for the nested-jar case where an entry's body is itself a jar.
    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;
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

    fn installed_jar(
        filename: &str,
        sha1: &str,
        name: &str,
    ) -> crate::mods::platform::InstalledMod {
        crate::mods::platform::InstalledMod {
            filename: filename.into(),
            sha1: sha1.into(),
            source: None,
            project_id: None,
            version_id: None,
            name: name.into(),
            version_number: None,
            installed_at: "2026-06-16T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: vec![],
        }
    }

    #[tokio::test]
    async fn fabric_api_bundled_submodule_satisfies_indium_dependency() {
        use crate::mods::installed::{add, mods_dir};
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let inner = zip_bytes(&[(
            "fabric.mod.json",
            br#"{"id":"fabric-renderer-api-v1","version":"3.2.0"}"#,
        )]);
        let fabric_api = zip_bytes(&[
            (
                "fabric.mod.json",
                br#"{"id":"fabric-api","version":"0.100.0"}"#,
            ),
            ("META-INF/jars/fabric-renderer-api-v1.jar", &inner),
        ]);
        let indium = zip_bytes(&[(
            "fabric.mod.json",
            br#"{"id":"indium","version":"1.0.35","depends":{"fabric-renderer-api-v1":"*"}}"#,
        )]);

        tokio::fs::write(dir.join("fabric-api.jar"), &fabric_api)
            .await
            .unwrap();
        tokio::fs::write(dir.join("indium.jar"), &indium)
            .await
            .unwrap();
        add(
            td.path(),
            installed_jar("fabric-api.jar", "sha-fabricapi", "Fabric API"),
        )
        .await
        .unwrap();
        add(
            td.path(),
            installed_jar("indium.jar", "sha-indium", "Indium"),
        )
        .await
        .unwrap();

        let report = dependency_preflight_for_root(td.path(), LoaderKind::Fabric, "1.20.1", None)
            .await
            .unwrap();
        assert!(
            report.violations.is_empty(),
            "submodule bundled in Fabric API must satisfy the dep; got {:?}",
            report.violations
        );
    }

    #[test]
    fn provider_index_matches_across_underscore_hyphen() {
        // forgified-fabric-api provides `fabric_api`; a mod requires `fabric-api`.
        let mods = vec![
            modz("a", vec![prov("fabric_api", "0.116.7")], vec![]),
            modz(
                "b",
                vec![prov("continuity", "3.0.0")],
                vec![dep("fabric-api", "*", RangeFamily::FabricPredicate)],
            ),
        ];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(
            resolve(
                &mods,
                &index,
                LoaderKind::Fabric,
                DescriptorEra::Modern,
                "1.20.1",
                None
            )
            .is_empty(),
            "fabric-api must match fabric_api"
        );
    }

    #[test]
    fn provider_index_umbrella_alias_for_forgified_fabric_api() {
        // Only forgified-fabric-api's own id is provided; a mod requires the
        // `fabric-api` umbrella. The alias table must treat it as satisfied.
        let mods = vec![
            modz("a", vec![prov("forgified_fabric_api", "2.2.4")], vec![]),
            modz(
                "b",
                vec![prov("continuity", "3.0.0")],
                vec![dep("fabric-api", "*", RangeFamily::FabricPredicate)],
            ),
        ];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::NeoForge, DescriptorEra::Modern);
        assert!(
            resolve(
                &mods,
                &index,
                LoaderKind::Fabric,
                DescriptorEra::Modern,
                "1.20.1",
                None
            )
            .is_empty(),
            "fabric-api satisfied by forgified_fabric_api via alias"
        );
    }

    #[test]
    fn version_out_of_range_carries_provider_sha1() {
        use std::collections::HashMap;
        let v = Violation::VersionOutOfRange {
            dependent_sha1: "dep".into(),
            dependent_name: "Backpacks".into(),
            dep_id: "sophisticatedcore".into(),
            needed: "[1.3.51,)".into(),
            installed: "1.3.50".into(),
            family: crate::mods::version_range::RangeFamily::Maven,
        };
        let owner: HashMap<String, crate::mods::platform::DepProjectRef> = HashMap::new();
        let mut sha = HashMap::new();
        sha.insert("sophisticatedcore".to_string(), "PROVIDERSHA".to_string());
        let dv = enrich(v, &owner, &sha);
        assert_eq!(dv.provider_sha1.as_deref(), Some("PROVIDERSHA"));
        // family round-trips from the declared dep into the IPC violation.
        assert_eq!(
            dv.family,
            Some(crate::mods::version_range::RangeFamily::Maven)
        );
    }

    #[test]
    fn enrich_matches_provider_across_hyphen_underscore() {
        use std::collections::HashMap;
        // Provider maps are keyed via canon_id (fabric_api); the violation's
        // dep_id uses the hyphen form (fabric-api). Enrichment must still route
        // the provider link + sha1 by normalizing both sides.
        let v = Violation::VersionOutOfRange {
            dependent_sha1: "dep".into(),
            dependent_name: "Continuity".into(),
            dep_id: "fabric-api".into(),
            needed: "[0.100,)".into(),
            installed: "0.90".into(),
            family: crate::mods::version_range::RangeFamily::FabricPredicate,
        };
        let mut owner: HashMap<String, crate::mods::platform::DepProjectRef> = HashMap::new();
        owner.insert(
            "fabric_api".to_string(),
            crate::mods::platform::DepProjectRef::Modrinth {
                project_id: "P7dR8mSH".into(),
                version_id: None,
            },
        );
        let mut sha = HashMap::new();
        sha.insert("fabric_api".to_string(), "FABRICSHA".to_string());
        let dv = enrich(v, &owner, &sha);
        assert_eq!(dv.provider_sha1.as_deref(), Some("FABRICSHA"));
        assert!(
            dv.provider_project.is_some(),
            "fabric-api dep must resolve to the fabric_api provider ref"
        );
    }

    #[test]
    fn missing_required_has_no_provider_sha1() {
        use std::collections::HashMap;
        let v = Violation::MissingRequired {
            dependent_sha1: "a".into(),
            dependent_name: "A".into(),
            dep_id: "balm".into(),
        };
        let dv = enrich(v, &HashMap::new(), &HashMap::new());
        assert_eq!(dv.provider_sha1, None);
    }

    /// A required `platform` declaration (a `minecraft` or loader-id range),
    /// as opposed to `dep`/`dep_of`, which build ordinary dependency-graph
    /// declarations.
    fn platform_dep(id: &str, range: &str) -> DeclaredDep {
        DeclaredDep {
            dep_id: id.into(),
            range: range.into(),
            kind: DependencyKind::Required,
            side: DepSide::Both,
            family: RangeFamily::Maven,
            source: DescriptorSource::ModsToml,
        }
    }

    fn parsed_mod_with_platform(sha: &str, name: &str, platform: Vec<DeclaredDep>) -> ParsedMod {
        ParsedMod {
            sha1: sha.into(),
            name: name.into(),
            manifest: ManifestDeps {
                provided: vec![],
                deps: vec![],
                sources_present: vec![DescriptorSource::ModsToml],
                platform,
            },
        }
    }

    #[test]
    fn a_platform_violation_is_reported_and_blocking() {
        // BiomesOPlenty declares no `minecraft` block — only `forge [61.0.2,)`.
        // The instance runs Forge 47.4.10, so the loader axis is the only thing
        // that can catch it.
        let mods = vec![parsed_mod_with_platform(
            "aa",
            "BiomesOPlenty",
            vec![platform_dep("forge", "[61.0.2,)")],
        )];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::Forge, DescriptorEra::Modern);
        let out = resolve(
            &mods,
            &index,
            LoaderKind::Forge,
            DescriptorEra::Modern,
            "1.20.1",
            Some("47.4.10"),
        );
        assert!(
            out.iter()
                .any(|v| matches!(v, Violation::PlatformMismatch { .. })),
            "a forge range the instance cannot satisfy must produce a violation, got {out:?}"
        );
    }

    #[test]
    fn a_platform_violation_is_not_produced_without_a_loader_version() {
        // `raw_minecraft.rs:49` writes `loader_version: None` on a real import.
        // The loader axis must go silent there, never wrong.
        let mods = vec![parsed_mod_with_platform(
            "aa",
            "BiomesOPlenty",
            vec![platform_dep("forge", "[61.0.2,)")],
        )];
        let index = ProviderIndex::build(&mods, &[], LoaderKind::Forge, DescriptorEra::Modern);
        let out = resolve(
            &mods,
            &index,
            LoaderKind::Forge,
            DescriptorEra::Modern,
            "1.20.1",
            None,
        );
        assert!(
            out.is_empty(),
            "no loader version means no loader-axis verdict, got {out:?}"
        );
    }

    #[tokio::test]
    async fn missing_fabric_api_still_flags_submodule_dependency() {
        use crate::mods::installed::{add, mods_dir};
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let indium = zip_bytes(&[(
            "fabric.mod.json",
            br#"{"id":"indium","version":"1.0.35","depends":{"fabric-renderer-api-v1":"*"}}"#,
        )]);
        tokio::fs::write(dir.join("indium.jar"), &indium)
            .await
            .unwrap();
        add(
            td.path(),
            installed_jar("indium.jar", "sha-indium", "Indium"),
        )
        .await
        .unwrap();

        let report = dependency_preflight_for_root(td.path(), LoaderKind::Fabric, "1.20.1", None)
            .await
            .unwrap();
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert_eq!(report.violations[0].dep_id, "fabric-renderer-api-v1");
        assert!(matches!(
            report.violations[0].kind,
            ViolationKind::MissingRequired
        ));
    }

    /// `usable_hit` is the whole partial-record policy in one pure function, so it
    /// gets a direct test rather than only being exercised through the integration
    /// path in `tests/dependency_preflight.rs`.
    #[test]
    fn a_record_without_the_annotation_is_refused_only_by_the_era_that_needs_it() {
        use crate::mods::jar_scan_cache::CachedScan;

        let modern_written = CachedScan {
            meta: None,
            manifest: Some(ManifestDeps::default()),
            legacy_deps: None,
            jij_provided: Some(Vec::new()),
        };
        assert!(
            usable_hit(Some(&modern_written), false).is_some(),
            "a modern scan does not need the annotation and must not re-read the jar"
        );
        assert!(
            usable_hit(Some(&modern_written), true).is_none(),
            "a legacy scan must refuse a record that never opened the annotation"
        );

        let measured_empty = CachedScan {
            legacy_deps: Some(Vec::new()),
            ..modern_written.clone()
        };
        assert!(
            usable_hit(Some(&measured_empty), true).is_some(),
            "a MEASURED empty annotation is a fact and is believed"
        );

        // The half a pre-flight never writes is not the half it needs.
        let compat_written = CachedScan {
            meta: Some(crate::mods::local::JarMeta::default()),
            manifest: None,
            legacy_deps: None,
            jij_provided: None,
        };
        assert!(usable_hit(Some(&compat_written), false).is_none());
        assert!(usable_hit(None, false).is_none());
    }
}
