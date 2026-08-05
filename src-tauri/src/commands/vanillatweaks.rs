//! Vanilla Tweaks IPC: read the pack catalogue, and install a built
//! selection into an instance's library or an own-server's world.
//!
//! There is no update command here. VT rows are ordinary registry / sidecar
//! rows with a `ModSource::VanillaTweaks` provenance, so both existing
//! datapack update surfaces already see them — see
//! `datapacks::vanillatweaks::platform`.

use crate::datapacks::vanillatweaks::{self, family_for, VtCatalogue};
use crate::datapacks::DatapackProvenance;
use crate::error::{Error, Result};
use crate::mods::platform::ModSource;

/// One selected pack's outcome. Reported per pack so a single bad pack never
/// costs the user the rest of their selection.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct VtInstallOutcome {
    pub filename: String,
    pub installed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct VtInstallReport {
    pub outcomes: Vec<VtInstallOutcome>,
}

/// Recover a pack's identity from the bundle filename plus the selection that
/// produced it, and turn it into provenance.
///
/// The filename is `<pack name> v<version>.zip` and the category is not in it,
/// so the category comes from what the user ticked. `project_id` keeps the
/// category because the build request is keyed by it — a name alone cannot
/// rebuild the request when an update comes.
///
/// A name we did not select yields `None` rather than a guessed category: an
/// unrecognised pack installs without provenance, which is honest, instead of
/// with a wrong one, which would break its updates.
fn provenance_for(
    filename: &str,
    selection: &[(String, Vec<String>)],
) -> Option<DatapackProvenance> {
    let stem = filename.strip_suffix(".zip").unwrap_or(filename);
    // rsplit: a pack may legitimately contain " v" in its own name.
    let (name, version) = stem.rsplit_once(" v")?;
    let category = selection.iter().find_map(|(cat, names)| {
        names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
            .then(|| cat.clone())
    })?;
    Some(DatapackProvenance {
        source: ModSource::VanillaTweaks,
        project_id: format!("{category}/{name}"),
        version_id: version.to_string(),
        version_number: Some(version.to_string()),
    })
}

/// The pack catalogue for `mc_version`'s family. A version with no family —
/// pre-1.13, or a Minecraft release VT has not caught up with — is refused
/// with a typed error rather than answered with an empty list.
#[tauri::command]
#[specta::specta]
pub async fn vt_catalogue(mc_version: String) -> Result<VtCatalogue> {
    let family = family_for(&mc_version).ok_or_else(|| Error::VanillaTweaksUnavailable {
        mc_version: mc_version.clone(),
    })?;
    vanillatweaks::VtClient::new().catalogue(&family).await
}

/// Build `selection` and install every pack into the instance's datapack
/// library. Placing them into worlds is the caller's next step — the frontend
/// opens the same world picker a catalogue install opens.
#[tauri::command]
#[specta::specta]
pub async fn vt_install_to_instance(
    app: tauri::AppHandle,
    instance_id: String,
    selection: Vec<(String, Vec<String>)>,
) -> Result<VtInstallReport> {
    let root = crate::datapacks::instance_root(&app, &instance_id)?;
    let (mc_version, _loader) = super::read_active_mc_and_loader(&app, &instance_id)?;
    let family = family_for(&mc_version).ok_or_else(|| Error::VanillaTweaksUnavailable {
        mc_version: mc_version.clone(),
    })?;
    let packs = crate::network::throttle::with_interactive(vanillatweaks::build_selection(
        &family, &selection,
    ))
    .await?;

    let mut outcomes = Vec::with_capacity(packs.len());
    for (filename, bytes) in packs {
        let provenance = provenance_for(&filename, &selection);
        let res = crate::datapacks::library::install_named_at(
            &root,
            &filename,
            &bytes,
            provenance.as_ref(),
        )
        .await;
        outcomes.push(match res {
            Ok(_) => VtInstallOutcome {
                filename,
                installed: true,
                error: None,
            },
            Err(e) => VtInstallOutcome {
                filename,
                installed: false,
                error: Some(e.to_string()),
            },
        });
    }
    Ok(VtInstallReport { outcomes })
}

/// Build `selection` and install every pack into the server's world. Refused
/// while the server runs or is starting, through the same guard every other
/// server datapack mutation opens with — taken BEFORE the build request, so
/// the network round trip is inside the guarded window rather than racing it.
#[tauri::command]
#[specta::specta]
pub async fn vt_install_to_server(
    app: tauri::AppHandle,
    id: String,
    selection: Vec<(String, Vec<String>)>,
) -> Result<VtInstallReport> {
    crate::servers_runtime::datapacks::guard::gate(&id)?;
    let base = crate::paths::app_dir(&app).map_err(|e| Error::io("<app_dir>", e))?;
    let p = crate::paths::server_paths(&base, &id);
    let file = crate::servers_runtime::store::read_server_json(&p.json)?;
    let world =
        crate::servers_runtime::datapacks::world_dir(&p.runtime, &super::server_props_raw(&p));
    let family = family_for(&file.mc_version).ok_or_else(|| Error::VanillaTweaksUnavailable {
        mc_version: file.mc_version.clone(),
    })?;

    let packs = crate::network::throttle::with_interactive(vanillatweaks::build_selection(
        &family, &selection,
    ))
    .await?;

    let mut outcomes = Vec::with_capacity(packs.len());
    for (filename, bytes) in packs {
        let provenance = provenance_for(&filename, &selection);
        let res = crate::servers_runtime::datapacks::mutate::install_bytes(
            &world,
            &filename,
            &bytes,
            provenance.as_ref(),
        )
        .await;
        outcomes.push(match res {
            Ok(_) => VtInstallOutcome {
                filename,
                installed: true,
                error: None,
            },
            Err(e) => VtInstallOutcome {
                filename,
                installed: false,
                error: Some(e.to_string()),
            },
        });
    }
    Ok(VtInstallReport { outcomes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_recovers_the_category_and_version_from_the_bundle_name() {
        let sel = vec![("survival".to_string(), vec!["graves".to_string()])];
        let p = provenance_for("graves v2.8.5.zip", &sel).unwrap();
        assert_eq!(p.project_id, "survival/graves");
        assert_eq!(p.version_id, "2.8.5");
        assert_eq!(p.version_number.as_deref(), Some("2.8.5"));
        assert_eq!(p.source, ModSource::VanillaTweaks);
    }

    #[test]
    fn a_pack_name_containing_v_is_split_at_the_last_one() {
        let sel = vec![("survival".to_string(), vec!["v is for victory".to_string()])];
        let p = provenance_for("v is for victory v1.2.zip", &sel).unwrap();
        assert_eq!(p.project_id, "survival/v is for victory");
        assert_eq!(p.version_id, "1.2");
    }

    #[test]
    fn a_name_we_did_not_select_yields_no_provenance_rather_than_a_wrong_one() {
        let sel = vec![("survival".to_string(), vec!["graves".to_string()])];
        assert!(provenance_for("something else v1.0.zip", &sel).is_none());
    }

    #[test]
    fn a_filename_without_a_version_marker_yields_no_provenance() {
        let sel = vec![("survival".to_string(), vec!["graves".to_string()])];
        assert!(provenance_for("graves.zip", &sel).is_none());
    }
}
