//! Pure dependency pre-flight resolver. No network, no disk. Given each
//! installed mod's parsed manifest and an index of available providers,
//! returns the required-dependency violations the loader would hit.
//!
//! The module also exposes `dependency_preflight_for_root` — the testable
//! core of the `instance_dependency_preflight` Tauri command — along with
//! the `ViolationKind`, `DepViolation`, and `PreflightReport` IPC types.

use std::collections::HashMap;

use crate::mods::local::{DepSide, ManifestDeps};
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
    pub fn build(mods: &[ParsedMod], jij: &[(String, Option<String>)]) -> Self {
        let mut by_id = HashMap::new();
        for m in mods {
            for p in &m.manifest.provided {
                by_id
                    .entry(canon_id(&p.mod_id))
                    .or_insert(p.version.clone());
            }
        }
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
}

/// The launcher launches a client; a SERVER-only dep is not enforced.
/// True when a dependency declared with `family` is enforced on an instance
/// running `loader` — i.e. the loader actually reads that descriptor at runtime.
/// Forge never reads `fabric.mod.json`, so a multi-loader jar's fabric deps must
/// not be flagged on a Forge instance (and vice-versa).
fn dep_applies_to_loader(
    family: crate::mods::version_range::RangeFamily,
    loader: crate::instances::schema::LoaderKind,
) -> bool {
    use crate::instances::schema::LoaderKind as L;
    use crate::mods::version_range::RangeFamily as F;
    match loader {
        L::Forge | L::NeoForge => family == F::Maven,
        // Fabric reads only fabric.mod.json — a Quilt mod on a Fabric instance is
        // a loader-compat issue (handled elsewhere), not a missing-dependency one.
        L::Fabric => family == F::FabricPredicate,
        // Quilt runs Fabric mods too, so it reads both descriptors.
        L::Quilt => matches!(family, F::QuiltPredicate | F::FabricPredicate),
        // Vanilla loads no mods → no declared mod dependency applies.
        L::Vanilla => false,
    }
}

pub fn resolve(
    mods: &[ParsedMod],
    index: &ProviderIndex,
    loader: crate::instances::schema::LoaderKind,
) -> Vec<Violation> {
    let mut out = Vec::new();
    for m in mods {
        for dep in &m.manifest.deps {
            if !dep.required || dep.side == DepSide::Server {
                continue;
            }
            // Only enforce deps from the descriptor the instance's loader reads;
            // a Forge instance never loads fabric.mod.json, so its fabric/quilt
            // deps are not real (fixes cross-loader false positives).
            if !dep_applies_to_loader(dep.family, loader) {
                continue;
            }
            if !index.is_provided(&dep.dep_id) {
                out.push(Violation::MissingRequired {
                    dependent_sha1: m.sha1.clone(),
                    dependent_name: m.name.clone(),
                    dep_id: dep.dep_id.clone(),
                });
                continue;
            }
            // present — range-check only if a concrete version is known via direct lookup
            if let Some(Some(v)) = index.get(&dep.dep_id) {
                if satisfies(v, &dep.range, dep.family) == Satisfaction::Violated {
                    out.push(Violation::VersionOutOfRange {
                        dependent_sha1: m.sha1.clone(),
                        dependent_name: m.name.clone(),
                        dep_id: dep.dep_id.clone(),
                        needed: dep.range.clone(),
                        installed: v.clone(),
                        family: dep.family,
                    });
                }
            }
            // version None or satisfied-via-alias-only => provider present but version unknown => silent
        }
    }
    out
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
    /// Optional human-readable display name for `dep_id`, if we could look
    /// it up. `None` in v1 (best-effort enrichment is out of scope).
    pub dep_display_name: Option<String>,
    /// `MissingRequired` or `VersionOutOfRange`.
    pub kind: ViolationKind,
    /// The version that is actually installed (`None` for `MissingRequired`).
    pub installed_version: Option<String>,
    /// The version range the dependent declared (empty string for
    /// `MissingRequired`).
    pub needed: String,
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
        ModSource::Ftb | ModSource::Atlauncher | ModSource::Hangar => None,
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
            dep_display_name: None,
            kind: ViolationKind::MissingRequired,
            installed_version: None,
            needed: String::new(),
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
        } => {
            // Normalize '-'/'_' + lowercase on BOTH sides so a `fabric-api`
            // dep routes to a `fabric_api` provider (the two are used
            // interchangeably by the ecosystem). Matches `canon_id`, which the
            // provider index already uses.
            let key = canon_id(&dep_id);
            let provider_project = provider_owner.get(&key).cloned();
            let provider_sha1 = provider_sha1_map.get(&key).cloned();
            DepViolation {
                dependent_sha1,
                dependent_name,
                dep_id,
                dep_display_name: None,
                kind: ViolationKind::VersionOutOfRange,
                installed_version: Some(installed),
                needed,
                provider_project,
                provider_sha1,
                family: Some(family),
            }
        }
    }
}

/// The testable core of the `instance_dependency_preflight` Tauri command.
/// Accepts a resolved `instance_root` path so integration tests can call it
/// without a `tauri::AppHandle`.
pub async fn dependency_preflight_for_root(
    root: &std::path::Path,
    loader: crate::instances::schema::LoaderKind,
) -> crate::error::Result<PreflightReport> {
    use crate::mods::local::{read_jar_embedded_providers, read_jar_manifest_deps};
    use std::collections::HashMap;

    let installed = crate::mods::installed::list(root).await?;
    let mods_dir = crate::mods::installed::mods_dir(root);

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
        // Attempt to read the jar bytes; try the .disabled name as fallback.
        let bytes = {
            let path = mods_dir.join(&m.filename);
            match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(_) => {
                    let disabled = mods_dir.join(format!("{}.disabled", m.filename));
                    match tokio::fs::read(&disabled).await {
                        Ok(b) => b,
                        Err(_) => continue, // jar missing from disk — skip gracefully
                    }
                }
            }
        };

        let Ok(manifest) = read_jar_manifest_deps(&bytes) else {
            continue; // unreadable zip — skip, never fail the whole scan
        };

        // Collect JIJ (Jar-in-Jar) providers so an embedded lib is not
        // falsely flagged as a missing dependency.
        for p in read_jar_embedded_providers(&bytes) {
            jij.push((p.mod_id, p.version));
        }

        // Register provided mod-ids → platform identity and SHA-1 for enrichment.
        // provider_sha1 is populated regardless of source so that even FTB/ATL
        // mods (which yield no DepProjectRef) can still route updates correctly.
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

    let index = ProviderIndex::build(&parsed, &jij);
    let raw = resolve(&parsed, &index, loader);
    let violations = raw
        .into_iter()
        .map(|v| enrich(v, &provider_owner, &provider_sha1))
        .collect();
    Ok(PreflightReport { violations })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;
    use crate::mods::local::{DeclaredDep, ManifestDeps, ProvidedMod};
    use crate::mods::version_range::RangeFamily;

    #[test]
    fn dep_family_matches_instance_loader() {
        use LoaderKind as L;
        use RangeFamily as F;
        // Forge/NeoForge read only mods.toml (Maven) deps.
        assert!(dep_applies_to_loader(F::Maven, L::Forge));
        assert!(dep_applies_to_loader(F::Maven, L::NeoForge));
        assert!(!dep_applies_to_loader(F::FabricPredicate, L::Forge));
        assert!(!dep_applies_to_loader(F::QuiltPredicate, L::Forge));
        // Fabric reads only fabric.mod.json deps.
        assert!(dep_applies_to_loader(F::FabricPredicate, L::Fabric));
        assert!(!dep_applies_to_loader(F::Maven, L::Fabric));
        assert!(!dep_applies_to_loader(F::QuiltPredicate, L::Fabric));
        // Quilt runs Fabric mods → accepts both.
        assert!(dep_applies_to_loader(F::QuiltPredicate, L::Quilt));
        assert!(dep_applies_to_loader(F::FabricPredicate, L::Quilt));
        assert!(!dep_applies_to_loader(F::Maven, L::Quilt));
        // Vanilla loads no mods → nothing applies.
        assert!(!dep_applies_to_loader(F::Maven, L::Vanilla));
        assert!(!dep_applies_to_loader(F::FabricPredicate, L::Vanilla));
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
        let index = ProviderIndex::build(&mods, &[]);
        // Forge instance: only the Maven dep is a real violation.
        let forge = resolve(&mods, &index, LoaderKind::Forge);
        assert_eq!(forge.len(), 1);
        assert!(
            matches!(&forge[0], Violation::MissingRequired { dep_id, .. } if dep_id == "create")
        );
        // Fabric instance: only the fabric dep is real.
        let fabric = resolve(&mods, &index, LoaderKind::Fabric);
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
        let index = ProviderIndex::build(&mods, &[]);
        let quilt = resolve(&mods, &index, LoaderKind::Quilt);
        assert_eq!(quilt.len(), 1);
        assert!(
            matches!(&quilt[0], Violation::MissingRequired { dep_id, .. } if dep_id == "fabric-api")
        );
    }

    fn dep(id: &str, range: &str, family: RangeFamily) -> DeclaredDep {
        DeclaredDep {
            dep_id: id.into(),
            range: range.into(),
            required: true,
            side: DepSide::Both,
            family,
        }
    }
    fn modz(sha: &str, provided: Vec<ProvidedMod>, deps: Vec<DeclaredDep>) -> ParsedMod {
        ParsedMod {
            sha1: sha.into(),
            name: sha.to_uppercase(),
            manifest: ManifestDeps { provided, deps },
        }
    }
    fn prov(id: &str, ver: &str) -> ProvidedMod {
        ProvidedMod {
            mod_id: id.into(),
            version: Some(ver.into()),
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
        let index = ProviderIndex::build(&mods, &[]);
        let v = resolve(&mods, &index, LoaderKind::Forge);
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
        let index = ProviderIndex::build(&mods, &[]);
        assert!(matches!(
            resolve(&mods, &index, LoaderKind::Forge)[0],
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
        let index = ProviderIndex::build(&mods, &[("sophisticatedcore".into(), None)]);
        assert!(resolve(&mods, &index, LoaderKind::Forge).is_empty()); // present via JIJ, version unknown => silent
    }

    #[test]
    fn server_side_dep_not_enforced_on_client() {
        let mut d = dep("servercore", "[2,)", RangeFamily::Maven);
        d.side = DepSide::Server;
        let mods = vec![modz("a", vec![prov("x", "1")], vec![d])];
        let index = ProviderIndex::build(&mods, &[]);
        assert!(resolve(&mods, &index, LoaderKind::Forge).is_empty());
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
        let index = ProviderIndex::build(&mods, &[]);
        assert!(resolve(&mods, &index, LoaderKind::Forge).is_empty());
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
                dep_display_name: None,
                kind: ViolationKind::VersionOutOfRange,
                installed_version: Some("1.3.50".into()),
                needed: "[1.3.51,)".into(),
                provider_project: Some(DepProjectRef::Modrinth {
                    project_id: "sc".into(),
                    version_id: None,
                }),
                provider_sha1: Some("abc123".into()),
                family: Some(crate::mods::version_range::RangeFamily::Maven),
            }],
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

        let report = dependency_preflight_for_root(td.path(), LoaderKind::Fabric)
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
        let index = ProviderIndex::build(&mods, &[]);
        assert!(
            resolve(&mods, &index, LoaderKind::Fabric).is_empty(),
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
        let index = ProviderIndex::build(&mods, &[]);
        assert!(
            resolve(&mods, &index, LoaderKind::Fabric).is_empty(),
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

        let report = dependency_preflight_for_root(td.path(), LoaderKind::Fabric)
            .await
            .unwrap();
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert_eq!(report.violations[0].dep_id, "fabric-renderer-api-v1");
        assert!(matches!(
            report.violations[0].kind,
            ViolationKind::MissingRequired
        ));
    }
}
