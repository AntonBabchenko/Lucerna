//! Checking files the user handed over against the entries a pack waits for,
//! and placing the matches (spec §4.2). `plan` reads and hashes with no claim;
//! `apply` writes and must run under the instance's shared maintenance claim.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::mods::curseforge::CurseForgeClient;
use crate::mods::installed::{ManualSupplied, PackOrigin, SupplyVerified};
use crate::mods::manual_downloads::digest;
use crate::mods::manual_downloads::naming::{self, Unmatched};
use crate::mods::manual_downloads::place::{self, Ext, Occupancy};
use crate::mods::manual_downloads::state::{self, Expected};
use crate::mods::manual_downloads::types::SupplyRow;
use crate::mods::modpack::schema::{MissingModState, MissingModStatus, ModpackUnresolvable};
use crate::mods::platform::InstalledMod;
use crate::mods::store::{self, StageError};

pub struct Ctx<'a> {
    pub instance_root: &'a Path,
    pub origin: &'a PackOrigin,
    pub installed: &'a [InstalledMod],
    pub statuses: &'a [MissingModStatus],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Sha1,
    Md5,
    User,
}

#[derive(Debug, Clone)]
pub struct Placement {
    pub src: PathBuf,
    pub display: String,
    pub key: String,
    pub entry: ModpackUnresolvable,
    pub rel: String,
    pub sha1: String,
    pub by: By,
}

#[derive(Debug)]
pub enum Planned {
    Row(SupplyRow),
    Place(Placement),
}

#[derive(Debug, Clone)]
pub struct PlacedFile {
    pub sha1: String,
    pub filename: String,
    pub name: String,
    pub is_mod: bool,
}

#[derive(Debug, Default)]
pub struct Applied {
    pub rows: Vec<SupplyRow>,
    pub placed: Vec<PlacedFile>,
}

enum One {
    Planned(Planned),
    Unmatched(Unmatched),
}

/// The boundary checks every handed-over path goes through: a regular file
/// (never a link or a directory) with a `.jar` / `.zip` extension.
pub(crate) async fn probe(display: &str) -> Result<(PathBuf, Ext, u64), SupplyRow> {
    let src = PathBuf::from(display);
    let meta = tokio::fs::symlink_metadata(&src)
        .await
        .map_err(|e| SupplyRow::Unreadable {
            path: display.to_string(),
            reason: e.to_string(),
        })?;
    match place::ext_of(&src) {
        Some(ext) if meta.file_type().is_file() => Ok((src, ext, meta.len())),
        _ => Err(SupplyRow::WrongType {
            path: display.to_string(),
        }),
    }
}

fn hash_matches(st: &MissingModStatus, d: &digest::Digests) -> bool {
    match state::expected(&st.entry) {
        Expected::Sha1(x) => x == d.sha1,
        Expected::Md5(x) => d.md5.as_deref() == Some(x.as_str()),
        Expected::Nothing => false,
    }
}

async fn plan_one(ctx: &Ctx<'_>, display: &str) -> One {
    let (src, ext, len) = match probe(display).await {
        Ok(p) => p,
        Err(row) => return One::Planned(Planned::Row(row)),
    };
    let waiting = |s: &&MissingModStatus| {
        matches!(
            s.state,
            MissingModState::Missing | MissingModState::DifferentVersion
        ) && state::is_actionable(&s.entry)
            && state::expected(&s.entry) != Expected::Nothing
    };
    // A published size is a hint, not a gate: ATLauncher and FTB sizes are
    // author-written and may be wrong, so a file no size matches is still
    // hashed against every waiting entry, up to the fingerprint cap.
    let mut candidates: Vec<&MissingModStatus> = ctx
        .statuses
        .iter()
        .filter(waiting)
        .filter(|s| s.entry.size <= 0.0 || s.entry.size as u64 == len)
        .collect();
    if candidates.is_empty() && len <= naming::FINGERPRINT_CAP {
        candidates = ctx.statuses.iter().filter(waiting).collect();
    }
    if candidates.is_empty() {
        return One::Unmatched(Unmatched {
            path: display.to_string(),
            len,
        });
    }
    let want_md5 = candidates
        .iter()
        .any(|s| matches!(state::expected(&s.entry), Expected::Md5(_)));
    let d = match digest::digests(&src, want_md5).await {
        Ok(d) => d,
        Err(e) => {
            return One::Planned(Planned::Row(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.to_string(),
            }))
        }
    };
    let Some(st) = candidates.into_iter().find(|s| hash_matches(s, &d)) else {
        return One::Unmatched(Unmatched {
            path: display.to_string(),
            len,
        });
    };
    let key = state::entry_key(&st.entry);
    if st.state == MissingModState::DifferentVersion {
        let others = replace_candidates(ctx, &st.entry);
        if !others.is_empty() {
            return One::Planned(Planned::Row(SupplyRow::NeedsReplace {
                path: display.to_string(),
                key,
                mod_name: st.entry.mod_name.clone(),
                existing: names_of(&others),
            }));
        }
    }
    let by = if matches!(state::expected(&st.entry), Expected::Md5(_)) {
        By::Md5
    } else {
        By::Sha1
    };
    match place::destination(&st.entry, &src, ext, &st.entry.filename).await {
        Ok(Some(rel)) => One::Planned(Planned::Place(Placement {
            src,
            display: display.to_string(),
            key,
            entry: st.entry.clone(),
            rel,
            sha1: d.sha1,
            by,
        })),
        Ok(None) => One::Planned(Planned::Row(SupplyRow::CantPlace {
            path: display.to_string(),
        })),
        Err(e) => One::Planned(Planned::Row(SupplyRow::Unreadable {
            path: display.to_string(),
            reason: e.to_string(),
        })),
    }
}

/// Read, hash and decide — no writes. `cf: None` skips naming refusals (the
/// folder scan reports only what concerns the pack).
pub async fn plan(ctx: &Ctx<'_>, paths: &[String], cf: Option<&CurseForgeClient>) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut unmatched = Vec::new();
    for p in paths {
        match plan_one(ctx, p).await {
            One::Planned(planned) => out.push(planned),
            One::Unmatched(u) => unmatched.push(u),
        }
    }
    for row in naming::name_refusals(cf, &unmatched, ctx.statuses).await {
        out.push(Planned::Row(row));
    }
    out
}

/// Note an md5 match or a user choice on the pack record; `true` when there
/// was nothing to note or the note was written.
async fn record(instance_root: &Path, pl: &Placement) -> bool {
    let verified = match pl.by {
        By::Sha1 => return true,
        By::Md5 => SupplyVerified::Md5,
        By::User => SupplyVerified::User,
    };
    let note = ManualSupplied {
        key: pl.key.clone(),
        sha1: pl.sha1.clone(),
        verified,
    };
    match crate::mods::installed::update_pack_origin(instance_root, |o| {
        o.manual_supplied.retain(|s| s.key != note.key);
        o.manual_supplied.push(note.clone());
    })
    .await
    {
        Ok(true) => true,
        Ok(false) => {
            crate::diag!("[manual-downloads] no pack record to note {} on", pl.key);
            false
        }
        Err(e) => {
            crate::diag!("[manual-downloads] could not note {}: {e}", pl.key);
            false
        }
    }
}

/// Installed jars that look like another version of `e`'s mod, minus the
/// pack's own files: those are never something a Replace may remove.
pub(crate) fn replace_candidates<'a>(
    ctx: &'a Ctx<'_>,
    e: &ModpackUnresolvable,
) -> Vec<&'a InstalledMod> {
    let packs = pack_file_shas(ctx.origin);
    state::different_version_rows(e, ctx.installed)
        .into_iter()
        .filter(|r| !packs.contains(&r.sha1.to_ascii_lowercase()))
        .collect()
}

fn names_of(rows: &[&InstalledMod]) -> String {
    rows.iter()
        .map(|r| r.filename.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn pack_file_shas(origin: &PackOrigin) -> HashSet<String> {
    origin
        .files
        .iter()
        .map(|f| f.sha1.to_ascii_lowercase())
        .collect()
}

async fn bookkeeping(ctx: &Ctx<'_>, pl: &Placement) -> bool {
    let registered = match place::register(
        ctx.instance_root,
        &pl.rel,
        &pl.sha1,
        &pl.entry,
        pl.by != By::User,
    )
    .await
    {
        Ok(()) => true,
        Err(e) => {
            crate::diag!("[manual-downloads] registry add for {} failed: {e}", pl.rel);
            false
        }
    };
    let noted = record(ctx.instance_root, pl).await;
    registered && noted
}

pub(crate) async fn place_one(
    ctx: &Ctx<'_>,
    pl: Placement,
    placed: &mut Vec<PlacedFile>,
) -> SupplyRow {
    let mc = ctx.instance_root.join(".minecraft");
    let path = pl.display.clone();
    let key = pl.key.clone();
    let mod_name = pl.entry.mod_name.clone();
    match place::occupancy(
        &mc,
        &pl.rel,
        &pl.sha1,
        ctx.installed,
        &pack_file_shas(ctx.origin),
    )
    .await
    {
        Err(e) => SupplyRow::Unreadable {
            path,
            reason: e.to_string(),
        },
        Ok(Occupancy::PackFile { existing }) => SupplyRow::Conflict { path, existing },
        Ok(Occupancy::Taken { existing, .. }) => SupplyRow::NeedsReplace {
            path,
            key,
            mod_name,
            existing,
        },
        Ok(Occupancy::Same) => {
            let recorded = bookkeeping(ctx, &pl).await;
            SupplyRow::AlreadyThere {
                path,
                key,
                mod_name,
                recorded,
            }
        }
        Ok(Occupancy::Free) => {
            let dest = mc.join(&pl.rel);
            if let Some(parent) = dest.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return SupplyRow::Unreadable {
                        path,
                        reason: e.to_string(),
                    };
                }
            }
            match store::place_copy_verified(&pl.src, &dest, &pl.sha1).await {
                Err(StageError::Changed) => {
                    digest::forget(&pl.src);
                    SupplyRow::Changed { path }
                }
                Err(StageError::Io(e)) => SupplyRow::Unreadable {
                    path,
                    reason: e.details(),
                },
                Ok(()) => {
                    let recorded = bookkeeping(ctx, &pl).await;
                    placed.push(PlacedFile {
                        sha1: pl.sha1.clone(),
                        filename: pl.rel.rsplit('/').next().unwrap_or(&pl.rel).to_string(),
                        name: mod_name.clone(),
                        is_mod: pl.rel.starts_with("mods/"),
                    });
                    SupplyRow::Added {
                        path,
                        key,
                        mod_name,
                        recorded,
                    }
                }
            }
        }
    }
}

/// Write what `plan` decided. Caller holds the shared maintenance claim. The
/// place is checked again here: another writer may have taken it meanwhile.
pub async fn apply(ctx: &Ctx<'_>, planned: Vec<Planned>) -> Applied {
    let mut applied = Applied::default();
    for p in planned {
        let row = match p {
            Planned::Row(r) => r,
            Planned::Place(pl) => place_one(ctx, pl, &mut applied.placed).await,
        };
        applied.rows.push(row);
    }
    applied
}

/// Check plans made before the maintenance claim against what is true now
/// (`fresh` is loaded under the claim): the entry must still exist, still be
/// waiting, and still expect the same hash — else the placement becomes the row
/// that says why, and nothing is written for it.
pub fn revalidate(fresh: &Ctx<'_>, planned: Vec<Planned>) -> Vec<Planned> {
    planned
        .into_iter()
        .map(|p| {
            let Planned::Place(pl) = p else { return p };
            let path = pl.display.clone();
            match status_for(fresh, &pl.key) {
                None => Planned::Row(SupplyRow::NotInPack {
                    path,
                    found: None,
                    asked: false,
                }),
                Some(st)
                    if !matches!(
                        st.state,
                        MissingModState::Missing | MissingModState::DifferentVersion
                    ) =>
                {
                    Planned::Row(SupplyRow::AlreadyThere {
                        path,
                        key: pl.key.clone(),
                        mod_name: st.entry.mod_name.clone(),
                        recorded: true,
                    })
                }
                Some(st) if state::expected(&st.entry) != state::expected(&pl.entry) => {
                    Planned::Row(SupplyRow::NotInPack {
                        path,
                        found: None,
                        asked: false,
                    })
                }
                Some(_) => Planned::Place(pl),
            }
        })
        .collect()
}

/// The entry `key` names in this context.
pub(crate) fn status_for<'a>(ctx: &'a Ctx<'_>, key: &str) -> Option<&'a MissingModStatus> {
    ctx.statuses
        .iter()
        .find(|s| state::entry_key(&s.entry) == key)
}

/// A file the user chose for an entry with no published hash (spec D8): it
/// keeps its own name (the entry's may be synthetic), lands where the entry's
/// kind goes, and is noted as the user's choice. An entry that does have a
/// hash goes through the normal check instead.
pub async fn supply_unverified(ctx: &Ctx<'_>, key: &str, display: &str) -> Applied {
    let one = |row: SupplyRow| Applied {
        rows: vec![row],
        placed: vec![],
    };
    let Some(st) = status_for(ctx, key) else {
        return one(SupplyRow::NotInPack {
            path: display.to_string(),
            found: None,
            asked: false,
        });
    };
    if state::expected(&st.entry) != Expected::Nothing || !state::is_actionable(&st.entry) {
        let planned = plan(ctx, &[display.to_string()], None).await;
        return apply(ctx, planned).await;
    }
    // Classify like `plan_one`: another version asks first; a closed entry has
    // nothing to supply.
    match st.state {
        MissingModState::Missing => {}
        MissingModState::DifferentVersion => {
            let others = replace_candidates(ctx, &st.entry);
            if !others.is_empty() {
                return one(SupplyRow::NeedsReplace {
                    path: display.to_string(),
                    key: key.to_string(),
                    mod_name: st.entry.mod_name.clone(),
                    existing: names_of(&others),
                });
            }
        }
        _ => {
            return one(SupplyRow::AlreadyThere {
                path: display.to_string(),
                key: key.to_string(),
                mod_name: st.entry.mod_name.clone(),
                recorded: true,
            })
        }
    }
    let (src, ext, _) = match probe(display).await {
        Ok(p) => p,
        Err(row) => return one(row),
    };
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let d = match digest::digests(&src, false).await {
        Ok(d) => d,
        Err(e) => {
            return one(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.to_string(),
            })
        }
    };
    let rel = match place::destination(&st.entry, &src, ext, &name).await {
        Ok(Some(rel)) => rel,
        Ok(None) => {
            return one(SupplyRow::CantPlace {
                path: display.to_string(),
            })
        }
        Err(e) => {
            return one(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.to_string(),
            })
        }
    };
    let pl = Placement {
        src,
        display: display.to_string(),
        key: key.to_string(),
        entry: st.entry.clone(),
        rel,
        sha1: d.sha1,
        by: By::User,
    };
    let mut placed = Vec::new();
    let row = place_one(ctx, pl, &mut placed).await;
    Applied {
        rows: vec![row],
        placed,
    }
}

/// What a Replace did, for the command to announce.
pub struct Replaced {
    pub row: SupplyRow,
    pub trashed: Option<crate::mods::trash::Trashed>,
    pub recycled: Vec<String>,
    pub placed: Vec<PlacedFile>,
}

/// The confirmed Replace (spec D9): the pack's file is staged first, then the
/// occupants leave the way a user removal does (a mod to the instance trash,
/// anything else to the recycle bin), then the staged copy is renamed in. If
/// that rename fails, the trashed mods are put back and that result is checked.
///
/// `confirmed` is what the user was shown as leaving; if what would leave now
/// differs, nothing is written and the new names come back to be asked again.
pub async fn replace(ctx: &Ctx<'_>, key: &str, display: &str, confirmed: &str) -> Replaced {
    let refuse = |row: SupplyRow| Replaced {
        row,
        trashed: None,
        recycled: vec![],
        placed: vec![],
    };
    let Some(st) = status_for(ctx, key) else {
        return refuse(SupplyRow::NotInPack {
            path: display.to_string(),
            found: None,
            asked: false,
        });
    };
    let (src, ext, _) = match probe(display).await {
        Ok(p) => p,
        Err(row) => return refuse(row),
    };
    let want_md5 = matches!(state::expected(&st.entry), Expected::Md5(_));
    let d = match digest::digests(&src, want_md5).await {
        Ok(d) => d,
        Err(e) => {
            return refuse(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.to_string(),
            })
        }
    };
    if !hash_matches(st, &d) {
        return refuse(SupplyRow::NotInPack {
            path: display.to_string(),
            found: None,
            asked: false,
        });
    }
    let rel = match place::destination(&st.entry, &src, ext, &st.entry.filename).await {
        Ok(Some(rel)) => rel,
        Ok(None) => {
            return refuse(SupplyRow::CantPlace {
                path: display.to_string(),
            })
        }
        Err(e) => {
            return refuse(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.to_string(),
            })
        }
    };
    let mc = ctx.instance_root.join(".minecraft");
    let dest = mc.join(&rel);

    // Who leaves: other versions of the mod, and whatever holds the name.
    let others = replace_candidates(ctx, &st.entry);
    let mut shown = names_of(&others);
    let mut mod_shas: Vec<String> = others.iter().map(|r| r.sha1.clone()).collect();
    let mut loose: Vec<PathBuf> = Vec::new();
    match place::occupancy(
        &mc,
        &rel,
        &d.sha1,
        ctx.installed,
        &pack_file_shas(ctx.origin),
    )
    .await
    {
        Err(e) => {
            return refuse(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.to_string(),
            })
        }
        Ok(Occupancy::PackFile { existing }) => {
            return refuse(SupplyRow::Conflict {
                path: display.to_string(),
                existing,
            })
        }
        Ok(Occupancy::Taken { existing, row_sha1 }) => {
            if shown.is_empty() {
                shown = existing.clone();
            }
            match row_sha1 {
                Some(sha) if rel.starts_with("mods/") => mod_shas.push(sha),
                _ => loose.push(dest.with_file_name(existing)),
            }
        }
        Ok(Occupancy::Same) | Ok(Occupancy::Free) => {}
    }
    mod_shas.sort();
    mod_shas.dedup();
    if !shown.is_empty() && shown != confirmed {
        return refuse(SupplyRow::NeedsReplace {
            path: display.to_string(),
            key: key.to_string(),
            mod_name: st.entry.mod_name.clone(),
            existing: shown,
        });
    }

    if let Some(parent) = dest.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return refuse(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.to_string(),
            });
        }
    }
    let staged = match store::stage_copy_verified(&src, &dest, &d.sha1).await {
        Ok(s) => s,
        Err(StageError::Changed) => {
            digest::forget(&src);
            return refuse(SupplyRow::Changed {
                path: display.to_string(),
            });
        }
        Err(StageError::Io(e)) => {
            return refuse(SupplyRow::Unreadable {
                path: display.to_string(),
                reason: e.details(),
            })
        }
    };
    let trashed = if mod_shas.is_empty() {
        None
    } else {
        match crate::mods::trash::uninstall_to_trash(
            ctx.instance_root,
            &mod_shas,
            std::time::SystemTime::now(),
        )
        .await
        {
            Ok(t) => Some(t),
            Err(e) => {
                staged.discard().await;
                return refuse(SupplyRow::Unreadable {
                    path: display.to_string(),
                    reason: e.to_string(),
                });
            }
        }
    };
    let mut recycled = Vec::new();
    for path in loose {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match crate::platform::recycle_bin::off_thread(
            path,
            crate::platform::recycle_bin::to_recycle_bin,
        )
        .await
        {
            Ok(()) => {
                if let Some(kind) = crate::mods::install::content_kind_for_install_path(&rel) {
                    if let Err(e) =
                        crate::mods::assets::remove(ctx.instance_root, kind, &name).await
                    {
                        crate::diag!("[manual-downloads] assets row for {name} not removed: {e}");
                    }
                }
                recycled.push(name);
            }
            Err(e) => {
                staged.discard().await;
                let back = put_back(ctx.instance_root, trashed.as_ref()).await;
                return refuse(SupplyRow::Unreadable {
                    path: display.to_string(),
                    reason: format!("{name}: {e}{back}"),
                });
            }
        }
    }
    if let Err(e) = staged.commit().await {
        let back = put_back(ctx.instance_root, trashed.as_ref()).await;
        let reason = if recycled.is_empty() {
            format!("{}{back}", e.details())
        } else {
            format!(
                "{}{back} — {} in the recycle bin",
                e.details(),
                recycled.join(", ")
            )
        };
        return Replaced {
            row: SupplyRow::Unreadable {
                path: display.to_string(),
                reason,
            },
            trashed: None,
            recycled,
            placed: vec![],
        };
    }
    let pl = Placement {
        src,
        display: display.to_string(),
        key: key.to_string(),
        entry: st.entry.clone(),
        rel: rel.clone(),
        sha1: d.sha1.clone(),
        by: if want_md5 { By::Md5 } else { By::Sha1 },
    };
    let recorded = bookkeeping(ctx, &pl).await;
    Replaced {
        row: SupplyRow::Added {
            path: display.to_string(),
            key: key.to_string(),
            mod_name: st.entry.mod_name.clone(),
            recorded,
        },
        trashed,
        recycled,
        placed: vec![PlacedFile {
            sha1: d.sha1,
            filename: rel.rsplit('/').next().unwrap_or(&rel).to_string(),
            name: st.entry.mod_name.clone(),
            is_mod: rel.starts_with("mods/"),
        }],
    }
}

/// Bring trashed mods back after a failed Replace; the answer says how it went
/// (Fallback discipline Q4 — a recovery's own result is checked and told).
async fn put_back(instance_root: &Path, trashed: Option<&crate::mods::trash::Trashed>) -> String {
    let Some(t) = trashed else {
        return String::new();
    };
    match crate::mods::trash::restore(instance_root, &t.receipt.token).await {
        Ok(_) => " (the removed mod was put back)".to_string(),
        Err(e) => format!(" (the removed mod could not be put back: {e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::installed::PackOrigin;
    use crate::mods::manual_downloads::state::{self, tests::entry};
    use crate::mods::platform::ModSource;

    fn sha(b: &[u8]) -> String {
        hex::encode(<sha1::Sha1 as sha1::Digest>::digest(b))
    }

    fn origin(entries: Vec<crate::mods::modpack::schema::ModpackUnresolvable>) -> PackOrigin {
        PackOrigin {
            project_id: None,
            source: ModSource::Curseforge,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![],
            missing_mods: entries,
            skipped_overrides: vec![],
            resolved_missing: vec![],
            manual_supplied: vec![],
            inert_loader_jars: vec![],
        }
    }

    #[tokio::test]
    async fn a_matching_file_lands_under_the_packs_name_and_others_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        let downloads = dir.path().join("dl");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(downloads.join("SRP (1).jar"), b"pack bytes").unwrap();
        std::fs::write(downloads.join("other.jar"), b"nope!!!!!!").unwrap();
        std::fs::write(downloads.join("notes.txt"), b"x").unwrap();
        let mut e = entry(Some("9"), None, Some(&sha(b"pack bytes")));
        e.size = 10.0;
        let o = origin(vec![e]);
        let st = state::statuses(&o, &[], &Default::default());
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &[],
            statuses: &st,
        };
        let paths: Vec<String> = ["SRP (1).jar", "other.jar", "notes.txt"]
            .iter()
            .map(|n| downloads.join(n).display().to_string())
            .collect();
        let planned = plan(&ctx, &paths, None).await;
        let applied = apply(&ctx, planned).await;
        assert!(applied
            .rows
            .iter()
            .any(|r| matches!(r, SupplyRow::Added { recorded: true, .. })));
        assert!(applied
            .rows
            .iter()
            .any(|r| matches!(r, SupplyRow::NotInPack { asked: false, .. })));
        assert!(applied
            .rows
            .iter()
            .any(|r| matches!(r, SupplyRow::WrongType { .. })));
        assert_eq!(
            std::fs::read(root.join(".minecraft/mods/SRP.jar")).unwrap(),
            b"pack bytes"
        );
        assert!(
            downloads.join("SRP (1).jar").exists(),
            "the source is copied, never moved"
        );
        let rows = crate::mods::installed::list(&root).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, Some(ModSource::Curseforge));
        assert_eq!(rows[0].version_id.as_deref(), Some("9"));
    }

    #[tokio::test]
    async fn another_version_installed_asks_to_replace_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        std::fs::write(root.join(".minecraft/mods/srp-2.0.jar"), b"old").unwrap();
        let src = dir.path().join("SRP.jar");
        std::fs::write(&src, b"pack bytes").unwrap();
        let mut e = entry(None, None, Some(&sha(b"pack bytes")));
        e.size = 10.0;
        e.filename = "srp-1.0.jar".into();
        let o = origin(vec![e]);
        let installed = crate::mods::installed::list(&root).await.unwrap();
        let disk =
            crate::mods::manual_downloads::evidence::on_disk_shas(&root, &o, &installed).await;
        let st = state::statuses(&o, &installed, &disk);
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &installed,
            statuses: &st,
        };
        let applied = apply(&ctx, plan(&ctx, &[src.display().to_string()], None).await).await;
        assert!(
            matches!(&applied.rows[0], SupplyRow::NeedsReplace { existing, .. } if existing == "srp-2.0.jar")
        );
        assert!(!root.join(".minecraft/mods/srp-1.0.jar").exists());
    }

    #[tokio::test]
    async fn an_md5_match_notes_its_sha1() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        crate::mods::installed::set_pack_origin(&root, origin(vec![]))
            .await
            .unwrap();
        let src = dir.path().join("a.jar");
        std::fs::write(&src, b"abc").unwrap();
        let mut e = entry(None, Some("900150983cd24fb0d6963f7d28e17f72"), None);
        e.size = 3.0;
        e.filename = "a.jar".into();
        crate::mods::installed::update_pack_origin(&root, |o| o.missing_mods = vec![e.clone()])
            .await
            .unwrap();
        let o = crate::mods::installed::get_pack_origin(&root)
            .await
            .unwrap()
            .unwrap();
        let st = state::statuses(&o, &[], &Default::default());
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &[],
            statuses: &st,
        };
        let applied = apply(&ctx, plan(&ctx, &[src.display().to_string()], None).await).await;
        assert!(matches!(
            applied.rows[0],
            SupplyRow::Added { recorded: true, .. }
        ));
        let after = crate::mods::installed::get_pack_origin(&root)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.manual_supplied.len(), 1);
        assert_eq!(
            after.manual_supplied[0].sha1,
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }
    #[tokio::test]
    async fn an_already_placed_md5_file_that_cannot_be_noted_is_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        std::fs::write(root.join(".minecraft/mods/a.jar"), b"abc").unwrap();
        let src = dir.path().join("dl.jar");
        std::fs::write(&src, b"abc").unwrap();
        let mut e = entry(None, Some("900150983cd24fb0d6963f7d28e17f72"), None);
        e.size = 3.0;
        e.filename = "a.jar".into();
        // No pack record is written, so the note has nowhere to go.
        let o = origin(vec![e]);
        let st = state::statuses(&o, &[], &Default::default());
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &[],
            statuses: &st,
        };
        let applied = apply(&ctx, plan(&ctx, &[src.display().to_string()], None).await).await;
        assert!(
            matches!(
                applied.rows[0],
                SupplyRow::AlreadyThere {
                    recorded: false,
                    ..
                }
            ),
            "{:?}",
            applied.rows[0]
        );
    }

    #[tokio::test]
    async fn a_hashless_entry_takes_the_users_file_under_its_own_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        let e = entry(None, None, None);
        crate::mods::installed::set_pack_origin(&root, origin(vec![e.clone()]))
            .await
            .unwrap();
        let src = dir.path().join("my-choice.jar");
        std::fs::write(&src, b"mine").unwrap();
        let o = crate::mods::installed::get_pack_origin(&root)
            .await
            .unwrap()
            .unwrap();
        let st = state::statuses(&o, &[], &Default::default());
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &[],
            statuses: &st,
        };
        let applied =
            supply_unverified(&ctx, &state::entry_key(&e), &src.display().to_string()).await;
        assert!(matches!(
            applied.rows[0],
            SupplyRow::Added { recorded: true, .. }
        ));
        assert_eq!(applied.placed[0].filename, "my-choice.jar");
        assert!(root.join(".minecraft/mods/my-choice.jar").exists());
        let after = crate::mods::installed::get_pack_origin(&root)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.manual_supplied[0].verified, SupplyVerified::User);
    }

    #[test]
    fn a_placement_is_dropped_when_its_entry_closed_or_left_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let e = entry(Some("9"), None, Some(&sha(b"pack bytes")));
        let gone = entry(Some("10"), None, Some(&sha(b"other")));
        let o = origin(vec![e.clone()]);
        let st = vec![MissingModStatus {
            entry: e.clone(),
            state: MissingModState::Installed,
        }];
        let fresh = Ctx {
            instance_root: dir.path(),
            origin: &o,
            installed: &[],
            statuses: &st,
        };
        let place = |entry: &ModpackUnresolvable| {
            Planned::Place(Placement {
                src: PathBuf::from("a.jar"),
                display: "a.jar".into(),
                key: state::entry_key(entry),
                entry: entry.clone(),
                rel: "mods/a.jar".into(),
                sha1: "x".into(),
                by: By::Sha1,
            })
        };
        let out = revalidate(&fresh, vec![place(&e), place(&gone)]);
        assert!(matches!(
            out[0],
            Planned::Row(SupplyRow::AlreadyThere { recorded: true, .. })
        ));
        assert!(matches!(
            out[1],
            Planned::Row(SupplyRow::NotInPack {
                found: None,
                asked: false,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn a_file_is_placed_even_when_the_published_size_is_wrong() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        let src = dir.path().join("dl.jar");
        std::fs::write(&src, b"0123456789").unwrap();
        let mut e = entry(Some("9"), None, Some(&sha(b"0123456789")));
        e.size = 999.0;
        let o = origin(vec![e]);
        let st = state::statuses(&o, &[], &Default::default());
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &[],
            statuses: &st,
        };
        let applied = apply(&ctx, plan(&ctx, &[src.display().to_string()], None).await).await;
        assert!(
            matches!(applied.rows[0], SupplyRow::Added { .. }),
            "{:?}",
            applied.rows[0]
        );
    }

    #[tokio::test]
    async fn a_hashless_entry_with_another_version_installed_asks_to_replace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        std::fs::write(root.join(".minecraft/mods/srp-2.0.jar"), b"old").unwrap();
        let src = dir.path().join("my-choice.jar");
        std::fs::write(&src, b"mine").unwrap();
        let e = entry(None, None, None);
        let o = origin(vec![e.clone()]);
        let installed = crate::mods::installed::list(&root).await.unwrap();
        let disk =
            crate::mods::manual_downloads::evidence::on_disk_shas(&root, &o, &installed).await;
        let st = state::statuses(&o, &installed, &disk);
        assert_eq!(st[0].state, MissingModState::DifferentVersion);
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &installed,
            statuses: &st,
        };
        let applied =
            supply_unverified(&ctx, &state::entry_key(&e), &src.display().to_string()).await;
        assert!(
            matches!(&applied.rows[0], SupplyRow::NeedsReplace { existing, .. } if existing == "srp-2.0.jar"),
            "{:?}",
            applied.rows[0]
        );
        assert!(applied.placed.is_empty());
        assert!(!root.join(".minecraft/mods/my-choice.jar").exists());
    }

    #[tokio::test]
    async fn a_pack_file_is_never_listed_or_trashed_by_a_replace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        std::fs::write(root.join(".minecraft/mods/srp-2.0.jar"), b"old").unwrap();
        let mut e = entry(None, None, Some(&sha(b"pack bytes")));
        e.size = 10.0;
        e.filename = "srp-1.0.jar".into();
        let src = dir.path().join("SRP.jar");
        std::fs::write(&src, b"pack bytes").unwrap();
        let mut o = origin(vec![e.clone()]);
        o.files = vec![crate::mods::installed::PackOriginFile {
            sha1: sha(b"old"),
            name: "Bundled".into(),
            filename: "srp-2.0.jar".into(),
            install_path: "mods/srp-2.0.jar".into(),
            url: String::new(),
            size: 3.0,
            project_id: "1".into(),
            version_id: "1".into(),
            env_client: crate::mods::modpack::schema::EnvSupport::Required,
            source: ModSource::Curseforge,
        }];
        let installed = crate::mods::installed::list(&root).await.unwrap();
        let disk =
            crate::mods::manual_downloads::evidence::on_disk_shas(&root, &o, &installed).await;
        let st = state::statuses(&o, &installed, &disk);
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &installed,
            statuses: &st,
        };
        let planned = plan(&ctx, &[src.display().to_string()], None).await;
        assert!(
            matches!(planned[0], Planned::Place(_)),
            "a pack file is not an occupant to ask about: {planned:?}"
        );
        let done = replace(&ctx, &state::entry_key(&e), &src.display().to_string(), "").await;
        assert!(
            matches!(done.row, SupplyRow::Added { .. }),
            "{:?}",
            done.row
        );
        assert!(done.trashed.is_none());
        assert!(root.join(".minecraft/mods/srp-2.0.jar").exists());
    }

    #[tokio::test]
    async fn replace_moves_the_old_version_to_the_trash_and_places_the_packs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("inst");
        std::fs::create_dir_all(root.join(".minecraft/mods")).unwrap();
        std::fs::write(root.join(".minecraft/mods/srp-2.0.jar"), b"old").unwrap();
        let mut e = entry(None, None, Some(&sha(b"pack bytes")));
        e.size = 10.0;
        e.filename = "srp-1.0.jar".into();
        let src = dir.path().join("SRP.jar");
        std::fs::write(&src, b"pack bytes").unwrap();
        let o = origin(vec![e.clone()]);
        let installed = crate::mods::installed::list(&root).await.unwrap();
        let disk =
            crate::mods::manual_downloads::evidence::on_disk_shas(&root, &o, &installed).await;
        let st = state::statuses(&o, &installed, &disk);
        let ctx = Ctx {
            instance_root: &root,
            origin: &o,
            installed: &installed,
            statuses: &st,
        };
        let done = replace(
            &ctx,
            &state::entry_key(&e),
            &src.display().to_string(),
            "srp-2.0.jar",
        )
        .await;
        assert!(matches!(done.row, SupplyRow::Added { .. }));
        assert!(done.trashed.is_some());
        assert!(!root.join(".minecraft/mods/srp-2.0.jar").exists());
        assert_eq!(
            std::fs::read(root.join(".minecraft/mods/srp-1.0.jar")).unwrap(),
            b"pack bytes"
        );
    }
}
