//! Pure, offline orphan detection for bulk mod uninstall.
//!
//! A mod is an orphan candidate if, after the `removing` SHA-1 set is gone,
//! its `project_id` appears in NO remaining mod's `requires`, it is not itself
//! being removed, and it was pulled in as some removed mod's dependency.
//! Manual mods (no `project_id`) are never flagged. No network, no I/O.

use crate::mods::platform::{InstalledMod, OrphanRef};
use std::collections::HashSet;

pub fn find_orphans(mods: &[InstalledMod], removing: &[String]) -> Vec<OrphanRef> {
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
}
