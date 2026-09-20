//! Pure, offline orphan detection for bulk mod uninstall.
//!
//! A mod is an orphan candidate if, after the `removing` SHA-1 set is gone,
//! its `project_id` appears in NO remaining mod's `requires`, it is not itself
//! being removed, and it was pulled in as some removed mod's dependency.
//! Manual mods (no `project_id`) are never flagged. No network, no I/O.
//!
//! `requires_edges` is the one place a row's `requires` list is computed — by
//! install and by update alike.

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

/// The `requires` edge list to store on a primary's registry row: the edges
/// the OUTGOING row already carried, plus the projects this operation pulled
/// in that were not installed before it ran. Sorted, deduplicated.
///
/// One rule for both writers, because they had drifted: a fresh install
/// recorded its pulled-in closure, while `mods_update_one` recorded nothing —
/// and since an update removes the old row and writes a new one, every
/// «Update» silently emptied the list. `find_orphans` then no longer knew why
/// a library was there and never offered it for removal again.
///
/// - `registry` is the snapshot taken BEFORE the operation touched anything.
/// - `outgoing_sha1` is the row being replaced; `None` for a fresh install.
///   An update must not forget why a library is there, so its edges carry
///   over — including transitive ones the update itself never resolves.
/// - `pulled_in` are the dependencies the operation installs. One that was
///   already in the registry (same source + project) is NOT claimed: the user
///   had it first, and claiming it would later offer it as this mod's orphan.
///   The install path's closure is already pruned this way, so for it the
///   filter changes nothing; the update path resolves its deps unpruned.
pub(crate) fn requires_edges<'a>(
    registry: &[InstalledMod],
    outgoing_sha1: Option<&str>,
    pulled_in: impl IntoIterator<Item = &'a ModVersion>,
) -> Vec<String> {
    let carried = outgoing_sha1
        .and_then(|sha| registry.iter().find(|m| m.sha1.eq_ignore_ascii_case(sha)))
        .map(|m| m.requires.clone())
        .unwrap_or_default();
    let already_installed = |v: &ModVersion| {
        registry.iter().any(|m| {
            m.source == Some(v.source) && m.project_id.as_deref() == Some(v.project_id.as_str())
        })
    };
    let mut ids: Vec<String> = carried
        .into_iter()
        .chain(
            pulled_in
                .into_iter()
                // `filter` hands out `&&ModVersion`; deref once, explicitly.
                .filter(|v| !already_installed(*v))
                .map(|v| v.project_id.clone()),
        )
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
