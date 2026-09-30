//! Per-instance trash: the undo window of a user-facing mod uninstall (D2).
//!
//! Layout: `{instance}/lucerna/trash/<unix_ms>-<rand8>/` holds the moved jars
//! under their on-disk names (`<file>` or `<file>.disabled`) and `record.json`
//! (`{ items: [{ row, file_name_on_disk }] }`) — the registry rows verbatim, so a
//! restore brings back `enabled`, `requires` and `installed_at` unchanged.
//!
//! Only `mods_uninstall` / `mods_uninstall_many` come here; the internal
//! `install::uninstall` (update, migration apply, log repair) still deletes.
//!
//! Hardlinks: jars are MOVED (`rename`), never opened for writing, so a store
//! hardlink stays one inode with the store; a purge deletes one name only.
//! A move is a rename inside one instance folder: a `mods/` that is a link to
//! another volume cannot rename into `lucerna/`, and that uninstall fails with
//! the file named — nothing is deleted.
//!
//! Purge: entries older than [`TRASH_TTL_MS`] at every uninstall; entries from
//! earlier launcher sessions at start ([`purge_earlier_sessions`]). Both run
//! under the trash lock. Only a directory whose name is exactly a token is ever
//! deleted — anything else under `trash/` (a `-kept` entry a failed rollback set
//! aside) is not ours to delete.
//!
//! Fallback discipline: a move that fails midway puts back what it moved and
//! checks that (Q4); a jar that cannot go back — or whose name a concurrent
//! install took meanwhile, in either spelling — is kept out of the purge's
//! reach and named in the error. A restore never overwrites: a taken name
//! (either spelling) or a project installed again is skipped with its reason,
//! and its jar stays in the trash until the entry is purged.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::Error;
use crate::mods::installed;
use crate::mods::platform::InstalledMod;

/// How long an uninstall stays undoable across later uninstalls (D2).
pub const TRASH_TTL_MS: u64 = 10 * 60 * 1000;
const RECORD_FILE: &str = "record.json";
/// A unique temp name per record write — the `installed::write` shape.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct UninstalledItem {
    pub sha1: String,
    pub name: String,
}

/// `token` undoes the whole batch; `items` are the mods it removed, in the
/// order asked. No items (every digest was already gone) = nothing to undo.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct UninstallReceipt {
    pub token: String,
    pub items: Vec<UninstalledItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RestoreSkipReason {
    /// A file already answers to the mod's name, in either spelling — never overwritten.
    NameTaken,
    /// The jar is no longer in the trash.
    Missing,
    /// The same jar, or another build of the same project, is installed again.
    AlreadyInstalled,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SkippedRestore {
    pub name: String,
    pub reason: RestoreSkipReason,
}

/// `restored` and `skipped[].name` are display names. `expired`: the token's
/// entry is gone (purged) and nothing was restored.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub skipped: Vec<SkippedRestore>,
    pub expired: bool,
}

/// An uninstall: the receipt for the UI, the rows it removed for the journal and events.
#[derive(Debug)]
pub struct Trashed {
    pub receipt: UninstallReceipt,
    pub removed: Vec<InstalledMod>,
}

/// A restore. `failure`: it stopped at an item it could not restore — `rows`
/// still lists the ones already back, which the caller must announce.
#[derive(Debug)]
pub struct Restored {
    pub report: RestoreReport,
    pub rows: Vec<InstalledMod>,
    pub failure: Option<Error>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    items: Vec<RecordItem>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RecordItem {
    row: InstalledMod,
    file_name_on_disk: String,
}

enum Item {
    Back,
    Skipped(RestoreSkipReason),
}

/// Serialises uninstall, restore and both purges (per uninstall, at start), so a
/// restore never reads an entry a concurrent purge is deleting. Process-wide:
/// these are clicks, plus one pass per launcher start.
fn trash_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// `{instance}/lucerna/trash` — beside `installed-mods.json`, outside `.minecraft/`.
pub fn trash_dir(instance_root: &Path) -> PathBuf {
    installed::registry_dir(instance_root).join("trash")
}

fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn new_token(now: SystemTime) -> String {
    let rand: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect();
    format!("{}-{rand}", unix_ms(now))
}

/// The token's creation time, or `None` for anything that is not exactly
/// `<digits>-<8 lowercase hex>` — which is also what makes it safe to join.
fn parse_token(name: &str) -> Option<u64> {
    let (ms, rand) = name.split_once('-')?;
    let hex = rand.len() == 8 && rand.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    // At most 19 digits: always within `u64`, so the parse cannot fail.
    let digits = !ms.is_empty() && ms.len() <= 19 && ms.bytes().all(|b| b.is_ascii_digit());
    if hex && digits {
        ms.parse().ok()
    } else {
        None
    }
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::ModsInstancePath {
        path: path.display().to_string(),
        details: e.to_string(),
    }
}

/// Purge expired entries, then move `sha1s`' jars into ONE new token entry,
/// record their rows and drop them from the registry. An unknown sha is already
/// gone and is skipped — the idempotence `install::uninstall` always had.
pub async fn uninstall_to_trash(
    instance_root: &Path,
    sha1s: &[String],
    now: SystemTime,
) -> Result<Trashed, Error> {
    let _serial = trash_lock().lock().await;
    let root = instance_root.to_path_buf();
    let cutoff = unix_ms(now).saturating_sub(TRASH_TTL_MS);
    let lines = tokio::task::spawn_blocking(move || purge(&root, cutoff))
        .await
        .unwrap_or_else(|e| vec![format!("mods trash: purge task failed: {e}")]);
    for line in lines {
        crate::diag!("{line}");
    }
    move_to_trash(instance_root, sha1s, now).await
}

async fn move_to_trash(
    instance_root: &Path,
    sha1s: &[String],
    now: SystemTime,
) -> Result<Trashed, Error> {
    let rows = installed::list(instance_root).await?;
    let mut targets: Vec<InstalledMod> = Vec::new();
    for sha in sha1s {
        if let Some(row) = rows.iter().find(|m| m.sha1.eq_ignore_ascii_case(sha)) {
            if !targets
                .iter()
                .any(|t| t.sha1.eq_ignore_ascii_case(&row.sha1))
            {
                targets.push(row.clone());
            }
        }
    }
    let token = new_token(now);
    if targets.is_empty() {
        // Nothing the registry knows: nothing moves, and no empty entry is left
        // for the purge. A receipt without items offers no undo.
        return Ok(Trashed {
            receipt: UninstallReceipt {
                token,
                items: Vec::new(),
            },
            removed: Vec::new(),
        });
    }
    let parent = trash_dir(instance_root);
    fs::create_dir_all(&parent)
        .await
        .map_err(|e| io_err(&parent, e))?;
    let dir = parent.join(&token);
    // `create_dir`, not `_all`: a colliding token fails here, before anything moves.
    fs::create_dir(&dir).await.map_err(|e| io_err(&dir, e))?;

    let mods = installed::mods_dir(instance_root);
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut items = Vec::new();
    for row in &targets {
        let name = installed::on_disk_name(row);
        let from = mods.join(&name);
        match fs::try_exists(&from).await {
            Ok(true) => {
                let to = dir.join(&name);
                if let Err(e) = fs::rename(&from, &to).await {
                    return Err(abort(&dir, &moved, io_err(&from, e)).await);
                }
                moved.push((from, to));
            }
            // Gone since the `list` above: there is no jar to keep, but the row
            // is recorded all the same, so an undo reports it as missing rather
            // than passing over a mod the receipt says was removed.
            Ok(false) => {}
            Err(e) => return Err(abort(&dir, &moved, io_err(&from, e)).await),
        }
        items.push(RecordItem {
            row: row.clone(),
            file_name_on_disk: name,
        });
    }
    if let Err(e) = write_record(&dir, &Record { items }).await {
        let path = dir.join(RECORD_FILE);
        return Err(abort(&dir, &moved, io_err(&path, e)).await);
    }
    let shas: HashSet<String> = targets
        .iter()
        .map(|t| t.sha1.to_ascii_lowercase())
        .collect();
    if let Err(e) = installed::remove_many(instance_root, &shas).await {
        return Err(abort(&dir, &moved, e).await);
    }
    let receipt = UninstallReceipt {
        token,
        items: targets
            .iter()
            .map(|t| UninstalledItem {
                sha1: t.sha1.clone(),
                name: t.name.clone(),
            })
            .collect(),
    };
    Ok(Trashed {
        receipt,
        removed: targets,
    })
}

/// Temp-then-rename, the `hash_cache::save` shape (the entry is fresh and private,
/// but a torn record would strand every jar in it).
async fn write_record(dir: &Path, record: &Record) -> std::io::Result<()> {
    let final_path = dir.join(RECORD_FILE);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    fs::write(&tmp, &bytes).await?;
    fs::rename(&tmp, &final_path).await
}

/// Put back every jar this call moved, newest first. Returns the ones that could
/// NOT go back (their names in the entry, with the cause) — the caller must say so.
///
/// Never over another file: the shared maintenance claim admits a concurrent
/// install, and `rename` replaces its target on Windows. A jar whose name was
/// taken meanwhile — in either spelling, `restore_item`'s `NameTaken` rule — or
/// could not be checked stays in the entry, counted with the stuck ones.
async fn put_back(moved: &[(PathBuf, PathBuf)]) -> Vec<String> {
    let mut stuck = Vec::new();
    for (from, to) in moved.iter().rev() {
        let name = to
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| to.display().to_string());
        if let Some(why) = way_back_blocked(from).await {
            crate::diag!("mods trash: kept {} in the trash: {why}", to.display());
            stuck.push(format!("{name} ({why})"));
            continue;
        }
        if let Err(e) = fs::rename(to, from).await {
            crate::diag!(
                "mods trash: could not move {} back to {}: {e}",
                to.display(),
                from.display()
            );
            stuck.push(format!("{name} ({e})"));
        }
    }
    stuck
}

/// Why a jar must not go back to `from`, if it must not: its base name (the
/// file name without a trailing `.disabled`, as the registry reads names off
/// disk) is taken in `from`'s folder in either spelling, or that cannot be told.
async fn way_back_blocked(from: &Path) -> Option<String> {
    let (Some(mods), Some(file)) = (from.parent(), from.file_name()) else {
        return Some(format!("{} names no file in a folder", from.display()));
    };
    let file = file.to_string_lossy().into_owned();
    let base = file.strip_suffix(".disabled").unwrap_or(file.as_str());
    match taken_spelling(mods, base).await {
        Ok(None) => None,
        Ok(Some(spelling)) => Some(format!("name taken by {spelling}")),
        Err(e) => Some(format!("could not check the name: {e}")),
    }
}

/// The spelling of `base` a file in `mods` already answers to — `<base>` or
/// `<base>.disabled` — if any. Either one taken means two jars would share one
/// base name, and a later enable/disable would rename one over the other.
/// `Err`: it could not be checked.
async fn taken_spelling(mods: &Path, base: &str) -> Result<Option<String>, Error> {
    for spelling in [base.to_string(), format!("{base}.disabled")] {
        let p = mods.join(&spelling);
        if fs::try_exists(&p).await.map_err(|e| io_err(&p, e))? {
            return Ok(Some(spelling));
        }
    }
    Ok(None)
}

/// Undo a half-done move and turn `cause` into the error to report.
async fn abort(dir: &Path, moved: &[(PathBuf, PathBuf)], cause: Error) -> Error {
    let stuck = put_back(moved).await;
    if stuck.is_empty() {
        if let Err(e) = fs::remove_dir_all(dir).await {
            // Nothing of the user's is left in it; the purge retries the empty entry.
            crate::diag!(
                "mods trash: could not remove {} after a rollback: {e}",
                dir.display()
            );
        }
        return cause;
    }
    let (folder, fate) = match keep_out_of_purge(dir).await {
        Ok(kept) => (kept, "they are kept in this folder"),
        Err(e) => {
            crate::diag!("mods trash: could not set {} aside: {e}", dir.display());
            (
                dir.to_path_buf(),
                "they are in this folder, which could not be set aside, so the trash purge \
                 clears it (ten minutes, or the next start)",
            )
        }
    };
    Error::ModsInstancePath {
        path: folder.display().to_string(),
        details: format!(
            "{cause}; could not be moved back to the mods folder, {fate}: {}",
            stuck.join(", ")
        ),
    }
}

/// `<token>-kept` is not a token, so no purge ever deletes it.
async fn keep_out_of_purge(dir: &Path) -> std::io::Result<PathBuf> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kept = dir.with_file_name(format!("{name}-kept"));
    fs::rename(dir, &kept).await?;
    Ok(kept)
}

/// Put back what `token`'s entry holds. `Err` = nothing was restored (a token
/// that is not ours, an unreadable record or registry); a failure midway is
/// `Restored::failure`, with the items already back in `rows`.
pub async fn restore(instance_root: &Path, token: &str) -> Result<Restored, Error> {
    let _serial = trash_lock().lock().await;
    if parse_token(token).is_none() {
        return Err(Error::ModsInstancePath {
            path: token.to_string(),
            details: "not an uninstall token".into(),
        });
    }
    let dir = trash_dir(instance_root).join(token);
    let record_path = dir.join(RECORD_FILE);
    let bytes = match fs::read(&record_path).await {
        Ok(b) => b,
        // Purged (or a move that never finished): nothing to bring back.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Restored {
                report: RestoreReport {
                    restored: Vec::new(),
                    skipped: Vec::new(),
                    expired: true,
                },
                rows: Vec::new(),
                failure: None,
            })
        }
        Err(e) => return Err(io_err(&record_path, e)),
    };
    let record: Record = serde_json::from_slice(&bytes).map_err(|e| Error::ModsDecode {
        platform: "trash record.json".into(),
        details: e.to_string(),
    })?;
    let registry = installed::list(instance_root).await?;
    let mods = installed::mods_dir(instance_root);
    fs::create_dir_all(&mods)
        .await
        .map_err(|e| io_err(&mods, e))?;

    let mut out = Restored {
        report: RestoreReport {
            restored: Vec::new(),
            skipped: Vec::new(),
            expired: false,
        },
        rows: Vec::new(),
        failure: None,
    };
    let mut kept_in_trash = 0usize;
    for item in record.items {
        match restore_item(&dir, &mods, instance_root, &registry, &item).await {
            Ok(Item::Back) => {
                out.report.restored.push(item.row.name.clone());
                out.rows.push(item.row);
            }
            Ok(Item::Skipped(reason)) => {
                if reason != RestoreSkipReason::Missing {
                    kept_in_trash += 1;
                }
                out.report.skipped.push(SkippedRestore {
                    name: item.row.name.clone(),
                    reason,
                });
            }
            Err(e) => {
                out.failure = Some(e);
                break;
            }
        }
    }
    if out.failure.is_none() && kept_in_trash == 0 {
        if let Err(e) = fs::remove_dir_all(&dir).await {
            // Everything that could come back is back; the empty entry is the purge's job.
            crate::diag!("mods trash: could not remove {}: {e}", dir.display());
        }
    }
    Ok(out)
}

async fn restore_item(
    dir: &Path,
    mods: &Path,
    instance_root: &Path,
    registry: &[InstalledMod],
    item: &RecordItem,
) -> Result<Item, Error> {
    let row = &item.row;
    let same_project = |m: &InstalledMod| {
        row.source.is_some()
            && row.project_id.is_some()
            && m.source == row.source
            && m.project_id == row.project_id
    };
    if registry
        .iter()
        .any(|m| m.sha1.eq_ignore_ascii_case(&row.sha1) || same_project(m))
    {
        return Ok(Item::Skipped(RestoreSkipReason::AlreadyInstalled));
    }
    // Our own file, but bytes read from disk: never join a name that is not a plain file name.
    if !crate::pathsafe::is_safe_filename(&item.file_name_on_disk)
        || !crate::pathsafe::is_safe_filename(&row.filename)
    {
        return Ok(Item::Skipped(RestoreSkipReason::Missing));
    }
    let from = dir.join(&item.file_name_on_disk);
    if !fs::try_exists(&from).await.map_err(|e| io_err(&from, e))? {
        return Ok(Item::Skipped(RestoreSkipReason::Missing));
    }
    if taken_spelling(mods, &row.filename).await?.is_some() {
        return Ok(Item::Skipped(RestoreSkipReason::NameTaken));
    }
    let to = mods.join(&item.file_name_on_disk);
    fs::rename(&from, &to).await.map_err(|e| io_err(&from, e))?;
    if let Err(e) = installed::add(instance_root, row.clone()).await {
        // A jar without its row would come back as an anonymous manual mod.
        return Err(match fs::rename(&to, &from).await {
            Ok(()) => e,
            Err(back) => Error::ModsInstancePath {
                path: to.display().to_string(),
                details: format!(
                    "{e}; the jar stayed in the mods folder without its record: {back}"
                ),
            },
        });
    }
    Ok(Item::Back)
}

/// Delete `instance_root`'s trash entries created before `created_before_ms`.
/// One line per failure, for the caller to `diag!`.
pub(crate) fn purge(instance_root: &Path, created_before_ms: u64) -> Vec<String> {
    let root = trash_dir(instance_root);
    let entries = match std::fs::read_dir(&root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => return vec![format!("mods trash: cannot list {}: {e}", root.display())],
    };
    let mut lines = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                lines.push(format!("mods trash: cannot read {}: {e}", root.display()));
                continue;
            }
        };
        let old = parse_token(&entry.file_name().to_string_lossy())
            .is_some_and(|ms| ms < created_before_ms);
        if !old {
            continue;
        }
        let path = entry.path();
        if let Err(e) = crate::data_root::recovery::remove_no_follow(&path) {
            lines.push(format!(
                "mods trash: could not purge {}: {e}",
                path.display()
            ));
        }
    }
    lines
}

/// Launcher start: `purge_all_instances` behind the trash lock, like every
/// other trash writer, so it never deletes an entry a restore is reading. One
/// line per failure for the caller to `diag!` — a purge that could not run at
/// all is one line too.
pub async fn purge_earlier_sessions(
    instances_dir: PathBuf,
    session_start: SystemTime,
) -> Vec<String> {
    let _serial = trash_lock().lock().await;
    tokio::task::spawn_blocking(move || purge_all_instances(&instances_dir, session_start))
        .await
        .unwrap_or_else(|e| vec![format!("mods trash: start purge task failed: {e}")])
}

/// Empty every instance's trash of entries from EARLIER sessions ("cleared at
/// next start"). The cutoff is this session's start, so an uninstall made while
/// this runs keeps its undo window. Unlocked: reached only through
/// [`purge_earlier_sessions`].
fn purge_all_instances(instances_dir: &Path, session_start: SystemTime) -> Vec<String> {
    let cutoff = unix_ms(session_start);
    let entries = match std::fs::read_dir(instances_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            return vec![format!(
                "mods trash: cannot list {}: {e}",
                instances_dir.display()
            )]
        }
    };
    let mut lines = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                lines.push(format!(
                    "mods trash: cannot read {}: {e}",
                    instances_dir.display()
                ));
                continue;
            }
        };
        match entry.file_type() {
            Ok(t) if t.is_dir() => lines.extend(purge(&entry.path(), cutoff)),
            // A file or a link is not an instance directory.
            Ok(_) => {}
            Err(e) => lines.push(format!(
                "mods trash: cannot stat {}: {e}",
                entry.path().display()
            )),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModSource;
    use sha1::{Digest, Sha1};
    use tempfile::TempDir;

    /// A provenanced row plus its jar in `mods/` (`<file>` enabled, `<file>.disabled` not).
    async fn place(
        root: &Path,
        file: &str,
        body: &[u8],
        project: &str,
        enabled: bool,
    ) -> InstalledMod {
        let dir = installed::mods_dir(root);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let on_disk = if enabled {
            file.to_string()
        } else {
            format!("{file}.disabled")
        };
        tokio::fs::write(dir.join(on_disk), body).await.unwrap();
        let row = InstalledMod {
            filename: file.into(),
            sha1: hex::encode(Sha1::digest(body)),
            source: Some(ModSource::Modrinth),
            project_id: Some(project.into()),
            version_id: Some(format!("{project}-v1")),
            name: project.to_uppercase(),
            version_number: Some("1.0".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled,
            enrich_attempted: true,
            requires: vec!["lib-x".into()],
        };
        installed::add(root, row.clone()).await.unwrap();
        row
    }

    fn token_at(ms: u64) -> String {
        format!("{ms}-0123abcd")
    }

    #[tokio::test]
    async fn uninstall_moves_both_spellings_into_one_token_and_drops_the_rows() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let a = place(root, "a.jar", b"A", "pa", true).await;
        let b = place(root, "b.jar", b"B", "pb", false).await;
        let t = uninstall_to_trash(
            root,
            &[a.sha1.clone(), b.sha1.to_uppercase()],
            SystemTime::now(),
        )
        .await
        .unwrap();
        let dir = trash_dir(root).join(&t.receipt.token);
        assert!(
            parse_token(&t.receipt.token).is_some(),
            "{}",
            t.receipt.token
        );
        assert!(dir.join("a.jar").is_file() && dir.join("b.jar.disabled").is_file());
        assert!(dir.join("record.json").is_file());
        assert!(!installed::mods_dir(root).join("a.jar").exists());
        assert!(!installed::mods_dir(root).join("b.jar.disabled").exists());
        assert!(
            installed::list(root).await.unwrap().is_empty(),
            "rows dropped in one RMW"
        );
        let names: Vec<&str> = t.receipt.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["PA", "PB"]);
        assert_eq!(
            t.removed.len(),
            2,
            "rows handed back for the journal and the events"
        );
    }

    #[tokio::test]
    async fn an_unknown_sha_is_already_gone_and_is_skipped() {
        let td = TempDir::new().unwrap();
        let a = place(td.path(), "a.jar", b"A", "pa", true).await;
        let t = uninstall_to_trash(
            td.path(),
            &["ffff".into(), a.sha1.clone()],
            SystemTime::now(),
        )
        .await
        .unwrap();
        assert_eq!(t.receipt.items.len(), 1);
    }

    /// Nothing the registry knows: nothing moves, so no entry is left behind
    /// for the purge — and a receipt with no items offers no undo.
    #[tokio::test]
    async fn nothing_to_remove_leaves_no_trash_entry() {
        let td = TempDir::new().unwrap();
        let t = uninstall_to_trash(td.path(), &["ffff".into()], SystemTime::now())
            .await
            .unwrap();
        assert!(t.receipt.items.is_empty() && t.removed.is_empty());
        assert!(
            parse_token(&t.receipt.token).is_some(),
            "still a well-formed token"
        );
        assert!(
            !trash_dir(td.path()).exists(),
            "no trash was created for nothing"
        );
    }

    #[tokio::test]
    async fn an_uninstall_first_purges_entries_older_than_ten_minutes() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let now = SystemTime::now();
        let old = trash_dir(root).join(token_at(unix_ms(now) - 11 * 60 * 1000));
        let fresh = trash_dir(root).join(token_at(unix_ms(now) - 60 * 1000));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&fresh).unwrap();
        let a = place(root, "a.jar", b"A", "pa", true).await;
        uninstall_to_trash(root, &[a.sha1], now).await.unwrap();
        assert!(!old.exists(), "older than ten minutes: purged");
        assert!(fresh.exists(), "a recent undo stays available");
    }

    #[tokio::test]
    async fn restore_puts_every_jar_back_and_re_adds_its_row_verbatim() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let a = place(root, "a.jar", b"A", "pa", true).await;
        let b = place(root, "b.jar", b"B", "pb", false).await;
        let t = uninstall_to_trash(root, &[a.sha1.clone(), b.sha1.clone()], SystemTime::now())
            .await
            .unwrap();
        let out = restore(root, &t.receipt.token).await.unwrap();
        assert!(out.failure.is_none(), "{:?}", out.failure);
        assert_eq!(out.report.restored, ["PA", "PB"]);
        assert!(out.report.skipped.is_empty() && !out.report.expired);
        assert!(installed::mods_dir(root).join("a.jar").is_file());
        assert!(installed::mods_dir(root).join("b.jar.disabled").is_file());
        let rows = installed::list(root).await.unwrap();
        for original in [&a, &b] {
            let back = rows
                .iter()
                .find(|r| r.sha1 == original.sha1)
                .expect("row back");
            assert_eq!(
                serde_json::to_value(back).unwrap(),
                serde_json::to_value(original).unwrap(),
                "enabled, requires, installed_at … all verbatim"
            );
        }
        assert!(
            !trash_dir(root).join(&t.receipt.token).exists(),
            "an emptied entry is removed"
        );
    }

    #[tokio::test]
    async fn restore_never_overwrites_a_taken_name_and_keeps_that_jar() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let a = place(root, "a.jar", b"A", "pa", true).await;
        let t = uninstall_to_trash(root, &[a.sha1.clone()], SystemTime::now())
            .await
            .unwrap();
        // Another jar now answers to the name — in the OTHER spelling.
        let other = installed::mods_dir(root).join("a.jar.disabled");
        tokio::fs::write(&other, b"OTHER").await.unwrap();
        let out = restore(root, &t.receipt.token).await.unwrap();
        assert!(out.report.restored.is_empty());
        assert_eq!(out.report.skipped[0].reason, RestoreSkipReason::NameTaken);
        assert_eq!(tokio::fs::read(&other).await.unwrap(), b"OTHER");
        assert!(
            trash_dir(root)
                .join(&t.receipt.token)
                .join("a.jar")
                .is_file(),
            "kept, not lost"
        );
    }

    #[tokio::test]
    async fn restore_skips_a_project_that_is_installed_again() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let a = place(root, "a.jar", b"A", "pa", true).await;
        let t = uninstall_to_trash(root, &[a.sha1.clone()], SystemTime::now())
            .await
            .unwrap();
        place(root, "a-2.jar", b"A2", "pa", true).await; // a newer build of the same project
        let out = restore(root, &t.receipt.token).await.unwrap();
        assert_eq!(
            out.report.skipped[0].reason,
            RestoreSkipReason::AlreadyInstalled
        );
        assert!(
            !installed::mods_dir(root).join("a.jar").exists(),
            "never two builds of one mod"
        );
    }

    #[tokio::test]
    async fn restore_reports_a_jar_gone_from_the_trash_as_missing() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let a = place(root, "a.jar", b"A", "pa", true).await;
        let t = uninstall_to_trash(root, &[a.sha1.clone()], SystemTime::now())
            .await
            .unwrap();
        std::fs::remove_file(trash_dir(root).join(&t.receipt.token).join("a.jar")).unwrap();
        let out = restore(root, &t.receipt.token).await.unwrap();
        assert_eq!(out.report.skipped[0].reason, RestoreSkipReason::Missing);
        assert!(
            installed::list(root).await.unwrap().is_empty(),
            "no row without its jar"
        );
    }

    #[tokio::test]
    async fn restore_of_a_purged_token_reports_expired() {
        let td = TempDir::new().unwrap();
        let out = restore(td.path(), &token_at(1000)).await.unwrap();
        assert!(out.report.expired && out.report.restored.is_empty());
        assert!(out.rows.is_empty() && out.failure.is_none());
    }

    #[tokio::test]
    async fn restore_refuses_a_token_that_is_not_ours() {
        let td = TempDir::new().unwrap();
        for bad in [
            "",
            "../x",
            "1000",
            "1000-XYZ12345",
            "1000-0123abcd/..",
            "1000-0123abcd-kept",
        ] {
            assert!(restore(td.path(), bad).await.is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn a_clean_rollback_leaves_no_entry_and_reports_the_cause() {
        let td = TempDir::new().unwrap();
        let mods = installed::mods_dir(td.path());
        let dir = trash_dir(td.path()).join(token_at(1000));
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jar"), b"A").unwrap();
        let cause = Error::ModsNotFound {
            platform: "test".into(),
        };
        let err = abort(
            &dir,
            &[(mods.join("a.jar"), dir.join("a.jar"))],
            cause.clone(),
        )
        .await;
        assert_eq!(err, cause);
        assert!(mods.join("a.jar").is_file() && !dir.exists());
    }

    #[tokio::test]
    async fn a_rollback_that_cannot_put_a_jar_back_keeps_the_entry_and_names_it() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let mods = installed::mods_dir(root);
        let dir = trash_dir(root).join(token_at(1000));
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jar"), b"A").unwrap();
        // `b.jar` "moved" too, but its trash copy is gone: its way back fails (Fallback Q4).
        let moved = vec![
            (mods.join("a.jar"), dir.join("a.jar")),
            (mods.join("b.jar"), dir.join("b.jar")),
        ];
        let err = abort(
            &dir,
            &moved,
            Error::ModsNotFound {
                platform: "test".into(),
            },
        )
        .await;
        assert!(mods.join("a.jar").is_file(), "what can go back does");
        let kept = trash_dir(root).join(format!("{}-kept", token_at(1000)));
        assert!(kept.is_dir(), "the entry is set aside");
        assert!(
            matches!(&err, Error::ModsInstancePath { details, .. } if details.contains("b.jar")),
            "{err:?}"
        );
        assert!(purge(root, u64::MAX).is_empty());
        assert!(kept.is_dir(), "and no purge deletes it");
    }

    /// The shared maintenance claim admits a concurrent install: while a bulk
    /// uninstall was moving jars, another jar may have landed under a moved
    /// one's name — in either spelling. `rename` replaces its target on
    /// Windows, so the rollback must leave the moved jar in the entry, set
    /// aside and named, rather than destroy what arrived meanwhile.
    #[tokio::test]
    async fn a_rollback_never_puts_a_jar_back_over_a_name_taken_meanwhile() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let mods = installed::mods_dir(root);
        let dir = trash_dir(root).join(token_at(1000));
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jar"), b"OLD-A").unwrap();
        std::fs::write(dir.join("b.jar"), b"OLD-B").unwrap();
        // Placed by a concurrent install after the move: the same name, and the
        // other spelling of the second jar's name.
        std::fs::write(mods.join("a.jar"), b"NEW-A").unwrap();
        std::fs::write(mods.join("b.jar.disabled"), b"NEW-B").unwrap();
        let moved = vec![
            (mods.join("a.jar"), dir.join("a.jar")),
            (mods.join("b.jar"), dir.join("b.jar")),
        ];
        let err = abort(
            &dir,
            &moved,
            Error::ModsNotFound {
                platform: "test".into(),
            },
        )
        .await;
        assert_eq!(
            std::fs::read(mods.join("a.jar")).unwrap(),
            b"NEW-A",
            "the jar placed meanwhile is intact"
        );
        assert!(
            !mods.join("b.jar").exists(),
            "never a second spelling of one name"
        );
        let kept = trash_dir(root).join(format!("{}-kept", token_at(1000)));
        assert_eq!(std::fs::read(kept.join("a.jar")).unwrap(), b"OLD-A");
        assert_eq!(std::fs::read(kept.join("b.jar")).unwrap(), b"OLD-B");
        assert!(
            matches!(&err, Error::ModsInstancePath { details, .. }
                if details.contains("a.jar") && details.contains("b.jar")),
            "{err:?}"
        );
    }

    #[test]
    fn purge_removes_only_tokens_older_than_the_cutoff_and_nothing_foreign() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let trash = trash_dir(root);
        let kept = format!("{}-kept", token_at(1000));
        for name in [token_at(1000), token_at(5000), "notes".into(), kept.clone()] {
            std::fs::create_dir_all(trash.join(name)).unwrap();
        }
        assert!(purge(root, 3000).is_empty());
        assert!(!trash.join(token_at(1000)).exists());
        assert!(trash.join(token_at(5000)).exists());
        assert!(purge(root, u64::MAX).is_empty());
        assert!(!trash.join(token_at(5000)).exists());
        assert!(
            trash.join("notes").exists() && trash.join(kept).exists(),
            "not ours to delete"
        );
        assert!(
            purge(&root.join("no-such-instance"), u64::MAX).is_empty(),
            "no trash is no error"
        );
    }

    #[test]
    fn launcher_start_empties_every_instance_trash_from_earlier_sessions() {
        let td = TempDir::new().unwrap();
        let start = SystemTime::now();
        let (a, b) = (td.path().join("inst-a"), td.path().join("inst-b"));
        let earlier = token_at(unix_ms(start) - 1);
        let later = token_at(unix_ms(start) + 1);
        std::fs::create_dir_all(trash_dir(&a).join(&earlier)).unwrap();
        std::fs::create_dir_all(trash_dir(&b).join(&earlier)).unwrap();
        std::fs::create_dir_all(trash_dir(&b).join(&later)).unwrap();
        assert!(purge_all_instances(td.path(), start).is_empty());
        assert!(!trash_dir(&a).join(&earlier).exists() && !trash_dir(&b).join(&earlier).exists());
        assert!(
            trash_dir(&b).join(&later).exists(),
            "this session's undo window survives"
        );
        assert!(purge_all_instances(&td.path().join("none"), start).is_empty());
    }

    /// The start purge is a trash writer like any other: it waits while an
    /// uninstall or a restore holds the lock, then empties the earlier
    /// sessions' entries and keeps this session's.
    #[tokio::test]
    async fn the_start_purge_waits_for_the_trash_lock_and_keeps_this_session() {
        let td = TempDir::new().unwrap();
        let start = SystemTime::now();
        let inst = td.path().join("inst-a");
        let earlier = trash_dir(&inst).join(token_at(unix_ms(start) - 1));
        let later = trash_dir(&inst).join(token_at(unix_ms(start) + 1));
        std::fs::create_dir_all(&earlier).unwrap();
        std::fs::create_dir_all(&later).unwrap();

        let held = trash_lock().lock().await;
        let purge = tokio::spawn(purge_earlier_sessions(td.path().to_path_buf(), start));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(earlier.exists(), "nothing is purged while the lock is held");
        drop(held);

        let lines = purge.await.unwrap();
        assert!(lines.is_empty(), "{lines:?}");
        assert!(!earlier.exists(), "an earlier session's entry is purged");
        assert!(later.exists(), "this session's undo window survives");
    }
}
