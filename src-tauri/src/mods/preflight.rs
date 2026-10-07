//! Pure dependency pre-flight resolver. No network, no disk. Given each
//! installed mod's parsed manifest and an index of available providers,
//! returns the required-dependency violations the loader would hit.
//!
//! The module also exposes [`parse_instance`] — one read of every registry
//! row, after which [`ParsedInstance::resolve`] answers for any enabled set —
//! and `dependency_preflight_for_root`, the testable core of the
//! `instance_dependency_preflight` Tauri command, along with the
//! `ViolationKind`, `DepViolation`, and `PreflightReport` IPC types.

use std::collections::{HashMap, HashSet};

use crate::instances::schema::LoaderKind;
use crate::mods::local::{
    DeclaredDep, DepSide, DependencyKind, DescriptorEra, ManifestDeps, ProvidedMod,
};
use crate::mods::mod_annotation::LegacyAnnotations;
use crate::mods::platform::{InstalledMod, ModSource};
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
pub(crate) fn canon_id(id: &str) -> String {
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
    /// A requirement nothing ENABLED provides but a DISABLED row does (own id,
    /// Jar-in-Jar id or umbrella alias). Disabled jars still satisfy nothing;
    /// this names the absence truthfully so the fix is "enable", not an install
    /// that would put a duplicate next to the switched-off jar.
    RequiredDisabled {
        dependent_sha1: String,
        dependent_name: String,
        dep_id: String,
        /// Registry digest of the first such disabled row, in registry order.
        provider_sha1: String,
    },
}

impl Violation {
    fn parts(&self) -> (&str, &str, &str) {
        match self {
            Self::MissingRequired {
                dependent_sha1,
                dependent_name,
                dep_id,
            }
            | Self::VersionOutOfRange {
                dependent_sha1,
                dependent_name,
                dep_id,
                ..
            }
            | Self::OptionalOutOfRange {
                dependent_sha1,
                dependent_name,
                dep_id,
                ..
            }
            | Self::IncompatibleInstalled {
                dependent_sha1,
                dependent_name,
                dep_id,
                ..
            }
            | Self::PlatformMismatch {
                dependent_sha1,
                dependent_name,
                dep_id,
                ..
            }
            | Self::RequiredDisabled {
                dependent_sha1,
                dependent_name,
                dep_id,
                ..
            } => (
                dependent_sha1.as_str(),
                dependent_name.as_str(),
                dep_id.as_str(),
            ),
        }
    }

    /// Registry digest of the mod that declared the dependency.
    pub fn dependent_sha1(&self) -> &str {
        self.parts().0
    }

    /// Display name of the mod that declared the dependency.
    pub fn dependent_name(&self) -> &str {
        self.parts().1
    }

    /// The dependency id the violation is about (`"minecraft"` or the loader's
    /// canonical id for a platform mismatch).
    pub fn dep_id(&self) -> &str {
        self.parts().2
    }
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
            // FML's own declaration outranks its display metadata. The
            // annotation is the requirement list AND the version every
            // requirement on that mod is measured against
            // (`FMLModContainer.bindMetadata`: annotation → `version.properties`
            // → `mcmod.info` → "1.0"); `mcmod.info` answers only for a mod-id no
            // annotation names a version for. `effective_rank` never shadows it,
            // because `McmodAnnotation` never enters `sources_present`.
            E::Legacy => match source {
                S::McmodAnnotation => Some(0),
                S::McmodInfo => Some(1),
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
/// "Strictly better", not "the single best": a file of equal rank never shadows
/// another. On the legacy era the annotation (rank 0) would shadow `mcmod.info`
/// (rank 1) by this rule, but it never can: `McmodAnnotation` is a class
/// annotation, not a file, and never enters `sources_present`. So `mcmod.info`
/// stays admitted and keeps answering for the mod-ids no annotation names a
/// version for — exactly FML's fallback.
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

/// The declarations of one jar that this instance's loader enforces — the ONE
/// admission test, shared by [`resolve`] and the version-fix planner so they can
/// never disagree about what a jar requires (audit A-F4):
///
/// - side first, matching `ModSorter.java:275` — a SERVER-only dep is invisible
///   to every check on the client we launch;
/// - only from a descriptor the loader opens for THIS jar ([`effective_rank`],
///   shadowing included): a Forge instance never loads `fabric.mod.json`, and a
///   1.12.2 one never loads `mods.toml`. Anything else is a declaration the
///   loader cannot enforce, so it is not a launch-readiness problem;
/// - never `Discouraged`: the loader only logs a warning and carries on.
pub(crate) fn active_deps_for(
    manifest: &ManifestDeps,
    loader: LoaderKind,
    era: DescriptorEra,
) -> impl Iterator<Item = &DeclaredDep> {
    manifest.deps.iter().filter(move |dep| {
        dep.side != DepSide::Server
            && effective_rank(dep.source, &manifest.sources_present, loader, era).is_some()
            && dep.kind != DependencyKind::Discouraged
    })
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
        for dep in active_deps_for(&m.manifest, loader, era) {
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
    /// A required dependency no enabled mod provides, that a disabled mod does.
    /// `provider_sha1` names that disabled jar.
    RequiredDisabled,
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
    /// What the loader would object to — see [`ViolationKind`].
    pub kind: ViolationKind,
    /// The version that is actually installed (`None` for `MissingRequired`
    /// and `RequiredDisabled`: nothing enabled provides it).
    pub installed_version: Option<String>,
    /// The version range the dependent declared (empty string for
    /// `MissingRequired` and `RequiredDisabled`), verbatim from the jar. Kept
    /// for remediation (`mods_filter_satisfying` evaluates it) and for the log
    /// line — the UI renders `needed_desc` instead, because raw Maven bracket
    /// notation is unreadable.
    pub needed: String,
    /// `needed`, decomposed into displayable clauses. The UI formats these
    /// through i18n and only falls back to the raw string when
    /// `needed_desc.unparseable`.
    pub needed_desc: crate::mods::range_describe::RangeDescription,
    /// Platform project reference for the provider, if we could link it.
    /// Powers a "View on Modrinth / CurseForge" link in the UI.
    pub provider_project: Option<crate::mods::platform::DepProjectRef>,
    /// SHA-1 of the jar that provides `dep_id`. For the range kinds: the
    /// ENABLED provider, so the UI routes "Обновить" through `mods_update_one`
    /// (remove-old + install-new) instead of a bare `mods_install_with_deps`
    /// that would leave duplicate jars. For `RequiredDisabled`: the DISABLED jar
    /// to switch back on. `None` otherwise.
    pub provider_sha1: Option<String>,
    /// The registry name of the row `provider_sha1` names — what the registry
    /// calls that mod, as `dependent_name` is what it calls the dependent — so
    /// a surface with no installed list of its own (the Play gate on a cold
    /// start) names the provider as a mod, never by its loader id. `None`
    /// exactly when `provider_sha1` is. `#[serde(default)]` so specta emits it
    /// optional.
    #[serde(default)]
    pub provider_name: Option<String>,
    /// Range grammar for `needed` (Maven / Fabric / Quilt). `None` for
    /// `MissingRequired` and `RequiredDisabled` (no range to interpret). Lets
    /// the UI pick a version that actually satisfies `needed` via
    /// `mods_filter_satisfying`.
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
    /// Registry digests of ENABLED mods whose jar was missing from disk or would
    /// not parse: their dependencies were not judged, so their silence is not "no
    /// problem". `#[serde(default)]` for the same reason as `pack_completion`.
    #[serde(default)]
    pub unjudged: Vec<String>,
}

/// A mod that loses something it needs when others leave the enabled set.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ImpactedMod {
    pub sha1: String,
    pub name: String,
    /// Display names (registry names — the project title for platform mods) of
    /// the leaving mods — targets, or other dependents — that provided what it
    /// loses.
    pub needs: Vec<String>,
}

/// What removing or disabling a set of mods breaks (`mods_removal_impact`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RemovalImpact {
    /// Enabled mods, other than the leaving ones, that gain a violation: those
    /// the targets' leaving breaks and, to a fixed point, those that break once
    /// the earlier ones are off too. With all of them off as well, no enabled mod
    /// has gained a violation. Empty means nothing the pre-flight can read loses
    /// anything it needs.
    ///
    /// In a safe disable order among themselves: a mod comes before any listed
    /// mod it needs — one whose jar answers a requirement the loader enforces
    /// on it — save inside a cycle, which no order keeps whole: it is broken at
    /// its earliest mod. Of the mods free to go next, the earliest in wave order
    /// (what breaks directly first), each wave in registry order, goes first.
    /// Switched off together with the targets, they follow `order`: a target
    /// may need one of them.
    pub dependents: Vec<ImpactedMod>,
    /// The targets and every mod in `dependents`, as registry digests, each
    /// once, in ONE SAFE DISABLE ORDER: switched off one by one as listed, a
    /// mod goes off before any of them it needs, so a run that stops early
    /// leaves none of them on without one it needs — save inside a cycle,
    /// broken at its earliest mod. Of the mods free to go next, the earliest
    /// goes first — the dependents as listed, then the targets in registry
    /// order — so where no target needs a dependent, this is `dependents`,
    /// then the targets. Filtered to any subset — the targets alone, for "only
    /// these" — it is still a safe order for that subset.
    pub order: Vec<String>,
}

/// A disabled mod that must be switched on together with the ones being enabled.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DisabledRequirement {
    pub sha1: String,
    pub name: String,
}

/// What enabling a set of mods needs switched on with them (`mods_enable_impact`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct EnableImpact {
    /// Disabled mods the targets need, transitively, each once, never a target
    /// itself.
    ///
    /// In a safe enable order among themselves: a mod comes after any listed
    /// mod it needs — one whose jar answers a requirement the loader enforces
    /// on it — save inside a cycle, which no order keeps whole: it is broken at
    /// its earliest mod. Of the mods free to go next, the one found first goes
    /// first. Switched on together with the targets, they follow `order`: one
    /// of them may need a target.
    pub requirements: Vec<DisabledRequirement>,
    /// The targets and every mod in `requirements`, as registry digests, each
    /// once, in ONE SAFE ENABLE ORDER: switched on one by one as listed, a mod
    /// comes on after any of them it needs, so a run that stops early leaves
    /// none of them on without one it needs — save inside a cycle, broken at
    /// its earliest mod. Of the mods free to go next, the earliest goes first —
    /// the requirements as listed, then the targets in registry order — so
    /// where no requirement needs a target, this is `requirements`, then the
    /// targets. Filtered to any subset — the targets alone, for "only these" —
    /// it is still a safe order for that subset.
    pub order: Vec<String>,
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
            provider_name: None,
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
        Violation::RequiredDisabled {
            dependent_sha1,
            dependent_name,
            dep_id,
            provider_sha1,
        } => DepViolation {
            dependent_sha1,
            dependent_name,
            dep_id,
            kind: ViolationKind::RequiredDisabled,
            installed_version: None,
            needed: String::new(),
            // As for `MissingRequired`: nothing enabled provides it, so there is
            // no range to read against.
            needed_desc: crate::mods::range_describe::describe(
                "",
                crate::mods::version_range::RangeFamily::Maven,
            ),
            // No platform link: the fix is switching this jar back on, by digest.
            provider_project: None,
            provider_sha1: Some(provider_sha1),
            provider_name: None,
            family: None,
        },
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
            provider_name: None,
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
        provider_name: None,
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
    /// What the `@Mod` annotations declare. Empty on a modern-era instance,
    /// where the reader never runs — and deliberately NOT stored as a fact
    /// there; see [`usable_hit`].
    legacy: LegacyAnnotations,
    jij_provided: Vec<crate::mods::local::ProvidedMod>,
}

impl JarScan {
    /// The jar's manifest with the annotation's requirements AND providers
    /// folded in, plus its Jar-in-Jar providers. The ONE merge point: the panel
    /// (`parse_instance`) and the version-fix planner (`scan_loose_jar`) both go
    /// through it, so they cannot come to disagree about what a legacy jar
    /// provides or at which version.
    fn into_parts(self) -> (ManifestDeps, Vec<ProvidedMod>) {
        let mut manifest = self.manifest;
        manifest.deps.extend(self.legacy.deps);
        manifest.provided.extend(self.legacy.provided);
        (manifest, self.jij_provided)
    }
}

/// A cached record turned into a complete [`JarScan`], or `None` when it does
/// not answer everything this era asks.
///
/// The `legacy` branch is the point of the whole `Option` shape. A record
/// written while scanning a MODERN instance carries `legacy: None` — "never
/// scanned", not "this jar declares none" — and a 1.12.2 instance that believed
/// it would report zero requirements for every mod in the pack, because the
/// annotation is the ONLY place that era declares them. A modern instance,
/// symmetrically, does not care what the annotation says and must not be sent
/// back to the jar to find out.
fn usable_hit(
    hit: Option<&crate::mods::jar_scan_cache::CachedScan>,
    want_legacy: bool,
) -> Option<JarScan> {
    let hit = hit?;
    let manifest = hit.manifest.clone()?;
    let jij_provided = hit.jij_provided.clone()?;
    let legacy = if want_legacy {
        hit.legacy.clone()?
    } else {
        LegacyAnnotations::default()
    };
    Some(JarScan {
        manifest,
        legacy,
        jij_provided,
    })
}

/// Read every descriptor this era needs out of one jar's bytes, OFF the async
/// executor.
///
/// `zip` is sync, and this is not a small sync call: `read_jar_manifest_deps`
/// inflates several entries, `read_jar_embedded_providers` recursively unzips
/// every nested jar, and on the legacy era `read_jar_legacy_annotations`
/// decompresses every class entry FML would scan. Run inline on a tokio worker, a
/// 140-mod pack starves every other task on that worker — including the
/// download and launch pipelines. One blocking task for the WHOLE jar rather
/// than one per reader, mirroring `datapacks::library::install_local_at`, which
/// offloads classify + read_meta + sha1 together for the same reason. The
/// caller's loop stays sequential, so at most one blocking task exists at a
/// time and the blocking pool is never flooded.
///
/// `None` is "could not tell" — an unreadable zip, a blocking task that
/// panicked or was cancelled, or on the legacy era a class the annotation
/// reader could not inflate or parse (see
/// `mod_annotation::read_jar_legacy_annotations`) — and it is never written to
/// the cache: a failure to read must not be frozen into "this jar declares
/// nothing".
async fn scan_jar(bytes: Vec<u8>, want_legacy: bool) -> Option<JarScan> {
    use crate::mods::local::{read_jar_embedded_providers, read_jar_manifest_deps};
    use crate::mods::mod_annotation::read_jar_legacy_annotations;
    let joined = tokio::task::spawn_blocking(move || {
        let manifest = read_jar_manifest_deps(&bytes).ok()?;
        // `None` from the annotation reader is "could not tell", and it makes
        // the whole scan "could not tell" — which is never cached.
        let legacy = if want_legacy {
            read_jar_legacy_annotations(&bytes)?
        } else {
            LegacyAnnotations::default()
        };
        let jij_provided = read_jar_embedded_providers(&bytes);
        Some(JarScan {
            manifest,
            legacy,
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

/// A jar that is not installed — a candidate build in the download cache — read
/// the way [`parse_instance`] reads an installed one.
#[derive(Debug, Clone)]
pub struct LooseJar {
    /// Legacy-era `@Mod` requirements and providers already merged in.
    pub manifest: ManifestDeps,
    pub jij_provided: Vec<ProvidedMod>,
}

/// `None` is "could not tell" (an unreadable zip), exactly as for an installed
/// jar — never "declares nothing".
pub(crate) async fn scan_loose_jar(bytes: Vec<u8>, era: DescriptorEra) -> Option<LooseJar> {
    let scan = scan_jar(bytes, era == DescriptorEra::Legacy).await?;
    let (manifest, jij_provided) = scan.into_parts();
    Some(LooseJar {
        manifest,
        jij_provided,
    })
}

/// The version `jar` would answer `dep_id` with, under the index's own version
/// authority ([`ProviderIndex::build`]). `None`: it does not provide the id, or
/// names no readable version — the pre-flight skips the range check then too.
pub(crate) fn provided_version(
    jar: &LooseJar,
    dep_id: &str,
    loader: LoaderKind,
    era: DescriptorEra,
) -> Option<String> {
    let one = [ParsedMod {
        sha1: String::new(),
        name: String::new(),
        manifest: jar.manifest.clone(),
    }];
    let jij: Vec<(String, Option<String>)> = jar
        .jij_provided
        .iter()
        .map(|p| (p.mod_id.clone(), p.version.clone()))
        .collect();
    ProviderIndex::build(&one, &jij, loader, era)
        .get(dep_id)
        .cloned()
        .flatten()
}

/// One registry row joined with what its jar says, kept PER ROW — its own
/// Jar-in-Jar providers included — so a resolution over any subset of the
/// instance ("without these", "with this one switched on") comes from one parse.
/// An ownerless JIJ list would keep a leaving jar's embedded libraries alive
/// (audit A-F1).
#[derive(Debug, Clone)]
pub struct ParsedRow {
    /// Registry digest and name; the manifest has the legacy-era `@Mod`
    /// requirements merged into `deps`.
    pub parsed: ParsedMod,
    /// Jar-in-Jar providers: an embedded library answers only for an id
    /// nothing top-level claims, and only while its host is switched on.
    pub jij_provided: Vec<ProvidedMod>,
    /// The registry's flag. A disabled row is parsed in full, but its
    /// declarations are never READ AS requirements unless the row enters the
    /// counterfactual enabled set a caller hands [`ParsedInstance::resolve`]
    /// (e.g. [`ParsedInstance::enable_impact`]). Outside that set it only
    /// answers what it would provide (`RequiredDisabled`).
    pub enabled: bool,
    pub source: Option<ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub version_number: Option<String>,
}

/// Every registry row the pre-flight could read, in registry order — the order
/// every "first provider wins" tie has always been broken in — plus the context
/// a resolution needs. Built once by [`parse_instance`]; everything after is pure.
#[derive(Debug, Clone)]
pub struct ParsedInstance {
    pub rows: Vec<ParsedRow>,
    /// Rows whose jar was missing from disk or would not parse — the two places
    /// the scan cannot tell what a jar says. Never guessed into `rows`.
    pub unreadable: Vec<InstalledMod>,
    pub loader: LoaderKind,
    pub era: DescriptorEra,
    pub mc: String,
    pub loader_version: Option<String>,
}

impl ParsedInstance {
    /// The rows the registry has switched on.
    pub fn registry_enabled(&self) -> HashSet<String> {
        self.rows
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.parsed.sha1.clone())
            .collect()
    }

    /// The rows in `enabled`, in registry order.
    fn active<'a>(
        &'a self,
        enabled: &'a HashSet<String>,
    ) -> impl Iterator<Item = &'a ParsedRow> + 'a {
        self.rows
            .iter()
            .filter(move |r| enabled.contains(&r.parsed.sha1))
    }

    /// The violations the loader would hit with exactly `enabled` switched on.
    /// With the registry's enabled set this is the pre-flight; with any other set
    /// it is the counterfactual the impact checks ask for. The index — JIJ
    /// included — is rebuilt from `enabled` alone, and rows outside it never emit.
    pub fn resolve(&self, enabled: &HashSet<String>) -> Vec<Violation> {
        let mods: Vec<ParsedMod> = self.active(enabled).map(|r| r.parsed.clone()).collect();
        // Jar-in-Jar providers, so an embedded lib is not falsely flagged as a
        // missing dependency — but only those whose host is in `enabled`.
        let jij: Vec<(String, Option<String>)> = self
            .active(enabled)
            .flat_map(|r| {
                r.jij_provided
                    .iter()
                    .map(|p| (p.mod_id.clone(), p.version.clone()))
            })
            .collect();
        let index = ProviderIndex::build(&mods, &jij, self.loader, self.era);
        // The module-level resolver, not this method.
        self::resolve(
            &mods,
            &index,
            self.loader,
            self.era,
            &self.mc,
            self.loader_version.as_deref(),
        )
        .into_iter()
        .map(|v| self.explain_absence(v, enabled))
        .collect()
    }

    /// A `MissingRequired` whose id a readable row OUTSIDE `enabled` provides
    /// becomes `RequiredDisabled`, naming the first such row in registry order.
    /// An unreadable jar is not in `rows`, so it can never be claimed as the
    /// provider (§9): that absence stays `MissingRequired`, today's answer.
    fn explain_absence(&self, v: Violation, enabled: &HashSet<String>) -> Violation {
        match v {
            Violation::MissingRequired {
                dependent_sha1,
                dependent_name,
                dep_id,
            } => match self
                .rows
                .iter()
                .find(|r| !enabled.contains(&r.parsed.sha1) && row_provides(r, &dep_id))
            {
                Some(p) => Violation::RequiredDisabled {
                    dependent_sha1,
                    dependent_name,
                    dep_id,
                    provider_sha1: p.parsed.sha1.clone(),
                },
                None => Violation::MissingRequired {
                    dependent_sha1,
                    dependent_name,
                    dep_id,
                },
            },
            other => other,
        }
    }

    /// Enabled rows the scan could not read — exactly the two skip sites (jar
    /// missing from disk, zip that would not parse). A disabled row is never
    /// judged, so it is never "unjudged" either.
    pub fn unjudged(&self) -> Vec<String> {
        self.unreadable
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.sha1.clone())
            .collect()
    }

    /// The parsed row with this registry digest.
    pub(crate) fn row(&self, sha1: &str) -> Option<&ParsedRow> {
        self.rows.iter().find(|r| r.parsed.sha1 == sha1)
    }

    /// `ModsNotFound` — the answer `mods_enable` gives for the same digest —
    /// when the registry does not list one of `targets` (gone since the caller
    /// read it), readable or not: an impact of a mod that is not there would be
    /// a guess, never "nothing".
    fn refuse_unlisted(&self, targets: &HashSet<String>) -> crate::error::Result<()> {
        let listed =
            |t: &String| self.row(t).is_some() || self.unreadable.iter().any(|m| &m.sha1 == t);
        if targets.iter().all(listed) {
            Ok(())
        } else {
            Err(crate::error::Error::ModsNotFound {
                platform: "installed".into(),
            })
        }
    }

    /// Every enabled mod, other than `targets`, that breaks when `targets` leave
    /// the enabled set — removed or disabled alike (spec §5.1) — and, to a fixed
    /// point, every mod that breaks once THOSE are switched off too: the dialog
    /// offers to switch all of them off, and nothing left on may be broken by
    /// it. A mod breaks when it gains a violation the registry's enabled set
    /// does not have. Each names the leaving mods — targets, or dependents of an
    /// earlier wave — that provided what it lost.
    ///
    /// Order: the dependents are safe to switch off one by one as listed, among
    /// themselves. A mod comes before any listed mod it needs
    /// ([`Self::flip_order`]) — save inside a cycle, which no order keeps whole
    /// and which is broken at its earliest mod. Of the mods free to go next, the
    /// earliest in today's order goes first: wave by wave — what breaks
    /// directly, then what breaks once that is off too — each wave in registry
    /// order, each mod once, in the wave it breaks in. `order` places the
    /// targets among them ([`Self::with_targets`]) — a target may need a
    /// dependent, and targets may need each other — so a run that switches off
    /// `order`, or any part of it, and stops early leaves none of them on
    /// without one it needs, save inside a cycle.
    ///
    /// Monotone: once broken, a mod stays counted, even where a later wave takes
    /// away the provider whose version broke it (an optional or incompatible
    /// declaration, which absence satisfies) — the restrictive answer.
    ///
    /// Errors when an ENABLED target's jar could not be read: what it provides
    /// is unknown, so "nothing depends on it" would be a guess. A disabled
    /// target satisfies nothing today, so its leaving is known to break nothing.
    /// Errors, too, when the registry does not list a target.
    pub fn removal_impact(&self, targets: &HashSet<String>) -> crate::error::Result<RemovalImpact> {
        if let Some(m) = self
            .unreadable
            .iter()
            .find(|m| m.enabled && targets.contains(&m.sha1))
        {
            return Err(crate::error::Error::io(
                m.filename.clone(),
                "the jar could not be read, so what depends on it is unknown",
            ));
        }
        self.refuse_unlisted(targets)?;
        let enabled = self.registry_enabled();
        let before = self.resolve(&enabled);
        // The targets and every dependent found so far — all of them off.
        let mut gone: HashSet<String> = targets.clone();
        let mut dependents: Vec<ImpactedMod> = Vec::new();
        // Each wave adds at least one row not yet in `gone` (`newly_broken` never
        // names one that is), or it is empty and the loop ends. Every row it names
        // is a registry row, so the rows bound the fixed point.
        loop {
            let wave = self.newly_broken(&enabled, &gone, &before);
            if wave.is_empty() {
                break;
            }
            gone.extend(wave.iter().map(|d| d.sha1.clone()));
            dependents.extend(wave);
        }
        let sha1s: Vec<&str> = dependents.iter().map(|d| d.sha1.as_str()).collect();
        let at = self.flip_order(&sha1s, Flip::Off);
        let dependents = reordered(dependents, &at);
        let listed: Vec<&str> = dependents.iter().map(|d| d.sha1.as_str()).collect();
        let order = self.with_targets(&listed, targets, Flip::Off);
        Ok(RemovalImpact { dependents, order })
    }

    /// The one order to switch `listed` — dependents or requirements, already in
    /// a safe order among themselves — and `targets` together, as registry
    /// digests ([`Self::flip_order`]). Today's order is `listed`, then the
    /// targets in registry order (the readable rows, then the unreadable ones):
    /// where no need puts a target ahead of a listed mod, `listed` keeps its
    /// order and the targets follow, ordered among themselves where they need
    /// each other. Every target is named, readable or not: the caller switches
    /// exactly what this names.
    fn with_targets(&self, listed: &[&str], targets: &HashSet<String>, flip: Flip) -> Vec<String> {
        let in_registry_order = self
            .rows
            .iter()
            .map(|r| r.parsed.sha1.as_str())
            .chain(self.unreadable.iter().map(|m| m.sha1.as_str()))
            .filter(|s| targets.contains(*s));
        let all: Vec<&str> = listed.iter().copied().chain(in_registry_order).collect();
        let at = self.flip_order(&all, flip);
        reordered(all, &at).into_iter().map(String::from).collect()
    }

    /// The order to switch `sha1s` — registry digests, in today's order — one by
    /// one, as indices into them ([`safe_flip_order`]): switching off, a mod goes
    /// before any of them it needs; switching on, after.
    ///
    /// A mod NEEDS another when the other's jar answers one of its REQUIRED
    /// declarations the loader enforces: the admission test of [`resolve`]
    /// ([`active_deps_for`]) against the provider test of `explain_absence`
    /// ([`row_provides`] — own ids, embedded libraries, aliases). Static: it reads
    /// the two jars and nothing else, so a third provider that could cover for
    /// the other does not undo the need. Optional and incompatible declarations
    /// are satisfied by absence and never order a flip — counted, a library's
    /// optional integration with a mod that requires it would read as a cycle.
    /// A digest with no parsed row — a jar that could not be read — needs
    /// nothing and answers nothing, so no need orders it.
    fn flip_order(&self, sha1s: &[&str], flip: Flip) -> Vec<usize> {
        let rows: Vec<Option<&ParsedRow>> = sha1s.iter().map(|s| self.row(s)).collect();
        // Once per mod, not once per pair.
        let provides: Vec<HashSet<String>> = rows
            .iter()
            .map(|&r| r.map(provided_ids).unwrap_or_default())
            .collect();
        let requires: Vec<Vec<&str>> = rows
            .iter()
            .map(|&r| {
                r.map(|r| {
                    active_deps_for(&r.parsed.manifest, self.loader, self.era)
                        .filter(|d| d.kind.is_required())
                        .map(|d| d.dep_id.as_str())
                        .collect::<Vec<&str>>()
                })
                .unwrap_or_default()
            })
            .collect();
        let needs = |x: usize, y: usize| requires[x].iter().any(|d| ids_provide(&provides[y], d));
        safe_flip_order(sha1s.len(), |a, b| match flip {
            Flip::Off => needs(a, b),
            Flip::On => needs(b, a),
        })
    }

    /// One wave of [`Self::removal_impact`]: the rows of `enabled` outside
    /// `gone` that gain a violation over `before`, in registry order, each
    /// naming the `gone` rows that provided what it lost.
    fn newly_broken(
        &self,
        enabled: &HashSet<String>,
        gone: &HashSet<String>,
        before: &[Violation],
    ) -> Vec<ImpactedMod> {
        let remaining: HashSet<String> = enabled.difference(gone).cloned().collect();
        let mut wave: Vec<ImpactedMod> = Vec::new();
        for v in self.resolve(&remaining) {
            // Only what the leaving mods cause, and never about a mod that is
            // leaving too — the check the fixed point's termination rests on.
            if before.contains(&v) || gone.contains(v.dependent_sha1()) {
                continue;
            }
            let at = match wave.iter().position(|d| d.sha1 == v.dependent_sha1()) {
                Some(i) => i,
                None => {
                    wave.push(ImpactedMod {
                        sha1: v.dependent_sha1().to_string(),
                        name: v.dependent_name().to_string(),
                        needs: Vec::new(),
                    });
                    wave.len() - 1
                }
            };
            let lost = self
                .rows
                .iter()
                .filter(|r| gone.contains(&r.parsed.sha1) && row_provides(r, v.dep_id()));
            for r in lost {
                if !wave[at].needs.contains(&r.parsed.name) {
                    wave[at].needs.push(r.parsed.name.clone());
                }
            }
        }
        wave
    }

    /// The `candidates` that can leave together with `targets` without a mod
    /// that stays enabled gaining a violation — the check under an orphan offer
    /// (A14). `orphans::find_orphans` trusts the registry's `requires` edges,
    /// which can be incomplete; an offer must never name a library another mod
    /// still needs.
    ///
    /// Greedy, in the order given: each candidate is judged with the targets AND
    /// every candidate kept before it already gone, so the kept set is harmless
    /// removed WHOLE — two libraries that cover for each other are never both
    /// offered. Only what a candidate adds counts: what the targets break is the
    /// removal's own impact. A candidate whose ENABLED jar could not be read may
    /// provide anything, and one this parse does not list is gone or changed
    /// since the caller read the registry — neither is kept, because an offer
    /// claims that nothing needs the mod. Known limit: an OTHER enabled mod whose
    /// jar could not be read declares nothing the resolver sees, so its needs are
    /// not protected here; the pre-flight reports it as unjudged.
    pub fn removable_with(&self, targets: &HashSet<String>, candidates: &[String]) -> Vec<String> {
        let mut gone = targets.clone();
        let mut before = self.resolve_without(&gone);
        let mut kept = Vec::new();
        for c in candidates {
            let judged =
                self.row(c).is_some() || self.unreadable.iter().any(|m| &m.sha1 == c && !m.enabled);
            if !judged {
                continue;
            }
            let mut next = gone.clone();
            next.insert(c.clone());
            let after = self.resolve_without(&next);
            // Every violation `after` holds is about a mod that stays enabled:
            // rows outside the set never emit.
            if after.iter().any(|v| !before.contains(v)) {
                continue;
            }
            gone = next;
            before = after;
            kept.push(c.clone());
        }
        kept
    }

    /// The violations of this instance with the `gone` rows removed outright.
    /// Not `resolve` over a smaller enabled set: a removed jar must not linger
    /// as the disabled provider a `RequiredDisabled` names — once it is gone,
    /// switching it on is no longer a way out.
    fn resolve_without(&self, gone: &HashSet<String>) -> Vec<Violation> {
        let rest = ParsedInstance {
            rows: self
                .rows
                .iter()
                .filter(|r| !gone.contains(&r.parsed.sha1))
                .cloned()
                .collect(),
            unreadable: self
                .unreadable
                .iter()
                .filter(|m| !gone.contains(&m.sha1))
                .cloned()
                .collect(),
            loader: self.loader,
            era: self.era,
            mc: self.mc.clone(),
            loader_version: self.loader_version.clone(),
        };
        rest.resolve(&rest.registry_enabled())
    }

    /// The disabled mods `targets` need switched on with them, transitively:
    /// enabling a requirement may reveal its own. Only requirements of the
    /// targets and of what they pull in count — another mod's disabled
    /// dependency is not theirs — and a target is never its own requirement.
    ///
    /// Order: the requirements are safe to switch on one by one as listed,
    /// among themselves. A mod comes after any listed mod it needs
    /// ([`Self::flip_order`]) — save inside a cycle, which no order keeps whole
    /// and which is broken at its earliest mod. Of the mods free to go next, the
    /// one found first goes first. `order` places the targets among them
    /// ([`Self::with_targets`]) — a requirement may need a target, and targets
    /// may need each other — so a run that switches on `order`, or any part of
    /// it, and stops early leaves none of them on without one it needs, save
    /// inside a cycle.
    ///
    /// Errors when a target's jar could not be read, or the registry does not
    /// list it: "none" would be a guess.
    pub fn enable_impact(&self, targets: &HashSet<String>) -> crate::error::Result<EnableImpact> {
        if let Some(m) = self.unreadable.iter().find(|m| targets.contains(&m.sha1)) {
            return Err(crate::error::Error::io(
                m.filename.clone(),
                "the jar could not be read, so what it requires is unknown",
            ));
        }
        self.refuse_unlisted(targets)?;
        let mut enabled = self.registry_enabled();
        enabled.extend(targets.iter().cloned());
        let mut asking: HashSet<String> = targets.clone();
        let mut pulled: Vec<String> = Vec::new();
        // Each round switches on at least one more row or stops: a
        // `RequiredDisabled` only ever names a row OUTSIDE `enabled`. There are
        // no more rows than the registry holds — the fixed point's cap.
        for _ in 0..self.rows.len() {
            let fresh: Vec<String> = self
                .resolve(&enabled)
                .into_iter()
                .filter_map(|v| match v {
                    Violation::RequiredDisabled {
                        dependent_sha1,
                        provider_sha1,
                        ..
                    } if asking.contains(&dependent_sha1) => Some(provider_sha1),
                    _ => None,
                })
                .collect();
            if fresh.is_empty() {
                break;
            }
            for p in fresh {
                // Two targets needing one provider name it twice: listed once.
                if enabled.insert(p.clone()) {
                    asking.insert(p.clone());
                    pulled.push(p);
                }
            }
        }
        let requirements: Vec<DisabledRequirement> = pulled
            .iter()
            .filter_map(|p| self.row(p))
            .map(|r| DisabledRequirement {
                sha1: r.parsed.sha1.clone(),
                name: r.parsed.name.clone(),
            })
            .collect();
        let sha1s: Vec<&str> = requirements.iter().map(|r| r.sha1.as_str()).collect();
        let at = self.flip_order(&sha1s, Flip::On);
        let requirements = reordered(requirements, &at);
        let listed: Vec<&str> = requirements.iter().map(|r| r.sha1.as_str()).collect();
        let order = self.with_targets(&listed, targets, Flip::On);
        Ok(EnableImpact {
            requirements,
            order,
        })
    }

    /// [`Self::resolve`], enriched for the UI. The provider maps come from the
    /// same `enabled` set, so a jar outside it can never be where «Обновить»
    /// (`ranged()`) is routed. Each violation names its provider's row
    /// ([`Self::with_provider_name`]).
    pub fn report(&self, enabled: &HashSet<String>) -> Vec<DepViolation> {
        let (owner, by_id) = self.provider_maps(enabled);
        self.resolve(enabled)
            .into_iter()
            .map(|v| self.with_provider_name(enrich(v, &owner, &by_id)))
            .collect()
    }

    /// `v` with `provider_name`: the registry name of the row `provider_sha1`
    /// names — a range's enabled provider, or the disabled jar of a
    /// `RequiredDisabled` — the same source `dependent_name` is read from, so a
    /// surface with no installed list of its own names both mods alike. Every
    /// digest `provider_sha1` can hold is a parsed row's (the provider maps and
    /// `explain_absence` read nothing else), so `None` here means no provider.
    fn with_provider_name(&self, v: DepViolation) -> DepViolation {
        let provider_name = v
            .provider_sha1
            .as_deref()
            .and_then(|sha1| self.row(sha1))
            .map(|r| r.parsed.name.clone());
        DepViolation { provider_name, ..v }
    }

    /// Canon provided id → platform ref / registry digest of the first row in
    /// registry order that provides it. Top-level ids only: an embedded library
    /// has no row of its own to update or link to.
    fn provider_maps(
        &self,
        enabled: &HashSet<String>,
    ) -> (
        HashMap<String, crate::mods::platform::DepProjectRef>,
        HashMap<String, String>,
    ) {
        let mut owner = HashMap::new();
        let mut by_id = HashMap::new();
        for r in self.active(enabled) {
            // Populated regardless of source, so even FTB/ATL mods (which yield
            // no DepProjectRef) still route updates. The REGISTRY digest, not
            // the on-disk one: it routes UI actions (`mods_update_one`, row
            // identity) against `installed-mods.json`. Canonicalized
            // ('-'/'_' equivalent, lowercase) like `enrich`'s lookup, so a
            // `fabric-api` dep matches a `fabric_api` provider.
            for p in &r.parsed.manifest.provided {
                by_id
                    .entry(canon_id(&p.mod_id))
                    .or_insert_with(|| r.parsed.sha1.clone());
            }
            // FTB/ATLauncher yield no ref (no per-mod browser) and must not
            // create a spurious link.
            let ref_ = r
                .source
                .zip(r.project_id.as_deref())
                .and_then(|(s, pid)| dep_project_ref(s, pid));
            if let Some(ref_) = ref_ {
                for p in &r.parsed.manifest.provided {
                    owner
                        .entry(canon_id(&p.mod_id))
                        .or_insert_with(|| ref_.clone());
                }
            }
        }
        (owner, by_id)
    }

    /// The enabled row «Обновить» is routed to for `dep_id` — the same
    /// first-in-registry-order owner `ranged()` reports as `provider_sha1`.
    /// `None` when only an embedded (JIJ) copy provides it.
    pub(crate) fn provider_row(
        &self,
        enabled: &HashSet<String>,
        dep_id: &str,
    ) -> Option<&ParsedRow> {
        let (_, by_id) = self.provider_maps(enabled);
        by_id.get(&canon_id(dep_id)).and_then(|sha| self.row(sha))
    }
}

/// Would `row`, switched on, answer for `dep_id`? The same three routes
/// [`ProviderIndex::is_provided`] accepts: an own id, an embedded (JIJ) id, or an
/// umbrella alias of either.
fn row_provides(row: &ParsedRow, dep_id: &str) -> bool {
    ids_provide(&provided_ids(row), dep_id)
}

/// The canonical ids `row`, switched on, answers for: its own and its embedded
/// (JIJ) ones — the first half of [`row_provides`], kept apart so a caller asking
/// about many requirements reads each row once.
fn provided_ids(row: &ParsedRow) -> HashSet<String> {
    row.parsed
        .manifest
        .provided
        .iter()
        .chain(&row.jij_provided)
        .map(|p| canon_id(&p.mod_id))
        .collect()
}

/// Do `ids` — a row's [`provided_ids`] — answer for `dep_id`, directly or
/// through an umbrella alias? The second half of [`row_provides`].
fn ids_provide(ids: &HashSet<String>, dep_id: &str) -> bool {
    let key = canon_id(dep_id);
    ids.contains(&key)
        || PROVIDES_ALIASES
            .iter()
            .find(|(name, _)| *name == key)
            .is_some_and(|(_, aliases)| aliases.iter().any(|a| ids.contains(*a)))
}

/// Which way a list of mods is switched, one mod at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flip {
    /// Switched off: a mod goes before any listed mod it needs.
    Off,
    /// Switched on: a mod goes after any listed mod it needs.
    On,
}

/// A stable order to switch `n` mods one at a time: a permutation of `0..n`,
/// where index order is today's order and `goes_first(a, b)` says `a` must be
/// switched before `b`.
///
/// Each step takes the EARLIEST remaining index that waits on nothing outside
/// its own cycle — no remaining index must go before it, or every one that must
/// is itself waiting on it, transitively. So an order that already respects
/// every pair comes back unchanged (ties keep today's order), and a cycle, which
/// no order respects whole, is broken at its earliest member that nothing
/// outside the cycle must precede: only that cycle's own pairs are broken.
/// Deterministic — plain indices, no hashing.
fn safe_flip_order(n: usize, goes_first: impl Fn(usize, usize) -> bool) -> Vec<usize> {
    // `after[a]`: what `a` must go before; `before[b]`: what must go before `b`.
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut before: Vec<Vec<usize>> = vec![Vec::new(); n];
    for a in 0..n {
        for b in (0..n).filter(|&b| b != a && goes_first(a, b)) {
            after[a].push(b);
            before[b].push(a);
        }
    }
    let mut placed = vec![false; n];
    let mut order = Vec::with_capacity(n);
    // Each round places one index not placed before, so the loop ends after `n`.
    while let Some(i) = next_to_flip(&before, &after, &placed) {
        placed[i] = true;
        order.push(i);
    }
    order
}

/// The step [`safe_flip_order`] takes next; `None` once every index is placed.
fn next_to_flip(before: &[Vec<usize>], after: &[Vec<usize>], placed: &[bool]) -> Option<usize> {
    let remaining = || (0..placed.len()).filter(|&i| !placed[i]);
    remaining()
        .find(|&i| waits_only_on_its_cycle(i, before, after, placed))
        // Never taken: walking back along "must go before" through the remaining
        // indices ends in a cycle nothing outside precedes, and its members
        // qualify. Kept so every index is still placed exactly once.
        .or_else(|| remaining().next())
}

/// Is every remaining index that must go before `i` one that `i` must go
/// before too, through remaining indices — do they share a cycle? True when
/// none remains.
fn waits_only_on_its_cycle(
    i: usize,
    before: &[Vec<usize>],
    after: &[Vec<usize>],
    placed: &[bool],
) -> bool {
    let waits_on: Vec<usize> = before[i].iter().copied().filter(|&p| !placed[p]).collect();
    if waits_on.is_empty() {
        return true;
    }
    // Everything `i` must go before, transitively, through remaining indices.
    let mut reached = vec![false; placed.len()];
    let mut stack = vec![i];
    while let Some(v) = stack.pop() {
        for &w in &after[v] {
            if !placed[w] && !reached[w] {
                reached[w] = true;
                stack.push(w);
            }
        }
    }
    waits_on.iter().all(|&p| reached[p])
}

/// `items` in `order` — indices into `items`, as [`safe_flip_order`] returns.
/// An index `order` does not name keeps its place after them: nothing is dropped.
fn reordered<T>(items: Vec<T>, order: &[usize]) -> Vec<T> {
    let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let mut out: Vec<T> = order
        .iter()
        .filter_map(|&i| slots.get_mut(i).and_then(Option::take))
        .collect();
    out.extend(slots.into_iter().flatten());
    out
}

/// One row's jar: through the cache when its record answers this era, else read
/// from disk, queuing a fresh record in `fresh`. `None` is "could not tell": no
/// jar under either spelling, or a zip that would not parse.
async fn scan_row(
    mods_dir: &std::path::Path,
    m: &InstalledMod,
    cached: &crate::mods::jar_scan_cache::ScanCache,
    want_legacy: bool,
    fresh: &mut Vec<(String, crate::mods::jar_scan_cache::CachedScan)>,
) -> Option<JarScan> {
    // The cache key is the digest of the bytes on disk RIGHT NOW, never
    // `m.sha1`: the registry deliberately keeps a record's EXPECTED digest when
    // the file under that name was replaced, so keying on it would serve the
    // previous jar's dependencies for the current one. `None` — no file, or a
    // digest we could not compute — means no cache participation for this jar
    // in either direction.
    let cache_key = crate::mods::installed::on_disk_sha1(mods_dir, &m.filename).await;
    if let Some(s) = usable_hit(
        cache_key.as_deref().and_then(|k| cached.get(k)),
        want_legacy,
    ) {
        return Some(s);
    }
    // Jar missing from disk: "could not tell", reported by the caller.
    let bytes = crate::mods::local::read_jar_for(mods_dir, &m.filename).await?;
    // Unreadable zip: the same.
    let s = scan_jar(bytes, want_legacy).await?;
    // Only a key computed from the real bytes may be written: without one we do
    // not know WHICH jar this record describes.
    if let Some(k) = cache_key {
        fresh.push((
            k,
            crate::mods::jar_scan_cache::CachedScan {
                // Not read on this path. `None` says exactly that, leaving
                // `local::scan_instance`'s half of the record for whoever
                // measures it.
                meta: None,
                manifest: Some(s.manifest.clone()),
                // `Some(empty)` only when the reader actually ran. A
                // modern-era scan stores `None`, so a later legacy-era scan
                // re-reads instead of believing an emptiness nobody measured.
                legacy: want_legacy.then(|| s.legacy.clone()),
                jij_provided: Some(s.jij_provided.clone()),
            },
        ));
    }
    Some(s)
}

/// Parse every registry row — enabled AND disabled — once. Unchanged cost for
/// enabled mods (same cache, same readers); a disabled jar is read through the
/// `.disabled` spelling `read_jar_for` / `on_disk_sha1` already try.
///
/// `cache_path` is the shared jar-scan cache (`paths::jar_scan_cache_file`).
/// `None` runs the scan uncached: the same answer, only slower. That is the
/// deliberate direction — a data root that cannot be resolved right now must
/// not fail a check sitting in the launch chokepoint.
pub async fn parse_instance(
    root: &std::path::Path,
    cache_path: Option<&std::path::Path>,
    loader: LoaderKind,
    mc: &str,
    loader_version: Option<&str>,
) -> crate::error::Result<ParsedInstance> {
    use crate::mods::jar_scan_cache::ScanCache;

    // Which descriptor this instance's loader actually opens. Decided by the
    // instance's MC version, never by which files a jar happens to ship.
    let era = crate::mods::local::descriptor_era(mc);
    let want_legacy = era == DescriptorEra::Legacy;

    let installed = crate::mods::installed::list(root).await?;
    let mods_dir = crate::mods::installed::mods_dir(root);

    // One read of a small JSON for the whole scan, then a map lookup per jar —
    // the shape `l10n::coverage::scan_instance` uses.
    let cached = cache_path.map(ScanCache::load).unwrap_or_default();
    let mut fresh = Vec::new();
    let mut rows = Vec::new();
    let mut unreadable = Vec::new();
    for m in installed {
        let Some(scan) = scan_row(&mods_dir, &m, &cached, want_legacy, &mut fresh).await else {
            // Skipped, never fatal — one bad jar must not fail the whole scan —
            // and recorded, so nobody reads the silence as "declares nothing".
            unreadable.push(m);
            continue;
        };
        // On the legacy era the requirements AND the compared versions live in
        // the `@Mod` annotation: `mcmod.info` is display metadata FML only falls
        // back to. Empty on every other era.
        let (manifest, jij_provided) = scan.into_parts();
        rows.push(ParsedRow {
            parsed: ParsedMod {
                sha1: m.sha1,
                name: m.name,
                manifest,
            },
            jij_provided,
            enabled: m.enabled,
            source: m.source,
            project_id: m.project_id,
            version_id: m.version_id,
            version_number: m.version_number,
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

    Ok(ParsedInstance {
        rows,
        unreadable,
        loader,
        era,
        mc: mc.to_string(),
        loader_version: loader_version.map(str::to_string),
    })
}

/// The testable core of the `instance_dependency_preflight` Tauri command.
/// Accepts a resolved `instance_root` path so integration tests can call it
/// without a `tauri::AppHandle`. `cache_path` as for [`parse_instance`].
pub async fn dependency_preflight_for_root(
    root: &std::path::Path,
    cache_path: Option<&std::path::Path>,
    loader: LoaderKind,
    mc: &str,
    loader_version: Option<&str>,
) -> crate::error::Result<PreflightReport> {
    let parsed = parse_instance(root, cache_path, loader, mc, loader_version).await?;
    Ok(PreflightReport {
        violations: parsed.report(&parsed.registry_enabled()),
        pack_completion: crate::mods::pack_completion::read(root),
        unjudged: parsed.unjudged(),
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

        // `mcmod.info` is never shadowed on the legacy era: the annotation
        // outranks it but, not being a file, never appears in `sources_present`.
        let legacy = [S::McmodInfo];
        assert!(effective_rank(S::McmodInfo, &legacy, L::Forge, E::Legacy).is_some());
        assert!(effective_rank(S::McmodAnnotation, &legacy, L::Forge, E::Legacy).is_some());
    }

    /// FML ≤ 1.12.2 measures a requirement against the `@Mod` annotation's
    /// `version`, not `mcmod.info`'s (`FMLModContainer.bindMetadata`). Measured on
    /// OreLib: annotation `3.6.0.1`, `mcmod.info` `1.12.2-3.6.0.1` — believing the
    /// second flagged a pack that starts.
    #[test]
    fn a_legacy_provider_version_comes_from_the_annotation_before_mcmod_info() {
        use crate::mods::local::{DescriptorEra as E, DescriptorSource as S};
        let pv = |ver: Option<&str>, source| ProvidedMod {
            mod_id: "orelib".into(),
            version: ver.map(str::to_string),
            source,
        };
        // Either order inside the jar: the rank decides, not the position.
        for provided in [
            vec![
                pv(Some("1.12.2-3.6.0.1"), S::McmodInfo),
                pv(Some("3.6.0.1"), S::McmodAnnotation),
            ],
            vec![
                pv(Some("3.6.0.1"), S::McmodAnnotation),
                pv(Some("1.12.2-3.6.0.1"), S::McmodInfo),
            ],
        ] {
            let mods = vec![modz_from("aa", provided, vec![], vec![S::McmodInfo])];
            let idx = ProviderIndex::build(&mods, &[], LoaderKind::Forge, E::Legacy);
            assert_eq!(idx.get("orelib"), Some(&Some("3.6.0.1".to_string())));
        }
        // An annotation naming no version leaves `mcmod.info` to answer — FML's
        // step 3. `sources_present` holding `McmodInfo` and never
        // `McmodAnnotation` is what keeps that fallback admitted.
        let silent = vec![modz_from(
            "bb",
            vec![
                pv(None, S::McmodAnnotation),
                pv(Some("7.0.1"), S::McmodInfo),
            ],
            vec![],
            vec![S::McmodInfo],
        )];
        let idx = ProviderIndex::build(&silent, &[], LoaderKind::Forge, E::Legacy);
        assert_eq!(idx.get("orelib"), Some(&Some("7.0.1".to_string())));
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
        // On the legacy era FML compares a requirement against the annotation's
        // `version` and falls back to `mcmod.info`'s only when there is none.
        assert!(
            descriptor_rank(S::McmodAnnotation, L::Forge, E::Legacy)
                < descriptor_rank(S::McmodInfo, L::Forge, E::Legacy)
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
                provider_name: Some("Sophisticated Core".into()),
                family: Some(crate::mods::version_range::RangeFamily::Maven),
            }],
            pack_completion: None,
            unjudged: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: PreflightReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.violations.len(), 1);
        assert_eq!(back.violations[0].dep_id, "sophisticatedcore");
        assert_eq!(
            back.violations[0].provider_name.as_deref(),
            Some("Sophisticated Core")
        );
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

        let report =
            dependency_preflight_for_root(td.path(), None, LoaderKind::Fabric, "1.20.1", None)
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

        let report =
            dependency_preflight_for_root(td.path(), None, LoaderKind::Fabric, "1.20.1", None)
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
            legacy: None,
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
            legacy: Some(LegacyAnnotations::default()),
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
            legacy: None,
            jij_provided: None,
        };
        assert!(usable_hit(Some(&compat_written), false).is_none());
        assert!(usable_hit(None, false).is_none());
    }

    // ── the split: one parse, any enabled set ─────────────────────────────

    fn row(m: ParsedMod, enabled: bool) -> ParsedRow {
        ParsedRow {
            parsed: m,
            jij_provided: vec![],
            enabled,
            source: None,
            project_id: None,
            version_id: None,
            version_number: None,
        }
    }
    fn instance(rows: Vec<ParsedRow>) -> ParsedInstance {
        ParsedInstance {
            rows,
            unreadable: vec![],
            loader: LoaderKind::NeoForge,
            era: DescriptorEra::Modern,
            mc: "1.20.1".into(),
            loader_version: None,
        }
    }
    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// The pre-split algorithm, kept as the oracle: enabled rows only, their JIJ
    /// flattened into one ownerless list, first-in-registry-order provider maps —
    /// the post-parse half of `dependency_preflight_for_root` as it stood before
    /// the split, over the SAME free `resolve`, `ProviderIndex::build` and
    /// `enrich` it called — plus `provider_name`, added since: the name of the
    /// row `provider_sha1` names, an enabled one, the only kind of provider
    /// there was then.
    fn pre_split_report(inst: &ParsedInstance) -> Vec<DepViolation> {
        let on: Vec<&ParsedRow> = inst.rows.iter().filter(|r| r.enabled).collect();
        let mods: Vec<ParsedMod> = on.iter().map(|r| r.parsed.clone()).collect();
        let mut jij = Vec::new();
        let mut owner = HashMap::new();
        let mut sha = HashMap::new();
        for r in &on {
            for p in &r.jij_provided {
                jij.push((p.mod_id.clone(), p.version.clone()));
            }
            for p in &r.parsed.manifest.provided {
                sha.entry(canon_id(&p.mod_id))
                    .or_insert_with(|| r.parsed.sha1.clone());
            }
            if let (Some(s), Some(pid)) = (r.source, r.project_id.as_deref()) {
                if let Some(rf) = dep_project_ref(s, pid) {
                    for p in &r.parsed.manifest.provided {
                        owner
                            .entry(canon_id(&p.mod_id))
                            .or_insert_with(|| rf.clone());
                    }
                }
            }
        }
        let index = ProviderIndex::build(&mods, &jij, inst.loader, inst.era);
        resolve(&mods, &index, inst.loader, inst.era, &inst.mc, None)
            .into_iter()
            .map(|v| enrich(v, &owner, &sha))
            .map(|v| {
                let provider_name = v
                    .provider_sha1
                    .as_deref()
                    .and_then(|s| on.iter().find(|r| r.parsed.sha1 == s))
                    .map(|r| r.parsed.name.clone());
                DepViolation { provider_name, ..v }
            })
            .collect()
    }

    /// Disabled rows are parsed now, yet with the registry's enabled set the
    /// answer is exactly the old one. `pnew` is DISABLED, listed FIRST and would
    /// satisfy `d` — if it leaked into the index, the provider maps or the
    /// dependents, this report would change.
    #[test]
    fn resolving_the_registry_enabled_set_reproduces_the_pre_split_report() {
        let mut pnew = row(modz("pnew", vec![prov("core", "9.0")], vec![]), false);
        pnew.source = Some(ModSource::Modrinth);
        pnew.project_id = Some("CORE-NEW".into());
        let mut p = row(modz("p", vec![prov("core", "1.0")], vec![]), true);
        p.source = Some(ModSource::Modrinth);
        p.project_id = Some("CORE".into());
        let d = row(
            modz("d", vec![], vec![dep("core", "[2.0,)", RangeFamily::Maven)]),
            true,
        );
        let mut j = row(modz("j", vec![prov("host", "1.0")], vec![]), true);
        j.jij_provided = vec![prov("lib", "3.0")];
        let u = row(
            modz("u", vec![], vec![dep("lib", "[4.0,)", RangeFamily::Maven)]),
            true,
        );
        let x = row(
            modz("x", vec![], vec![dep("ghost", "", RangeFamily::Maven)]),
            false,
        );
        let m = row(
            modz("m", vec![], vec![dep("absent", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![pnew, p, d, j, u, x, m]);

        let got = inst.report(&inst.registry_enabled());
        assert_eq!(format!("{got:?}"), format!("{:?}", pre_split_report(&inst)));
        let summary: Vec<(&str, &str, ViolationKind, Option<&str>)> = got
            .iter()
            .map(|v| {
                let k = v.kind.clone();
                (
                    v.dependent_sha1.as_str(),
                    v.dep_id.as_str(),
                    k,
                    v.provider_sha1.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("d", "core", ViolationKind::VersionOutOfRange, Some("p")),
                ("u", "lib", ViolationKind::VersionOutOfRange, None),
                ("m", "absent", ViolationKind::MissingRequired, None),
            ]
        );
        assert!(matches!(
            &got[0].provider_project,
            Some(crate::mods::platform::DepProjectRef::Modrinth { project_id, .. }) if project_id == "CORE"
        ));
    }

    /// Audit A-F1: an embedded library belongs to its host. With the host out of
    /// the enabled set, the library is gone too.
    #[test]
    fn a_hosts_embedded_library_leaves_the_index_with_its_host() {
        let mut host = row(modz("a", vec![prov("host", "1.0")], vec![]), true);
        host.jij_provided = vec![prov("lib", "1.0")];
        let user = row(
            modz("u", vec![], vec![dep("lib", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![host, user]);
        assert!(
            inst.resolve(&set(&["a", "u"])).is_empty(),
            "the embedded lib satisfies u"
        );
        let without = inst.resolve(&set(&["u"]));
        assert_eq!(without.len(), 1, "{without:?}");
        assert_eq!(
            (without[0].dependent_sha1(), without[0].dep_id()),
            ("u", "lib")
        );
    }

    /// One admission test, each exclusion by exactly one rule.
    #[test]
    fn active_deps_for_is_the_one_admission_test() {
        use crate::mods::local::DescriptorSource as S;
        let neo = |id: &str, kind| dep_from(id, "", RangeFamily::Maven, kind, S::NeoForgeToml);
        let mut server = neo("serveronly", DependencyKind::Required);
        server.side = DepSide::Server;
        let shadowed = dep_from(
            "shadowed",
            "",
            RangeFamily::Maven,
            DependencyKind::Required,
            S::ModsToml,
        );
        let m = modz_from(
            "aa",
            vec![],
            vec![
                server,
                shadowed,
                neo("discouraged", DependencyKind::Discouraged),
                neo("real", DependencyKind::Required),
                neo("opt", DependencyKind::Optional),
            ],
            vec![S::NeoForgeToml, S::ModsToml],
        );
        let ids: Vec<&str> =
            active_deps_for(&m.manifest, LoaderKind::NeoForge, DescriptorEra::Modern)
                .map(|d| d.dep_id.as_str())
                .collect();
        assert_eq!(ids, ["real", "opt"]);
    }

    // ── a disabled provider, and what could not be judged ────────────────

    #[test]
    fn a_requirement_only_a_disabled_mod_provides_is_required_disabled() {
        let p = row(modz("p", vec![prov("core", "1.0")], vec![]), false);
        let d = row(
            modz("d", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![p, d]);
        let got = inst.report(&inst.registry_enabled());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].kind, ViolationKind::RequiredDisabled);
        assert_eq!(got[0].provider_sha1.as_deref(), Some("p"));
        assert_eq!(
            (got[0].installed_version.as_deref(), got[0].family),
            (None, None)
        );
    }

    #[test]
    fn a_disabled_mods_embedded_library_and_aliases_count_as_what_it_provides() {
        let mut host = row(modz("h", vec![prov("host", "1.0")], vec![]), false);
        host.jij_provided = vec![prov("lib", "2.0")];
        let d = row(
            modz("d", vec![], vec![dep("lib", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![host, d]);
        assert!(matches!(
            inst.resolve(&inst.registry_enabled()).as_slice(),
            [Violation::RequiredDisabled { provider_sha1, .. }] if provider_sha1 == "h"
        ));

        let ff = row(
            modz("ff", vec![prov("forgified_fabric_api", "2.2")], vec![]),
            false,
        );
        let c = row(
            modz(
                "c",
                vec![],
                vec![dep("fabric-api", "*", RangeFamily::FabricPredicate)],
            ),
            true,
        );
        let mut fabric = instance(vec![ff, c]);
        fabric.loader = LoaderKind::Fabric;
        assert!(matches!(
            fabric.resolve(&fabric.registry_enabled()).as_slice(),
            [Violation::RequiredDisabled { provider_sha1, .. }] if provider_sha1 == "ff"
        ));
    }

    /// Plan §5b V1 (07c): the Play gate on a cold start has no installed list
    /// to name a provider from, so the report names it — the registry's name for
    /// the row `provider_sha1` names, enabled (a range) or disabled (a jar to
    /// switch on). No provider row, no name: an absent id, or one only an
    /// embedded library answers.
    #[test]
    fn a_violation_names_the_row_its_provider_sha1_names() {
        let p = row(modz("p", vec![prov("core", "1.0")], vec![]), true);
        let off = row(modz("off", vec![prov("lib", "1.0")], vec![]), false);
        let mut host = row(modz("host", vec![prov("host", "1.0")], vec![]), true);
        host.jij_provided = vec![prov("inner", "1.0")];
        let d = row(
            modz(
                "d",
                vec![],
                vec![
                    dep("core", "[2.0,)", RangeFamily::Maven),
                    dep("lib", "", RangeFamily::Maven),
                    dep("inner", "[2.0,)", RangeFamily::Maven),
                    dep("absent", "", RangeFamily::Maven),
                ],
            ),
            true,
        );
        let inst = instance(vec![p, off, host, d]);
        let got = inst.report(&inst.registry_enabled());
        let named = |id: &str| {
            let v = got
                .iter()
                .find(|v| v.dep_id == id)
                .unwrap_or_else(|| panic!("no violation for {id}: {got:?}"));
            (v.provider_sha1.as_deref(), v.provider_name.as_deref())
        };
        assert_eq!(named("core"), (Some("p"), Some("P")));
        assert_eq!(named("lib"), (Some("off"), Some("OFF")));
        assert_eq!(named("inner"), (None, None));
        assert_eq!(named("absent"), (None, None));
    }

    #[test]
    fn an_enabled_provider_is_never_upstaged_by_a_disabled_one() {
        let off = row(modz("off", vec![prov("core", "9.0")], vec![]), false);
        let on = row(modz("on", vec![prov("core", "1.0")], vec![]), true);
        let d = row(
            modz("d", vec![], vec![dep("core", "[2.0,)", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![off, on, d]);
        let got = inst.report(&inst.registry_enabled());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].kind, ViolationKind::VersionOutOfRange);
        assert_eq!(got[0].provider_sha1.as_deref(), Some("on"));
    }

    #[test]
    fn required_disabled_and_unjudged_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&ViolationKind::RequiredDisabled).unwrap(),
            "\"required_disabled\""
        );
        let old: PreflightReport = serde_json::from_str(r#"{"violations":[]}"#).unwrap();
        assert!(
            old.unjudged.is_empty(),
            "a report written before the field reads as all-judged"
        );
    }

    #[tokio::test]
    async fn a_disabled_fabric_api_is_named_as_the_provider_of_its_submodule() {
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
        // Switched off: `.disabled` on disk, `enabled: false` in the registry.
        tokio::fs::write(dir.join("fabric-api.jar.disabled"), &fabric_api)
            .await
            .unwrap();
        tokio::fs::write(dir.join("indium.jar"), &indium)
            .await
            .unwrap();
        let mut off = installed_jar("fabric-api.jar", "sha-fabricapi", "Fabric API");
        off.enabled = false;
        add(td.path(), off).await.unwrap();
        add(
            td.path(),
            installed_jar("indium.jar", "sha-indium", "Indium"),
        )
        .await
        .unwrap();

        let report =
            dependency_preflight_for_root(td.path(), None, LoaderKind::Fabric, "1.20.1", None)
                .await
                .unwrap();
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert_eq!(report.violations[0].kind, ViolationKind::RequiredDisabled);
        assert_eq!(
            report.violations[0].provider_sha1.as_deref(),
            Some("sha-fabricapi")
        );
        assert!(report.unjudged.is_empty());
    }

    /// §9: an unreadable disabled jar is "could not tell", never "it is there,
    /// switched off" — the row keeps today's `MissingRequired`.
    #[tokio::test]
    async fn an_unreadable_disabled_jar_is_not_claimed_as_a_provider() {
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
        tokio::fs::write(dir.join("fabric-api.jar.disabled"), b"not a zip")
            .await
            .unwrap();
        let mut off = installed_jar("fabric-api.jar", "sha-fabricapi", "Fabric API");
        off.enabled = false;
        add(td.path(), off).await.unwrap();
        add(
            td.path(),
            installed_jar("indium.jar", "sha-indium", "Indium"),
        )
        .await
        .unwrap();

        let report =
            dependency_preflight_for_root(td.path(), None, LoaderKind::Fabric, "1.20.1", None)
                .await
                .unwrap();
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert_eq!(report.violations[0].kind, ViolationKind::MissingRequired);
        assert!(report.unjudged.is_empty(), "a disabled jar is never judged");
    }

    /// Unjudged = exactly the two skip sites, enabled rows only. A jar that
    /// parses and declares nothing IS judged.
    #[tokio::test]
    async fn unjudged_names_exactly_the_enabled_jars_that_could_not_be_read() {
        use crate::mods::installed::{add, mods_dir};
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let quiet = zip_bytes(&[("fabric.mod.json", br#"{"id":"quiet","version":"1.0"}"#)]);
        tokio::fs::write(dir.join("quiet.jar"), &quiet)
            .await
            .unwrap();
        add(td.path(), installed_jar("quiet.jar", "sha-quiet", "Quiet"))
            .await
            .unwrap();
        tokio::fs::write(dir.join("broken.jar"), b"not a zip")
            .await
            .unwrap();
        add(
            td.path(),
            installed_jar("broken.jar", "sha-broken", "Broken"),
        )
        .await
        .unwrap();
        tokio::fs::write(dir.join("off.jar.disabled"), b"not a zip either")
            .await
            .unwrap();
        let mut off = installed_jar("off.jar", "sha-off", "Off");
        off.enabled = false;
        add(td.path(), off).await.unwrap();

        let report =
            dependency_preflight_for_root(td.path(), None, LoaderKind::Fabric, "1.20.1", None)
                .await
                .unwrap();
        assert_eq!(report.unjudged, vec!["sha-broken".to_string()]);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    // ── impact ────────────────────────────────────────────────────────────

    #[test]
    fn removing_a_provider_names_each_dependent_and_only_what_it_loses() {
        let a = row(modz("a", vec![prov("core", "1.0")], vec![]), true);
        let b = row(modz("b", vec![prov("lib", "1.0")], vec![]), true);
        let d = row(
            modz(
                "d",
                vec![],
                vec![
                    dep("core", "", RangeFamily::Maven),
                    dep("lib", "", RangeFamily::Maven),
                ],
            ),
            true,
        );
        let e = row(
            modz("e", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let f = row(modz("f", vec![prov("other", "1.0")], vec![]), true);
        let inst = instance(vec![a, b, d, e, f]);
        let hit = |sha1: &str, name: &str, needs: &[&str]| ImpactedMod {
            sha1: sha1.into(),
            name: name.into(),
            needs: needs.iter().map(|n| n.to_string()).collect(),
        };
        let impact = |t: &[&str]| inst.removal_impact(&set(t)).unwrap().dependents;
        assert_eq!(
            impact(&["a"]),
            vec![hit("d", "D", &["A"]), hit("e", "E", &["A"])]
        );
        assert_eq!(impact(&["a", "b"])[0], hit("d", "D", &["A", "B"]));
        assert_eq!(
            impact(&["a", "d"]),
            vec![hit("e", "E", &["A"])],
            "a dependent leaving too is not warned about"
        );
        assert!(impact(&["f"]).is_empty());
    }

    #[test]
    fn a_second_provider_or_an_inert_declaration_is_no_impact() {
        let a = row(modz("a", vec![prov("core", "1.0")], vec![]), true);
        let b = row(modz("b", vec![prov("core", "1.0")], vec![]), true);
        let d = row(
            modz("d", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        // fabric.mod.json: NeoForge never opens it, so this requirement does not exist here.
        let x = row(
            modz(
                "x",
                vec![],
                vec![dep("core", "*", RangeFamily::FabricPredicate)],
            ),
            true,
        );
        let inst = instance(vec![a, b, d, x]);
        assert!(
            inst.removal_impact(&set(&["a"]))
                .unwrap()
                .dependents
                .is_empty(),
            "b still provides core"
        );
        let both: Vec<String> = inst
            .removal_impact(&set(&["a", "b"]))
            .unwrap()
            .dependents
            .into_iter()
            .map(|m| m.sha1)
            .collect();
        assert_eq!(both, ["d"], "x's declaration is inert on NeoForge");
    }

    #[test]
    fn an_embedded_library_leaves_with_its_host() {
        let mut host = row(modz("a", vec![prov("host", "1.0")], vec![]), true);
        host.jij_provided = vec![prov("lib", "1.0")];
        let u = row(
            modz("u", vec![], vec![dep("lib", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![host, u]);
        let got = inst.removal_impact(&set(&["a"])).unwrap().dependents;
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(
            (got[0].sha1.as_str(), got[0].needs.as_slice()),
            ("u", &["A".to_string()][..])
        );
    }

    /// The umbrella alias counts on the way out too: a target that answers
    /// `fabric-api` only as `forgified_fabric_api` leaves it unmet, and the
    /// dependent names that target as what it loses.
    #[test]
    fn a_provider_known_only_by_an_alias_is_what_its_dependent_loses() {
        let ff = row(
            modz("ff", vec![prov("forgified_fabric_api", "2.2")], vec![]),
            true,
        );
        let c = row(
            modz(
                "c",
                vec![],
                vec![dep("fabric-api", "*", RangeFamily::FabricPredicate)],
            ),
            true,
        );
        let mut inst = instance(vec![ff, c]);
        inst.loader = LoaderKind::Fabric;
        assert!(
            inst.resolve(&inst.registry_enabled()).is_empty(),
            "installed, the alias satisfies the requirement"
        );
        assert_eq!(
            inst.removal_impact(&set(&["ff"])).unwrap().dependents,
            vec![ImpactedMod {
                sha1: "c".into(),
                name: "C".into(),
                needs: vec!["FF".into()],
            }]
        );
    }

    /// §9 Q1/Q2: an ENABLED jar we could not read may provide anything, so what
    /// breaks without it is unknown — an error, never "no dependents". A
    /// DISABLED one satisfies nothing, so its leaving is known to break nothing.
    #[test]
    fn removal_impact_will_not_guess_for_an_enabled_jar_it_could_not_read() {
        let d = row(
            modz("d", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let mut inst = instance(vec![d]);
        let on = installed_jar("on.jar", "on", "On");
        let mut off = installed_jar("off.jar", "off", "Off");
        off.enabled = false;
        inst.unreadable = vec![on, off];
        assert!(inst.removal_impact(&set(&["on"])).is_err());
        assert!(inst
            .removal_impact(&set(&["off"]))
            .unwrap()
            .dependents
            .is_empty());
    }

    /// A target the registry no longer lists (removed since the UI read it) is
    /// "could not tell", not "breaks nothing" — the `ModsNotFound`
    /// `enable_impact` answers with for the same digest.
    #[test]
    fn removal_impact_refuses_a_mod_the_registry_does_not_know() {
        let a = row(modz("a", vec![prov("core", "1.0")], vec![]), true);
        let d = row(
            modz("d", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![a, d]);
        assert!(matches!(
            inst.removal_impact(&set(&["a", "ghost"])),
            Err(crate::error::Error::ModsNotFound { platform }) if platform == "installed"
        ));
    }

    /// The `ImpactedMod` a `modz` fixture yields: its name is its sha1 in capitals.
    fn impacted(sha1: &str, needs: &[&str]) -> ImpactedMod {
        ImpactedMod {
            sha1: sha1.into(),
            name: sha1.to_uppercase(),
            needs: needs.iter().map(|n| n.to_string()).collect(),
        }
    }

    /// S ← A ← B: switching A off with S is not enough, because B needs A. The
    /// dialog's «Disable all» exists so that what remains still launches, so B is
    /// named too — and it names A, the mod it loses, not S. B is listed first: it
    /// needs A, so it goes off before A does.
    #[test]
    fn removing_a_mod_also_names_what_breaks_once_its_dependents_are_off() {
        let s = row(modz("s", vec![prov("smod", "1.0")], vec![]), true);
        let a = row(
            modz(
                "a",
                vec![prov("amod", "1.0")],
                vec![dep("smod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let b = row(
            modz("b", vec![], vec![dep("amod", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![s, a, b]);
        assert_eq!(
            inst.removal_impact(&set(&["s"])).unwrap().dependents,
            vec![impacted("b", &["A"]), impacted("a", &["S"])]
        );
    }

    /// S ← L, S ← R, and D needs both: D breaks once, when L and R leave
    /// together, and names both — listed before them, since it needs both.
    #[test]
    fn a_mod_two_dependents_feed_is_named_once_with_both() {
        let s = row(modz("s", vec![prov("smod", "1.0")], vec![]), true);
        let l = row(
            modz(
                "l",
                vec![prov("lmod", "1.0")],
                vec![dep("smod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let r = row(
            modz(
                "r",
                vec![prov("rmod", "1.0")],
                vec![dep("smod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let d = row(
            modz(
                "d",
                vec![],
                vec![
                    dep("lmod", "", RangeFamily::Maven),
                    dep("rmod", "", RangeFamily::Maven),
                ],
            ),
            true,
        );
        let inst = instance(vec![s, l, r, d]);
        assert_eq!(
            inst.removal_impact(&set(&["s"])).unwrap().dependents,
            vec![
                impacted("d", &["L", "R"]),
                impacted("l", &["S"]),
                impacted("r", &["S"]),
            ]
        );
    }

    /// A needs S and C, B needs A, C needs B — a cycle among the dependents: the
    /// fixed point ends, and each mod is named once, with what it lost in the
    /// wave it broke in. No order keeps a cycle whole: it is broken at A, the
    /// earliest, and C, which needs B, still goes off before B.
    #[test]
    fn a_cycle_among_dependents_ends_with_each_named_once() {
        let s = row(modz("s", vec![prov("smod", "1.0")], vec![]), true);
        let a = row(
            modz(
                "a",
                vec![prov("amod", "1.0")],
                vec![
                    dep("smod", "", RangeFamily::Maven),
                    dep("cmod", "", RangeFamily::Maven),
                ],
            ),
            true,
        );
        let b = row(
            modz(
                "b",
                vec![prov("bmod", "1.0")],
                vec![dep("amod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let c = row(
            modz(
                "c",
                vec![prov("cmod", "1.0")],
                vec![dep("bmod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let inst = instance(vec![s, a, b, c]);
        assert_eq!(
            inst.removal_impact(&set(&["s"])).unwrap().dependents,
            vec![
                impacted("a", &["S"]),
                impacted("c", &["B"]),
                impacted("b", &["A"]),
            ]
        );
    }

    /// Only what really breaks is carried along: B needs `amod`, which P still
    /// provides after A is switched off, so B stays on — while C, which needs
    /// what only Q provided, is carried along, and goes off before Q.
    #[test]
    fn a_mod_another_enabled_provider_still_covers_is_not_carried_along() {
        let s = row(modz("s", vec![prov("smod", "1.0")], vec![]), true);
        let a = row(
            modz(
                "a",
                vec![prov("amod", "1.0")],
                vec![dep("smod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let q = row(
            modz(
                "q",
                vec![prov("qmod", "1.0")],
                vec![dep("smod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let p = row(modz("p", vec![prov("amod", "1.0")], vec![]), true);
        let b = row(
            modz("b", vec![], vec![dep("amod", "", RangeFamily::Maven)]),
            true,
        );
        let c = row(
            modz("c", vec![], vec![dep("qmod", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![s, a, q, p, b, c]);
        assert_eq!(
            inst.removal_impact(&set(&["s"])).unwrap().dependents,
            vec![
                impacted("a", &["S"]),
                impacted("c", &["Q"]),
                impacted("q", &["S"]),
            ]
        );
    }

    /// Three waves — Y and A break directly, Z and X once those are off, W last
    /// — with the registry listing later waves first. A mod goes off before any
    /// listed mod it needs (Z before A, W before X before Y), and otherwise the
    /// earliest by wave, then registry order, goes first: the interleaving of the
    /// two chains is today's order, not the registry's. The answer never depends
    /// on a hash set's iteration order, which is seeded anew for every set.
    #[test]
    fn dependents_come_in_one_safe_order_whatever_the_hash_seed() {
        let w = row(
            modz("w", vec![], vec![dep("xmod", "", RangeFamily::Maven)]),
            true,
        );
        let z = row(
            modz("z", vec![], vec![dep("amod", "", RangeFamily::Maven)]),
            true,
        );
        let s = row(modz("s", vec![prov("smod", "1.0")], vec![]), true);
        let y = row(
            modz(
                "y",
                vec![prov("ymod", "1.0")],
                vec![dep("smod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let a = row(
            modz(
                "a",
                vec![prov("amod", "1.0")],
                vec![dep("smod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let x = row(
            modz(
                "x",
                vec![prov("xmod", "1.0")],
                vec![dep("ymod", "", RangeFamily::Maven)],
            ),
            true,
        );
        let inst = instance(vec![w, z, s, y, a, x]);
        let expected = vec![
            impacted("z", &["A"]),
            impacted("a", &["S"]),
            impacted("w", &["X"]),
            impacted("x", &["Y"]),
            impacted("y", &["S"]),
        ];
        for _ in 0..16 {
            assert_eq!(
                inst.removal_impact(&set(&["s"])).unwrap().dependents,
                expected
            );
        }
    }

    // ── orphan offers (A14) ───────────────────────────────────────────────

    fn shas(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// `l` and `n` were pulled in for `t`, which is leaving. `m` stays and needs
    /// `core`, which only `l` provides — whatever the registry's `requires`
    /// edges say, the jars say `l` is still needed.
    #[test]
    fn an_orphan_a_mod_that_stays_still_needs_is_not_offered() {
        let t = row(
            modz(
                "t",
                vec![prov("tmod", "1.0")],
                vec![dep("core", "", RangeFamily::Maven)],
            ),
            true,
        );
        let l = row(modz("l", vec![prov("core", "1.0")], vec![]), true);
        let n = row(modz("n", vec![prov("other", "1.0")], vec![]), true);
        let m = row(
            modz("m", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![t, l, n, m]);
        assert_eq!(
            inst.removable_with(&set(&["t"]), &shas(&["l", "n"])),
            shas(&["n"])
        );
    }

    /// Each alone is harmless, both together break `m`: the offer is judged as
    /// a whole, so accepting all of it can never break what one-by-one would not.
    #[test]
    fn two_libraries_that_cover_for_each_other_are_not_both_offered() {
        let t = row(modz("t", vec![prov("tmod", "1.0")], vec![]), true);
        let l1 = row(modz("l1", vec![prov("core", "1.0")], vec![]), true);
        let l2 = row(modz("l2", vec![prov("core", "1.0")], vec![]), true);
        let m = row(
            modz("m", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![t, l1, l2, m]);
        assert_eq!(
            inst.removable_with(&set(&["t"]), &shas(&["l1", "l2"])),
            shas(&["l1"])
        );
    }

    /// Removing `t` already breaks `m` (the removal dialog says so). Only what a
    /// CANDIDATE adds holds it back, so `n`, needed by nobody, is still offered.
    #[test]
    fn what_the_removal_itself_breaks_does_not_hold_an_orphan_back() {
        let t = row(modz("t", vec![prov("core", "1.0")], vec![]), true);
        let n = row(modz("n", vec![prov("other", "1.0")], vec![]), true);
        let m = row(
            modz("m", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![t, n, m]);
        assert_eq!(
            inst.removable_with(&set(&["t"]), &shas(&["n"])),
            shas(&["n"])
        );
    }

    /// `m` is broken today and its fix is «enable `l`»: a switched-off library
    /// an enabled mod needs is still needed. Removed, `l` could no longer be
    /// named as the provider to switch on — the counterfactual drops the row
    /// outright, it does not merely leave it disabled.
    #[test]
    fn a_disabled_library_an_enabled_mod_needs_is_not_offered() {
        let t = row(modz("t", vec![prov("tmod", "1.0")], vec![]), true);
        let l = row(modz("l", vec![prov("core", "1.0")], vec![]), false);
        let m = row(
            modz("m", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            true,
        );
        let inst = instance(vec![t, l, m]);
        assert!(inst.removable_with(&set(&["t"]), &shas(&["l"])).is_empty());
    }

    /// An ENABLED jar the scan could not read may provide anything, and a row
    /// the parse does not list is gone or changed since the registry was read:
    /// neither is known to be unneeded. A DISABLED unreadable jar satisfies
    /// nothing today, so its leaving is known to break nothing.
    #[test]
    fn an_orphan_that_cannot_be_judged_is_not_offered() {
        let t = row(modz("t", vec![prov("tmod", "1.0")], vec![]), true);
        let mut inst = instance(vec![t]);
        let on = installed_jar("on.jar", "on", "On");
        let mut off = installed_jar("off.jar", "off", "Off");
        off.enabled = false;
        inst.unreadable = vec![on, off];
        assert_eq!(
            inst.removable_with(&set(&["t"]), &shas(&["on", "off", "ghost"])),
            shas(&["off"])
        );
    }

    /// Transitive to a fixed point: t → a → b; b → t closes a cycle. `zmod` is
    /// absent (an install, not an enable); `q` is `e`'s problem, not `t`'s. B is
    /// listed first: A needs it, so it comes on before A.
    #[test]
    fn enabling_asks_for_disabled_requirements_to_a_fixed_point() {
        let t = row(
            modz(
                "t",
                vec![prov("tmod", "1.0")],
                vec![
                    dep("amod", "", RangeFamily::Maven),
                    dep("zmod", "", RangeFamily::Maven),
                ],
            ),
            false,
        );
        let a = row(
            modz(
                "a",
                vec![prov("amod", "1.0")],
                vec![dep("bmod", "", RangeFamily::Maven)],
            ),
            false,
        );
        let b = row(
            modz(
                "b",
                vec![prov("bmod", "1.0")],
                vec![dep("tmod", "", RangeFamily::Maven)],
            ),
            false,
        );
        let e = row(
            modz("e", vec![], vec![dep("qmod", "", RangeFamily::Maven)]),
            true,
        );
        let q = row(modz("q", vec![prov("qmod", "1.0")], vec![]), false);
        let inst = instance(vec![t, a, b, e, q]);
        let got = inst.enable_impact(&set(&["t"])).unwrap().requirements;
        assert_eq!(
            got,
            vec![
                DisabledRequirement {
                    sha1: "b".into(),
                    name: "B".into()
                },
                DisabledRequirement {
                    sha1: "a".into(),
                    name: "A".into()
                },
            ]
        );
    }

    /// Several targets are switched on together: a provider they share is
    /// listed once, and a provider that is itself among the targets is not a
    /// requirement at all.
    #[test]
    fn enabling_several_reports_a_shared_requirement_once_and_never_a_target() {
        let t1 = row(
            modz("t1", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            false,
        );
        let t2 = row(
            modz("t2", vec![], vec![dep("core", "", RangeFamily::Maven)]),
            false,
        );
        let p = row(modz("p", vec![prov("core", "1.0")], vec![]), false);
        let inst = instance(vec![t1, t2, p]);
        let req = |t: &[&str]| inst.enable_impact(&set(t)).unwrap().requirements;
        assert_eq!(
            req(&["t1", "t2"]),
            vec![DisabledRequirement {
                sha1: "p".into(),
                name: "P".into()
            }],
            "one provider, needed by two targets, is listed once"
        );
        assert!(
            req(&["t1", "t2", "p"]).is_empty(),
            "a provider being enabled with them is not a requirement"
        );
    }

    #[test]
    fn enable_impact_will_not_guess_for_a_jar_it_could_not_read() {
        let mut inst = instance(vec![]);
        let mut t = installed_jar("t.jar", "t", "T");
        t.enabled = false;
        inst.unreadable = vec![t];
        assert!(inst.enable_impact(&set(&["t"])).is_err());
    }

    /// A target the registry no longer lists (removed since the UI read it) is
    /// "could not tell", not "needs nothing" — the same `ModsNotFound` the
    /// enable itself would answer with.
    #[test]
    fn enable_impact_refuses_a_mod_the_registry_does_not_know() {
        let inst = instance(vec![row(modz("a", vec![], vec![]), false)]);
        assert!(matches!(
            inst.enable_impact(&set(&["a", "ghost"])),
            Err(crate::error::Error::ModsNotFound { .. })
        ));
    }

    // ── flip order: the lists are switched one by one, as given ──────────

    /// A row that provides `<sha1>mod` and requires `<n>mod` for each `n` of
    /// `needs` — `mods.toml` declarations, which the NeoForge fixture enforces.
    fn mod_row(sha1: &str, needs: &[&str], enabled: bool) -> ParsedRow {
        let deps = needs
            .iter()
            .map(|n| dep(&format!("{n}mod"), "", RangeFamily::Maven))
            .collect();
        row(
            modz(sha1, vec![prov(&format!("{sha1}mod"), "1.0")], deps),
            enabled,
        )
    }

    /// The dependents `targets` leaving takes along, in the order they are listed.
    fn off_order(inst: &ParsedInstance, targets: &[&str]) -> Vec<String> {
        let impact = inst.removal_impact(&set(targets)).unwrap();
        impact.dependents.into_iter().map(|d| d.sha1).collect()
    }

    /// The requirements enabling `targets` switches on, in the order they are listed.
    fn on_order(inst: &ParsedInstance, targets: &[&str]) -> Vec<String> {
        let impact = inst.enable_impact(&set(targets)).unwrap();
        impact.requirements.into_iter().map(|r| r.sha1).collect()
    }

    /// S is a library, L needs S, M needs S and L: both break in one wave, and
    /// whichever the registry lists first, M goes off before L — L off while M
    /// is still on would leave M without L.
    #[test]
    fn a_mod_goes_off_before_a_same_wave_mod_it_needs() {
        let s = || mod_row("s", &[], true);
        let l = || mod_row("l", &["s"], true);
        let m = || mod_row("m", &["s", "l"], true);
        for inst in [instance(vec![s(), l(), m()]), instance(vec![s(), m(), l()])] {
            assert_eq!(off_order(&inst, &["s"]), shas(&["m", "l"]));
        }
    }

    /// A needs S and B, B needs C, C needs S: A and C break at once, B once C is
    /// off. Wave order (A, C, B) would switch C off while B needs it; reversed,
    /// B off while A needs it. A, B, C keeps every step whole.
    #[test]
    fn a_mod_goes_off_before_a_later_wave_mod_it_needs() {
        let inst = instance(vec![
            mod_row("s", &[], true),
            mod_row("a", &["s", "b"], true),
            mod_row("b", &["c"], true),
            mod_row("c", &["s"], true),
        ]);
        assert_eq!(off_order(&inst, &["s"]), shas(&["a", "b", "c"]));
    }

    /// The same-wave mirror: T needs L and S, L needs S — both found in one
    /// round, in T's declaration order. Whichever T declares first, S comes on
    /// before L: L on without S would not load.
    #[test]
    fn a_requirement_comes_on_before_a_same_round_requirement_that_needs_it() {
        for t_needs in [["l", "s"], ["s", "l"]] {
            let inst = instance(vec![
                mod_row("t", &t_needs, false),
                mod_row("l", &["s"], false),
                mod_row("s", &[], false),
            ]);
            assert_eq!(on_order(&inst, &["t"]), shas(&["s", "l"]), "{t_needs:?}");
        }
    }

    /// The later-wave mirror: T needs S and B, B needs C, C needs S — found as S,
    /// B, then C. Found order switches B on before C; reversed, C before S.
    /// S, C, B keeps every step whole.
    #[test]
    fn a_requirement_comes_on_before_an_earlier_found_requirement_that_needs_it() {
        let inst = instance(vec![
            mod_row("t", &["s", "b"], false),
            mod_row("b", &["c"], false),
            mod_row("c", &["s"], false),
            mod_row("s", &[], false),
        ]);
        assert_eq!(on_order(&inst, &["t"]), shas(&["s", "c", "b"]));
    }

    /// P and Q need each other. No order keeps a cycle whole, so it keeps
    /// today's order — broken at its earliest mod, whichever the registry lists
    /// first. X needs Q too, and still goes off before Q.
    #[test]
    fn a_cycle_keeps_todays_order_broken_at_its_earliest_mod() {
        let s = || mod_row("s", &[], true);
        let p = || mod_row("p", &["s", "q"], true);
        let q = || mod_row("q", &["s", "p"], true);
        let x = || mod_row("x", &["s", "q"], true);
        assert_eq!(
            off_order(&instance(vec![s(), p(), q()]), &["s"]),
            shas(&["p", "q"])
        );
        assert_eq!(
            off_order(&instance(vec![s(), q(), p()]), &["s"]),
            shas(&["q", "p"])
        );
        assert_eq!(
            off_order(&instance(vec![s(), p(), q(), x()]), &["s"]),
            shas(&["p", "x", "q"])
        );
    }

    /// Requirements that need nothing of each other keep the order they were
    /// found in. Where Y needs X, found after it, Y waits for X while the
    /// earliest mod free to go goes first: P, Q, X, Y — P and Q keep their order.
    #[test]
    fn mods_that_need_nothing_of_each_other_keep_todays_order() {
        let inst = instance(vec![
            mod_row("t", &["r", "p", "q"], false),
            mod_row("p", &[], false),
            mod_row("q", &[], false),
            mod_row("r", &[], false),
        ]);
        assert_eq!(on_order(&inst, &["t"]), shas(&["r", "p", "q"]));
        let inst = instance(vec![
            mod_row("t", &["p", "y", "q"], false),
            mod_row("p", &[], false),
            mod_row("y", &["x"], false),
            mod_row("q", &[], false),
            mod_row("x", &[], false),
        ]);
        assert_eq!(on_order(&inst, &["t"]), shas(&["p", "q", "x", "y"]));
    }

    // ── one order over the targets too ────────────────────────────────────

    /// Fails unless `order` names each of `want` exactly once.
    fn assert_names_each_once<'a>(order: &[String], want: impl Iterator<Item = &'a str>) {
        let mut got: Vec<&str> = order.iter().map(String::as_str).collect();
        let mut want: Vec<&str> = want.collect();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(
            got, want,
            "the order names every target and listed mod once"
        );
    }

    /// The order a removal switches `targets` and their dependents off in.
    fn removal_order(inst: &ParsedInstance, targets: &[&str]) -> Vec<String> {
        let impact = inst.removal_impact(&set(targets)).unwrap();
        let listed = impact.dependents.iter().map(|d| d.sha1.as_str());
        assert_names_each_once(&impact.order, listed.chain(targets.iter().copied()));
        impact.order
    }

    /// The order an enable switches `targets` and their requirements on in.
    fn enable_order(inst: &ParsedInstance, targets: &[&str]) -> Vec<String> {
        let impact = inst.enable_impact(&set(targets)).unwrap();
        let listed = impact.requirements.iter().map(|r| r.sha1.as_str());
        assert_names_each_once(&impact.order, listed.chain(targets.iter().copied()));
        impact.order
    }

    /// Sodium, Indium and Fabric API switched off together, where Indium needs
    /// the other two and nothing else needs any of them: Indium goes off first
    /// — off after either, one step would leave it on without what it needs.
    /// Switched on together, it comes on last.
    #[test]
    fn targets_that_need_each_other_are_switched_in_a_safe_order() {
        let rows = |on| {
            vec![
                mod_row("s", &[], on),
                mod_row("i", &["s", "f"], on),
                mod_row("f", &[], on),
            ]
        };
        let targets = ["s", "i", "f"];
        assert_eq!(
            removal_order(&instance(rows(true)), &targets),
            shas(&["i", "s", "f"])
        );
        assert_eq!(
            enable_order(&instance(rows(false)), &targets),
            shas(&["s", "f", "i"])
        );
    }

    /// U and T leave together; D needs U, and T needs D. D is carried along —
    /// it loses U — but T, still on, needs D: T goes off before D, and D before
    /// U. The dependents, then the targets (D, U, T), would switch D off while
    /// T still needs it.
    #[test]
    fn a_target_goes_off_before_a_dependent_it_needs() {
        let inst = instance(vec![
            mod_row("u", &[], true),
            mod_row("t", &["d"], true),
            mod_row("d", &["u"], true),
        ]);
        assert_eq!(off_order(&inst, &["u", "t"]), shas(&["d"]));
        assert_eq!(removal_order(&inst, &["u", "t"]), shas(&["t", "d", "u"]));
    }

    /// T and U come on together; T needs R, which is off, and R needs U. R is
    /// switched on with them — after U, which it needs, and before T, which
    /// needs it. The requirements, then the targets (R, T, U), would switch R
    /// on without U.
    #[test]
    fn a_target_comes_on_before_a_requirement_that_needs_it() {
        let inst = instance(vec![
            mod_row("t", &["r"], false),
            mod_row("r", &["u"], false),
            mod_row("u", &[], false),
        ]);
        assert_eq!(on_order(&inst, &["t", "u"]), shas(&["r"]));
        assert_eq!(enable_order(&inst, &["t", "u"]), shas(&["u", "r", "t"]));
    }

    /// The caller switches exactly what `order` names, so it names every
    /// target — a disabled one whose jar could not be read too, after the
    /// readable ones.
    #[test]
    fn the_order_names_a_target_whose_jar_could_not_be_read() {
        let mut inst = instance(vec![mod_row("a", &[], true)]);
        let mut off = installed_jar("off.jar", "off", "Off");
        off.enabled = false;
        inst.unreadable = vec![off];
        assert_eq!(removal_order(&inst, &["off", "a"]), shas(&["a", "off"]));
    }
}
