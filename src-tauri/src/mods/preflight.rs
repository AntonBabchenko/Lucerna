//! Pure dependency pre-flight resolver. No network, no disk. Given each
//! installed mod's parsed manifest and an index of available providers,
//! returns the required-dependency violations the loader would hit.

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
}
