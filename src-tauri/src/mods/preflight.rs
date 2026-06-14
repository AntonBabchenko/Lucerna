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

/// What a provider id maps to: its declared version (if known).
#[derive(Debug, Clone, Default)]
pub struct ProviderIndex {
    by_id: HashMap<String, Option<String>>, // lowercased mod_id -> version
}

impl ProviderIndex {
    /// Build from all enabled mods' provided ids (own + JIJ).
    pub fn build(mods: &[ParsedMod], jij: &[(String, Option<String>)]) -> Self {
        let mut by_id = HashMap::new();
        for m in mods {
            for p in &m.manifest.provided {
                by_id
                    .entry(p.mod_id.to_ascii_lowercase())
                    .or_insert(p.version.clone());
            }
        }
        for (id, ver) in jij {
            by_id.entry(id.to_ascii_lowercase()).or_insert(ver.clone());
        }
        Self { by_id }
    }
    fn get(&self, id: &str) -> Option<&Option<String>> {
        self.by_id.get(&id.to_ascii_lowercase())
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
    },
}

/// The launcher launches a client; a SERVER-only dep is not enforced.
pub fn resolve(mods: &[ParsedMod], index: &ProviderIndex) -> Vec<Violation> {
    let mut out = Vec::new();
    for m in mods {
        for dep in &m.manifest.deps {
            if !dep.required || dep.side == DepSide::Server {
                continue;
            }
            match index.get(&dep.dep_id) {
                None => out.push(Violation::MissingRequired {
                    dependent_sha1: m.sha1.clone(),
                    dependent_name: m.name.clone(),
                    dep_id: dep.dep_id.clone(),
                }),
                Some(version) => {
                    if let Some(v) = version {
                        if satisfies(v, &dep.range, dep.family) == Satisfaction::Violated {
                            out.push(Violation::VersionOutOfRange {
                                dependent_sha1: m.sha1.clone(),
                                dependent_name: m.name.clone(),
                                dep_id: dep.dep_id.clone(),
                                needed: dep.range.clone(),
                                installed: v.clone(),
                            });
                        }
                    }
                    // version None => provider present but version unknown => silent
                }
            }
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
        ModSource::Ftb | ModSource::Atlauncher => None,
    }
}

/// Convert a raw `Violation` into a `DepViolation`, enriching the
/// `provider_project` field from the `provider_owner` map built earlier.
fn enrich(
    v: Violation,
    provider_owner: &std::collections::HashMap<String, crate::mods::platform::DepProjectRef>,
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
        },
        Violation::VersionOutOfRange {
            dependent_sha1,
            dependent_name,
            dep_id,
            needed,
            installed,
        } => {
            let provider_project = provider_owner.get(&dep_id.to_ascii_lowercase()).cloned();
            DepViolation {
                dependent_sha1,
                dependent_name,
                dep_id,
                dep_display_name: None,
                kind: ViolationKind::VersionOutOfRange,
                installed_version: Some(installed),
                needed,
                provider_project,
            }
        }
    }
}

/// The testable core of the `instance_dependency_preflight` Tauri command.
/// Accepts a resolved `instance_root` path so integration tests can call it
/// without a `tauri::AppHandle`.
pub async fn dependency_preflight_for_root(
    root: &std::path::Path,
) -> crate::error::Result<PreflightReport> {
    use crate::mods::local::{read_jar_embedded_providers, read_jar_manifest_deps};
    use std::collections::HashMap;

    let installed = crate::mods::installed::list(root).await?;
    let mods_dir = crate::mods::installed::mods_dir(root);

    // Map lowercased provided mod_id → DepProjectRef for violation enrichment
    // (powers the "view on platform" link). Built from mods with source identity.
    let mut provider_owner: HashMap<String, crate::mods::platform::DepProjectRef> = HashMap::new();

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

        // Register provided mod-ids → platform identity for violation enrichment.
        // Only insert when dep_project_ref returns Some; FTB/ATLauncher sources
        // return None (no per-mod browser) and must not create a spurious link.
        if let (Some(source), Some(project_id)) = (m.source, m.project_id.as_deref()) {
            if let Some(ref_) = dep_project_ref(source, project_id) {
                for p in &manifest.provided {
                    provider_owner
                        .entry(p.mod_id.to_ascii_lowercase())
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
    let raw = resolve(&parsed, &index);
    let violations = raw
        .into_iter()
        .map(|v| enrich(v, &provider_owner))
        .collect();
    Ok(PreflightReport { violations })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::local::{DeclaredDep, ManifestDeps, ProvidedMod};
    use crate::mods::version_range::RangeFamily;

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
        let v = resolve(&mods, &index);
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
            resolve(&mods, &index)[0],
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
        assert!(resolve(&mods, &index).is_empty()); // present via JIJ, version unknown => silent
    }

    #[test]
    fn server_side_dep_not_enforced_on_client() {
        let mut d = dep("servercore", "[2,)", RangeFamily::Maven);
        d.side = DepSide::Server;
        let mods = vec![modz("a", vec![prov("x", "1")], vec![d])];
        let index = ProviderIndex::build(&mods, &[]);
        assert!(resolve(&mods, &index).is_empty());
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
        assert!(resolve(&mods, &index).is_empty());
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
}
