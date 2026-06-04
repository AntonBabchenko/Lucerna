//! Pure mod classification.

use crate::mods::modpack::export::types::ExportMode;
use crate::mods::modpack::schema::ModpackFormat;
use crate::mods::platform::{InstalledMod, ModSource};

/// Split enabled mods into (referenced, unresolvable) for a given format +
/// mode. Pure. In `Full` mode every mod is unresolvable-by-policy (bundled).
/// In `Lightweight` mode a mod is referenced when its source matches the
/// format and it carries both platform ids:
///   - mrpack: any `Modrinth` OR `Curseforge` mod with ids (both CDN hosts
///     are on the mrpack allowed-download list).
///   - CurseForge zip: only `Curseforge` mods with ids.
///
/// Disabled mods must be filtered out by the caller before this runs.
pub fn classify(
    format: ModpackFormat,
    mode: ExportMode,
    mods: &[InstalledMod],
) -> (Vec<&InstalledMod>, Vec<&InstalledMod>) {
    if matches!(mode, ExportMode::Full) {
        return (vec![], mods.iter().collect());
    }
    let mut referenced = Vec::new();
    let mut unresolvable = Vec::new();
    for m in mods {
        let has_ids = m.project_id.is_some() && m.version_id.is_some();
        // FTB: pack-managed source — not an export target, so FTB mods always
        // land in unresolvable (bundled). The `Ftb` variant never appears in
        // an export format request; including it in the match would be wrong.
        let ok = has_ids
            && matches!(
                (format, m.source),
                (ModpackFormat::Modrinth, Some(ModSource::Modrinth))
                    | (ModpackFormat::Modrinth, Some(ModSource::Curseforge))
                    | (ModpackFormat::Curseforge, Some(ModSource::Curseforge))
            );
        if ok {
            referenced.push(m);
        } else {
            unresolvable.push(m);
        }
    }
    (referenced, unresolvable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn mk(sha: &str, source: Option<ModSource>, ids: bool) -> InstalledMod {
        InstalledMod {
            filename: format!("{sha}.jar"),
            sha1: sha.into(),
            source,
            project_id: ids.then(|| "p".to_string()),
            version_id: ids.then(|| "v".to_string()),
            name: sha.into(),
            version_number: None,
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    #[test]
    fn full_mode_bundles_everything() {
        let mods = vec![mk("a", Some(ModSource::Modrinth), true)];
        let (refd, unres) = classify(ModpackFormat::Modrinth, ExportMode::Full, &mods);
        assert!(refd.is_empty());
        assert_eq!(unres.len(), 1);
    }

    #[test]
    fn mrpack_references_modrinth_and_curseforge_with_ids() {
        let mods = vec![
            mk("a", Some(ModSource::Modrinth), true),
            mk("b", Some(ModSource::Curseforge), true),
        ];
        let (refd, unres) = classify(ModpackFormat::Modrinth, ExportMode::Lightweight, &mods);
        assert_eq!(refd.len(), 2);
        assert!(unres.is_empty());
    }

    #[test]
    fn cf_zip_references_only_curseforge() {
        let mods = vec![
            mk("a", Some(ModSource::Modrinth), true),
            mk("b", Some(ModSource::Curseforge), true),
        ];
        let (refd, unres) = classify(ModpackFormat::Curseforge, ExportMode::Lightweight, &mods);
        assert_eq!(refd.len(), 1);
        assert_eq!(refd[0].sha1, "b");
        assert_eq!(unres.len(), 1);
        assert_eq!(unres[0].sha1, "a");
    }

    #[test]
    fn local_mod_without_ids_is_unresolvable() {
        let mods = vec![mk("a", None, false)];
        let (refd, unres) = classify(ModpackFormat::Modrinth, ExportMode::Lightweight, &mods);
        assert!(refd.is_empty());
        assert_eq!(unres.len(), 1);
    }

    #[test]
    fn source_present_but_missing_ids_is_unresolvable() {
        let mods = vec![mk("a", Some(ModSource::Modrinth), false)];
        let (refd, unres) = classify(ModpackFormat::Modrinth, ExportMode::Lightweight, &mods);
        assert!(refd.is_empty());
        assert_eq!(unres.len(), 1);
    }
}
