//! Pure, offline orphan detection for bulk mod uninstall.
//!
//! A mod is an orphan candidate if, after the `removing` SHA-1 set is gone,
//! its `project_id` appears in NO remaining mod's `requires`, it is not itself
//! being removed, and it was pulled in as some removed mod's dependency.
//! Manual mods (no `project_id`) are never flagged. No network, no I/O.

use crate::mods::platform::{InstalledMod, ModVersion, OrphanRef};
use std::collections::HashSet;

pub(crate) fn find_orphans(mods: &[InstalledMod], removing: &[String]) -> Vec<OrphanRef> {
    let removing: HashSet<&str> = removing.iter().map(|s| s.as_str()).collect();

    // Project IDs still required by any mod that survives the removal.
    let still_required: HashSet<&str> = mods
        .iter()
        .filter(|m| !removing.contains(m.sha1.as_str()))
        .flat_map(|m| m.requires.iter().map(|s| s.as_str()))
        .collect();

    mods.iter()
        .filter(|m| !removing.contains(m.sha1.as_str()))
        .filter_map(|m| {
            let pid = m.project_id.as_deref()?;
            if still_required.contains(pid) {
                return None;
            }
            // Only flag mods that were pulled in as someone's dependency at
            // some point — i.e. some removed mod listed this project.
            let was_required_by_removed = mods
                .iter()
                .any(|x| removing.contains(x.sha1.as_str()) && x.requires.iter().any(|r| r == pid));
            if !was_required_by_removed {
                return None;
            }
            Some(OrphanRef {
                sha1: m.sha1.clone(),
                name: m.name.clone(),
                project_id: pid.to_string(),
            })
        })
        .collect()
}

/// The `requires` edge list to store on a primary's registry row.
/// D5 of the 2026-09-20 spec — see the implementation commit.
pub(crate) fn requires_edges<'a>(
    _registry: &[InstalledMod],
    _outgoing_sha1: Option<&str>,
    pulled_in: impl IntoIterator<Item = &'a ModVersion>,
) -> Vec<String> {
    // Scaffold — red round: today's install rule. Nothing carried over from an
    // outgoing row, nothing pruned against the registry.
    let mut ids: Vec<String> = pulled_in
        .into_iter()
        .map(|v| v.project_id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::find_orphans;
    use crate::mods::platform::{InstalledMod, ModSource};

    fn m(sha1: &str, project_id: &str, requires: &[&str]) -> InstalledMod {
        InstalledMod {
            filename: format!("{sha1}.jar"),
            sha1: sha1.into(),
            source: Some(ModSource::Modrinth),
            project_id: Some(project_id.into()),
            version_id: Some("v".into()),
            name: sha1.to_uppercase(),
            version_number: Some("1.0".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: requires.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn shared_dep_not_orphaned_while_one_dependent_remains() {
        let mods = vec![m("a", "A", &["D"]), m("b", "B", &["D"]), m("d", "D", &[])];
        let orphans = find_orphans(&mods, &["a".into()]);
        assert!(orphans.is_empty());
    }

    #[test]
    fn dep_becomes_orphan_when_all_dependents_removed() {
        let mods = vec![m("a", "A", &["D"]), m("b", "B", &["D"]), m("d", "D", &[])];
        let orphans = find_orphans(&mods, &["a".into(), "b".into()]);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].project_id, "D");
    }

    #[test]
    fn a_mod_in_removing_is_never_returned() {
        let mods = vec![m("a", "A", &["D"]), m("d", "D", &[])];
        let orphans = find_orphans(&mods, &["a".into(), "d".into()]);
        assert!(orphans.is_empty());
    }

    #[test]
    fn never_required_mod_is_not_flagged() {
        let mods = vec![m("a", "A", &["D"]), m("c", "C", &[]), m("d", "D", &[])];
        let orphans = find_orphans(&mods, &["a".into()]);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].project_id, "D");
    }

    // ── requires_edges (2026-09-20 spec, D5) ─────────────────────────────────
    use super::requires_edges;
    use crate::mods::platform::{LoaderKind, ModFile, ModVersion};

    fn pulled(project_id: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: project_id.into(),
            version_id: format!("{project_id}-v1"),
            name: project_id.to_uppercase(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.21.1".into()],
            loaders: vec![LoaderKind::NeoForge],
            primary_file: ModFile {
                filename: format!("{project_id}.jar"),
                url: format!("https://cdn.modrinth.com/{project_id}.jar"),
                sha1: Some("00".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: Vec::new(),
            published_at: None,
        }
    }

    #[test]
    fn a_fresh_install_records_what_it_pulled_in_sorted_and_deduplicated() {
        // (pin) Today's install rule, verbatim — the install command must get
        // the byte-identical list out of the shared helper.
        let pulled_in = [pulled("zeta"), pulled("alpha"), pulled("zeta")];
        assert_eq!(
            requires_edges(&[], None, pulled_in.iter()),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn an_update_keeps_the_edges_the_outgoing_row_carried() {
        // Every «Update» removed the old row and wrote a new one with NO edges,
        // so the libraries it had pulled in stopped being anyone's dependency
        // and were never offered for removal again. SHA-1 match ignores case.
        let registry = vec![
            m("old", "P", &["lib-b", "lib-a"]),
            m("a", "lib-a", &[]),
            m("b", "lib-b", &[]),
        ];
        assert_eq!(
            requires_edges(&registry, Some("OLD"), std::iter::empty::<&ModVersion>()),
            vec!["lib-a".to_string(), "lib-b".to_string()]
        );
    }

    #[test]
    fn a_dependency_that_was_already_installed_is_not_claimed() {
        // The install path gets this from its pruned closure; an update resolves
        // the target's deps UNPRUNED, so the helper has to say it: a library the
        // user already had must not become this mod's orphan later.
        let registry = vec![m("old", "P", &[]), m("l", "lib-present", &[])];
        let pulled_in = [pulled("lib-present"), pulled("lib-new")];
        assert_eq!(
            requires_edges(&registry, Some("old"), pulled_in.iter()),
            vec!["lib-new".to_string()]
        );
    }

    #[test]
    fn an_unknown_outgoing_sha_carries_nothing() {
        // (pin)
        let registry = vec![m("other", "Q", &["lib-q"])];
        assert!(requires_edges(
            &registry,
            Some("missing"),
            std::iter::empty::<&ModVersion>()
        )
        .is_empty());
    }
}
