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
    let candidates: Vec<&MissingModStatus> = ctx
        .statuses
        .iter()
        .filter(|s| {
            matches!(
                s.state,
                MissingModState::Missing | MissingModState::DifferentVersion
            ) && state::is_actionable(&s.entry)
                && state::expected(&s.entry) != Expected::Nothing
                && (s.entry.size <= 0.0 || s.entry.size as u64 == len)
        })
        .collect();
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
        let existing = state::different_version_rows(&st.entry, ctx.installed)
            .iter()
            .map(|r| r.filename.clone())
            .collect::<Vec<_>>()
            .join(", ");
        return One::Planned(Planned::Row(SupplyRow::NeedsReplace {
            path: display.to_string(),
            key,
            mod_name: st.entry.mod_name.clone(),
            existing,
        }));
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
            bookkeeping(ctx, &pl).await;
            SupplyRow::AlreadyThere {
                path,
                key,
                mod_name,
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
                Err(StageError::Changed) => SupplyRow::Changed { path },
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

/// The entry `key` names in this context.
pub(crate) fn status_for<'a>(ctx: &'a Ctx<'_>, key: &str) -> Option<&'a MissingModStatus> {
    ctx.statuses
        .iter()
        .find(|s| state::entry_key(&s.entry) == key)
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
}
