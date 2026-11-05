//! Finishing a pack whose files Lucerna could not download (spec
//! `2026-11-05-blocked-pack-files-dropin-design.md`): which entries are still
//! pending — from the bytes on disk, never a file's name — checking a file the
//! user downloaded against what the pack's source published, and placing it
//! through `mods::store`.

pub mod digest;
pub mod evidence;
pub mod naming;
pub mod place;
pub mod scan;
pub mod state;
pub mod supply;
pub mod types;

use std::path::Path;

use crate::error::Error;
use crate::mods::installed::{self, PackOrigin};
use crate::mods::modpack::schema::{MissingModState, MissingModStatus};
use crate::mods::platform::InstalledMod;

/// Everything a manual-downloads decision reads, at one moment.
pub struct Loaded {
    pub origin: PackOrigin,
    pub installed: Vec<InstalledMod>,
    pub on_disk: std::collections::HashSet<String>,
    pub statuses: Vec<MissingModStatus>,
}

/// `Ok(None)` for an instance with no pack record.
pub async fn load(instance_root: &Path) -> Result<Option<Loaded>, Error> {
    let Some(origin) = installed::get_pack_origin(instance_root).await? else {
        return Ok(None);
    };
    let rows = installed::list(instance_root).await?;
    let on_disk = evidence::on_disk_shas(instance_root, &origin, &rows).await;
    let statuses = state::statuses(&origin, &rows, &on_disk);
    Ok(Some(Loaded {
        origin,
        installed: rows,
        on_disk,
        statuses,
    }))
}

/// Required, actionable entries with nothing of the mod on disk — what the Play
/// gate lists (spec D10). A read that fails is logged and answers empty: the
/// gate never blocks, and the Overview row still reads the status.
pub async fn pending_required(instance_root: &Path) -> Vec<types::ManualPending> {
    match load(instance_root).await {
        Ok(Some(l)) => l
            .statuses
            .iter()
            .filter(|s| {
                s.state == MissingModState::Missing
                    && s.entry.meta.required
                    && state::is_actionable(&s.entry)
            })
            .map(|s| types::ManualPending {
                key: state::entry_key(&s.entry),
                mod_name: s.entry.mod_name.clone(),
            })
            .collect(),
        Ok(None) => Vec::new(),
        Err(e) => {
            crate::diag!("[manual-downloads] pending files unknown: {e}");
            Vec::new()
        }
    }
}

/// The supplied files of this instance, for the update diff.
pub async fn supplied_old_for(instance_root: &Path) -> Vec<state::SuppliedOld> {
    let l = match load(instance_root).await {
        Ok(Some(l)) => l,
        Ok(None) => return Vec::new(),
        Err(e) => {
            crate::diag!("[manual-downloads] supplied files unknown: {e}");
            return Vec::new();
        }
    };
    let assets = match crate::mods::assets::list_all(instance_root).await {
        Ok(a) => Some(a),
        Err(e) => {
            crate::diag!("[manual-downloads] assets registry unreadable: {e}");
            None
        }
    };
    state::supplied_old(
        &l.origin,
        &l.statuses,
        &l.on_disk,
        &l.installed,
        assets.as_deref(),
    )
}
