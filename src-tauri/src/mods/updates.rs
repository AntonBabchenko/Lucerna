//! Standalone mod-update check: classify each installed user-mod
//! against the versions a platform currently lists for the instance's
//! Minecraft version + loader. Pure logic — no I/O — so the command
//! layer in `commands.rs` stays a thin orchestrator.

use serde::Serialize;
use specta::Type;

use crate::mods::installed::PackOrigin;
use crate::mods::platform::{AssetUpdateState, InstalledMod, LoaderKind, ModSource, ModVersion};

/// One installed user-mod's update-check result. One per *eligible*
/// mod — see [`eligible_identity`]; ineligible mods are absent.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModUpdateCheck {
    /// SHA-1 of the currently installed jar — identifies the row and is
    /// the handle `mods_update_one` uses to remove the old file.
    pub sha1: String,
    /// Display name from the registry.
    pub name: String,
    pub source: ModSource,
    pub project_id: String,
    pub current_version_id: String,
    pub current_version_number: Option<String>,
    pub state: ModUpdateState,
}

/// The per-mod classification.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModUpdateState {
    /// The installed version is the newest for this MC + loader.
    UpToDate,
    /// A newer version exists; `target` is the version to install.
    UpdateAvailable { target: ModVersion },
    /// Cannot determine — the installed version is not in the
    /// platform's current list, or the list is empty.
    Unknown,
    /// The platform query failed (network, missing CurseForge key,
    /// project delisted / 404). Set by `mods_check_updates` on a failed
    /// query — never produced by `classify_update`.
    CheckFailed { reason: String },
}

/// The pack name + the SHA-1s of its bundled mods. Feeds the Installed
/// tab's "from modpack" chip. `mod_shas` are lowercased.
#[derive(Debug, Clone, Serialize, Type)]
pub struct PackOriginSummary {
    pub project_name: String,
    pub mod_shas: Vec<String>,
}

/// Classify `installed` against the platform's version list (which the
/// caller fetched, filtered to the instance's MC + loader, newest
/// first). Pure. Never returns `CheckFailed`.
pub fn classify_update(installed: &InstalledMod, versions: &[ModVersion]) -> ModUpdateState {
    let Some(current) = installed.version_id.as_deref() else {
        return ModUpdateState::Unknown;
    };
    let Some(newest) = versions.first() else {
        return ModUpdateState::Unknown;
    };
    if newest.version_id == current {
        return ModUpdateState::UpToDate;
    }
    // Only confident `newest` is an upgrade (not a downgrade) when the
    // installed version is itself somewhere in the list.
    if versions.iter().any(|v| v.version_id == current) {
        ModUpdateState::UpdateAvailable {
            target: newest.clone(),
        }
    } else {
        ModUpdateState::Unknown
    }
}

/// §5.4 of the 2026-09-21 spec, rows U1–U6: the update state the batch answers
/// settle for one Modrinth mod, or `None` = ask the listing. `own` =
/// `version_files[registry sha1]`, `latest` = `update_many[registry sha1]`.
/// Keyed by the REGISTRY digest: the update check is a statement about the
/// registered file, as [`classify_update`] has always been. Pure.
pub fn batch_update_state(
    current_version_id: &str,
    project_id: &str,
    mc: &str,
    loader: LoaderKind,
    own: Option<&ModVersion>,
    latest: &[ModVersion],
) -> Option<ModUpdateState> {
    let _ = (current_version_id, project_id, mc, loader, own, latest);
    None // stub: red round
}

/// `true` iff `installed` is one of the modpack's bundled mods — its
/// SHA-1 matches a `mods/` entry in `pack_origin.files`. `false` when
/// the instance has no `pack_origin`.
pub fn is_pack_origin_mod(installed: &InstalledMod, pack_origin: Option<&PackOrigin>) -> bool {
    let Some(po) = pack_origin else {
        return false;
    };
    po.files.iter().any(|f| {
        f.install_path.starts_with("mods/") && f.sha1.eq_ignore_ascii_case(&installed.sha1)
    })
}

/// If `installed` is eligible for an update check, return its platform
/// identity `(source, project_id, version_id)`. `None` when the mod
/// lacks platform identity (a hand-dropped jar) or is a modpack-origin
/// mod. Replaces the spec's `is_eligible(...) -> bool`: returning the
/// identity lets the command layer use it without an `unwrap()`.
pub fn eligible_identity(
    installed: &InstalledMod,
    pack_origin: Option<&PackOrigin>,
) -> Option<(ModSource, String, String)> {
    if is_pack_origin_mod(installed, pack_origin) {
        return None;
    }
    match (
        installed.source,
        &installed.project_id,
        &installed.version_id,
    ) {
        (Some(source), Some(project_id), Some(version_id)) => {
            Some((source, project_id.clone(), version_id.clone()))
        }
        _ => None,
    }
}

/// The platform identity a REPLACEMENT search needs: a project to ask about.
///
/// Deliberately weaker than [`eligible_identity`], which additionally demands
/// the installed `version_id` because it was written for update
/// *classification* — telling an upgrade from a downgrade genuinely needs to
/// know where you are. "What should this become" does not. Reusing the
/// stricter predicate would strand mods whose project IS known and whose
/// target build IS queryable, and it would do so exactly in the wrong-platform
/// case, because `enrich` drops `version_id` when the matched version's tags
/// do not fit the instance.
///
/// Pack-origin mods return `None` — the pack owns its versions, and changing
/// them piecemeal is the modpack version-switch flow, not this one.
pub fn replaceable_identity(
    installed: &InstalledMod,
    pack_origin: Option<&PackOrigin>,
) -> Option<(ModSource, String)> {
    if is_pack_origin_mod(installed, pack_origin) {
        return None;
    }
    match (installed.source, &installed.project_id) {
        (Some(source), Some(project_id)) => Some((source, project_id.clone())),
        _ => None,
    }
}

/// Classify an installed resource-pack or shader against the versions the
/// platform currently lists (caller fetches them, newest-first for the
/// instance's MC version). Pure. Never returns `CheckFailed`.
///
/// The newest fetched version is compared to `installed_version_id`.
/// Returns `UpToDate` when the fetched list is empty (nothing newer offered),
/// when `installed_version_id` is `None` (no baseline to compare against),
/// or when the installed version already matches the newest. Returns
/// `UpdateAvailable` only when a differing installed version is known.
pub fn classify_asset_update(
    installed_version_id: Option<&str>,
    fetched: &[ModVersion],
) -> AssetUpdateState {
    let Some(latest) = fetched.first() else {
        return AssetUpdateState::UpToDate;
    };
    match installed_version_id {
        // Same version, or we don't know what's installed → nothing to do.
        None => AssetUpdateState::UpToDate,
        Some(vid) if vid == latest.version_id => AssetUpdateState::UpToDate,
        Some(_) => AssetUpdateState::UpdateAvailable {
            latest: Box::new(latest.clone()),
        },
    }
}

/// Build the chip data for an instance's modpack origin: the pack name
/// and the lowercased SHA-1s of its bundled `mods/` files.
pub fn pack_origin_summary(pack_origin: &PackOrigin) -> PackOriginSummary {
    PackOriginSummary {
        project_name: pack_origin.project_name.clone(),
        mod_shas: pack_origin
            .files
            .iter()
            .filter(|f| f.install_path.starts_with("mods/"))
            .map(|f| f.sha1.to_ascii_lowercase())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::installed::{PackOrigin, PackOriginFile};
    use crate::mods::modpack::schema::EnvSupport;
    use crate::mods::platform::{LoaderKind, ModFile};

    fn installed_mod(
        sha1: &str,
        source: Option<ModSource>,
        project_id: Option<&str>,
        version_id: Option<&str>,
    ) -> InstalledMod {
        InstalledMod {
            filename: "mod.jar".into(),
            sha1: sha1.into(),
            source,
            project_id: project_id.map(String::from),
            version_id: version_id.map(String::from),
            name: "Mod".into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-05-22T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    fn version(version_id: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "p".into(),
            version_id: version_id.into(),
            name: "Mod".into(),
            version_number: version_id.into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Fabric],
            primary_file: ModFile {
                filename: format!("mod-{version_id}.jar"),
                url: "https://example/mod.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }

    fn pack_origin(files: &[(&str, &str)]) -> PackOrigin {
        PackOrigin {
            project_id: Some("pack".into()),
            source: ModSource::Modrinth,
            project_name: "Cool Pack".into(),
            version: "1.0".into(),
            files: files
                .iter()
                .map(|(sha, path)| PackOriginFile {
                    sha1: (*sha).into(),
                    name: "F".into(),
                    filename: "f.jar".into(),
                    install_path: (*path).into(),
                    url: "https://example/f.jar".into(),
                    size: 1.0,
                    project_id: "fp".into(),
                    version_id: "fv".into(),
                    env_client: EnvSupport::Required,
                    source: ModSource::Modrinth,
                })
                .collect(),
            missing_mods: vec![],
            skipped_overrides: vec![],
            resolved_missing: Vec::new(),
            inert_loader_jars: vec![],
        }
    }

    #[test]
    fn classify_up_to_date_when_installed_is_newest() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), Some("v3"));
        let versions = vec![version("v3"), version("v2"), version("v1")];
        assert!(matches!(
            classify_update(&m, &versions),
            ModUpdateState::UpToDate
        ));
    }

    #[test]
    fn classify_update_available_when_newer_exists() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let versions = vec![version("v3"), version("v2"), version("v1")];
        match classify_update(&m, &versions) {
            ModUpdateState::UpdateAvailable { target } => assert_eq!(target.version_id, "v3"),
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }
    }

    #[test]
    fn classify_unknown_when_installed_version_not_listed() {
        let m = installed_mod(
            "s1",
            Some(ModSource::Modrinth),
            Some("p"),
            Some("v-delisted"),
        );
        let versions = vec![version("v3"), version("v2")];
        assert!(matches!(
            classify_update(&m, &versions),
            ModUpdateState::Unknown
        ));
    }

    #[test]
    fn classify_unknown_when_version_list_empty() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        assert!(matches!(classify_update(&m, &[]), ModUpdateState::Unknown));
    }

    #[test]
    fn classify_unknown_when_installed_has_no_version_id() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("p"), None);
        assert!(matches!(
            classify_update(&m, &[version("v1")]),
            ModUpdateState::Unknown
        ));
    }

    #[test]
    fn pack_origin_mod_detected_by_sha() {
        let m = installed_mod("aaa", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let po = pack_origin(&[("aaa", "mods/x.jar")]);
        assert!(is_pack_origin_mod(&m, Some(&po)));
    }

    #[test]
    fn user_mod_on_modpack_instance_is_not_pack_origin() {
        let m = installed_mod("bbb", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let po = pack_origin(&[("aaa", "mods/x.jar")]);
        assert!(!is_pack_origin_mod(&m, Some(&po)));
    }

    #[test]
    fn no_pack_origin_means_not_pack_origin() {
        let m = installed_mod("aaa", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        assert!(!is_pack_origin_mod(&m, None));
    }

    #[test]
    fn manual_jar_has_no_eligible_identity() {
        let m = installed_mod("s1", None, None, None);
        assert!(eligible_identity(&m, None).is_none());
    }

    #[test]
    fn pack_origin_mod_has_no_eligible_identity() {
        let m = installed_mod("aaa", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let po = pack_origin(&[("aaa", "mods/x.jar")]);
        assert!(eligible_identity(&m, Some(&po)).is_none());
    }

    #[test]
    fn user_browser_mod_is_eligible() {
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("proj"), Some("ver"));
        assert_eq!(
            eligible_identity(&m, None),
            Some((ModSource::Modrinth, "proj".to_string(), "ver".to_string()))
        );
    }

    // -- replaceable_identity --------------------------------------------

    #[test]
    fn replaceable_identity_accepts_a_known_project_with_no_installed_version() {
        // This is the case `eligible_identity` gets wrong: `enrich` drops
        // `version_id` when the matched version's tags don't fit the
        // instance, but the project itself is known and queryable.
        let m = installed_mod("s1", Some(ModSource::Modrinth), Some("proj"), None);
        assert_eq!(
            replaceable_identity(&m, None),
            Some((ModSource::Modrinth, "proj".to_string()))
        );
    }

    #[test]
    fn replaceable_identity_is_none_for_a_pack_origin_mod() {
        let m = installed_mod("aaa", Some(ModSource::Modrinth), Some("p"), Some("v1"));
        let po = pack_origin(&[("aaa", "mods/x.jar")]);
        assert!(replaceable_identity(&m, Some(&po)).is_none());
    }

    #[test]
    fn replaceable_identity_is_none_for_a_hand_dropped_jar() {
        let m = installed_mod("s1", None, None, None);
        assert!(replaceable_identity(&m, None).is_none());
    }

    #[test]
    fn classify_asset_update_flags_newer_version_id() {
        use crate::mods::platform::AssetUpdateState;

        let latest = version("v2");
        let state = classify_asset_update(Some("v1"), &[latest]);
        assert!(matches!(state, AssetUpdateState::UpdateAvailable { .. }));

        let same = version("v1");
        assert_eq!(
            classify_asset_update(Some("v1"), &[same]),
            AssetUpdateState::UpToDate
        );

        // empty fetched list → UpToDate (nothing newer offered)
        assert_eq!(
            classify_asset_update(Some("v1"), &[]),
            AssetUpdateState::UpToDate
        );

        // Unknown installed version → don't nag.
        assert_eq!(
            classify_asset_update(None, &[version("v2")]),
            AssetUpdateState::UpToDate
        );
    }

    #[test]
    fn index_pairing_restores_installed_order_after_unordered_poll() {
        // `mods_check_updates` polls platforms with `buffer_unordered`, which
        // yields completions out of order. It pairs each result with its
        // original installed-list index and re-sorts by that index. This
        // proves that order-restoration mechanism: an arbitrarily shuffled
        // `(index, value)` set sorts back to ascending-index (installed) order.
        let mut shuffled: Vec<(usize, &str)> =
            vec![(3, "d"), (0, "a"), (4, "e"), (1, "b"), (2, "c")];
        shuffled.sort_by_key(|(i, _)| *i);
        let restored: Vec<&str> = shuffled.into_iter().map(|(_, v)| v).collect();
        assert_eq!(restored, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn pack_origin_summary_lists_only_mods_dir_shas_lowercased() {
        let po = pack_origin(&[("AAA", "mods/x.jar"), ("bbb", "resourcepacks/rp.zip")]);
        let s = pack_origin_summary(&po);
        assert_eq!(s.project_name, "Cool Pack");
        assert_eq!(s.mod_shas, vec!["aaa".to_string()]);
    }

    // ── Batch update state (2026-09-21 spec §5.4) ───────────────────────────
    use crate::mods::compat::batch_model::{build, Platform};

    const NF: LoaderKind = LoaderKind::NeoForge;

    #[test]
    fn batch_update_rows() {
        let v1 = build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["aa"]).version;
        let v2 = build("p", "v2", &["1.21.1"], &[NF], "p-2.jar", &["bb"]).version;
        let old = build("p", "v0", &["1.21"], &[NF], "p-0.jar", &["cc"]).version;
        // U1: unknown bytes, or bytes of another project → ask the listing.
        assert!(batch_update_state("v1", "p", "1.21.1", NF, None, &[v2.clone()]).is_none());
        assert!(batch_update_state("v1", "q", "1.21.1", NF, Some(&v1), &[v2.clone()]).is_none());
        // U2: nothing listed, the file itself not tagged → Unknown, as an empty listing says.
        assert!(matches!(
            batch_update_state("v0", "p", "1.21.1", NF, Some(&old), &[]),
            Some(ModUpdateState::Unknown)
        ));
        // U3: nothing listed although the file is tagged — contradiction → ask.
        assert!(batch_update_state("v1", "p", "1.21.1", NF, Some(&v1), &[]).is_none());
        // U3: the newest tagged build is one the filename rule drops → ask.
        let fo = LoaderKind::Forge;
        let forge_own = build("p", "f1", &["1.20.4"], &[fo], "p-forge-1.jar", &["dd"]).version;
        let dropped = build("p", "f2", &["1.20.4"], &[fo], "p-neoforge-2.jar", &["ee"]).version;
        assert!(
            batch_update_state("f1", "p", "1.20.4", fo, Some(&forge_own), &[dropped]).is_none()
        );
        // U4: the newest listed build is the registered one.
        assert!(matches!(
            batch_update_state("v2", "p", "1.21.1", NF, Some(&v2), &[v2.clone()]),
            Some(ModUpdateState::UpToDate)
        ));
        // U5: the bytes ARE the registered version: listed → update; not listed → Unknown.
        match batch_update_state("v1", "p", "1.21.1", NF, Some(&v1), &[v2.clone()]) {
            Some(ModUpdateState::UpdateAvailable { target }) => assert_eq!(target.version_id, "v2"),
            other => panic!("expected an update to v2, got {other:?}"),
        }
        assert!(matches!(
            batch_update_state("v0", "p", "1.21.1", NF, Some(&old), &[v2.clone()]),
            Some(ModUpdateState::Unknown)
        ));
        // U6: the bytes belong to another version than the registered one → ask.
        assert!(batch_update_state("v9", "p", "1.21.1", NF, Some(&v1), &[v2]).is_none());
    }

    fn kind(s: &ModUpdateState) -> &'static str {
        match s {
            ModUpdateState::UpToDate => "up_to_date",
            ModUpdateState::UpdateAvailable { .. } => "update_available",
            ModUpdateState::Unknown => "unknown",
            ModUpdateState::CheckFailed { .. } => "check_failed",
        }
    }

    #[test]
    fn a_batch_update_state_is_what_the_listing_would_say_or_it_declines() {
        let platform = Platform(vec![
            build("p", "v0", &["1.21"], &[NF], "p-0.jar", &["a0"]),
            build("p", "v1", &["1.21.1"], &[NF], "p-1.jar", &["a1"]),
            build("p", "v1b", &["1.21.1"], &[NF], "p-1b.jar", &["a1"]),
            build("p", "v2", &["1.21.1"], &[NF], "p-2.jar", &["a2", "a2x"]),
            build("q", "w1", &["1.21.1"], &[NF], "q-1.jar", &["b1"]),
        ]);
        let listing = platform.listing("p", "1.21.1", NF);
        let mut decided = 0;
        for (h, current) in [
            ("a0", "v0"),
            ("a1", "v1"),
            ("a1", "v1b"),
            ("a2", "v2"),
            ("a2x", "v2"),
            ("a1", "v9"),
            ("zz", "v1"),
        ] {
            let m = installed_mod(h, Some(ModSource::Modrinth), Some("p"), Some(current));
            let expected = classify_update(&m, &listing);
            let latest = platform.update_many(h, "1.21.1", NF);
            let owners = platform.owners(h);
            let picks: Vec<Option<&ModVersion>> = if owners.is_empty() {
                vec![None]
            } else {
                owners.into_iter().map(Some).collect()
            };
            for own in picks {
                if let Some(state) = batch_update_state(current, "p", "1.21.1", NF, own, &latest) {
                    decided += 1;
                    assert_eq!(kind(&state), kind(&expected), "{h} / {current}");
                    if let (
                        ModUpdateState::UpdateAvailable { target: a },
                        ModUpdateState::UpdateAvailable { target: b },
                    ) = (&state, &expected)
                    {
                        assert_eq!(a.version_id, b.version_id, "{h} / {current}: target");
                    }
                }
            }
        }
        assert!(
            decided >= 4,
            "only {decided} decided — the batch is not being exercised"
        );
    }
}
