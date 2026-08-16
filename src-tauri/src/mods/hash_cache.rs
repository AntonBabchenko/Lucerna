//! Per-instance memo of `(mtime, size) -> SHA-1` for the files in an instance's
//! `mods/` directory.
//!
//! File: `{instance}/lucerna/jar-hashes.json`, beside `installed-mods.json`.
//! Schema v1. Derived data — safe to delete at any time; the next list rebuilds
//! it by hashing.
//!
//! ## Why this exists
//!
//! [`crate::mods::installed`]'s `reconcile` needs the SHA-1 of every file in
//! `mods/` on every read of the installed list. The process-lifetime
//! `HASH_CACHE` in that module already turns a repeat read into a `stat`, but it
//! dies with the process: the FIRST list of each instance after every launcher
//! start re-reads and re-hashes every jar — gigabytes on a large modpack, on the
//! path that paints the Installed tab. This file carries the memo across
//! restarts so a cold start is stat-only.
//!
//! ## Why a separate file, and not extra fields on `InstalledMod`
//!
//! * `InstalledMod` is `specta::Type` and crosses IPC into `bindings.ts`. Disk
//!   stats have no meaning in the UI, and specta forbids `u64` exports (see the
//!   `size: f64` note on `PackOriginFile`) — an mtime narrowed to `f64`
//!   milliseconds compares equal across changes NTFS can actually represent,
//!   i.e. a memo that answers "unchanged" for a changed file.
//! * `reconcile` needs a digest for every file in `mods/` BEFORE it knows which
//!   record owns it, and the hand-dropped jars it synthesizes have no record to
//!   read a digest from at all — precisely the population that costs most on a
//!   hand-managed instance.
//! * `reconcile` deliberately RETAINS a record whose filename is present but
//!   whose bytes changed, keeping its EXPECTED sha1 so a repair knows what it
//!   wants (`corrupted_known_jar_keeps_its_provenance`). Storing that record's
//!   on-disk `(mtime, size)` beside a hash that deliberately does NOT describe
//!   the file on disk invites exactly the mis-pairing that would make a corrupt
//!   jar read green.
//! * `installed-mods.json` carries provenance and is treated as EMPTY when it
//!   fails to parse. Putting a disposable performance memo behind that schema
//!   risks user-bearing data for no gain — the hazard `summary_cache`'s module
//!   doc spells out for its own file.
//!
//! ## Hardlink safety
//!
//! Nothing here writes content bytes and nothing here writes inside
//! `.minecraft/`. The one file it writes lives in the instance's `lucerna/`
//! METADATA directory — a sibling of `.minecraft/`, the same class as
//! `installed-mods.json`, never one of the hardlinked names
//! `tests/structural_no_inplace_mods_write.rs` protects — and it is written
//! temp-then-rename with a per-write sequence number, the same shape
//! `installed::write` uses.
//!
//! `mtime` and `size` are inode properties shared by every hardlink to one
//! physical jar, so two instances sharing a jar record identical values in their
//! own files. An in-place corruption through any link changes the inode's mtime,
//! every instance's memo misses, and every instance re-hashes. The failure
//! direction is "hash it again", never "trust a stale digest".
//!
//! ## Fallback direction
//!
//! An absent, unreadable, unparseable, or foreign-version file is an EMPTY memo
//! — which is exactly today's behaviour (hash everything). "Absent" and "could
//! not read" are deliberately not discriminated: they resolve to the same
//! restrictive answer, and there is no third, less safe branch for a
//! discrimination to protect. `verify` never consults this memo — it hashes
//! bytes unconditionally — so a stale entry here can never turn a corrupt
//! artefact green there.
//!
//! ## The residual risk, named
//!
//! `(mtime, size)` is not a proof of identity. A jar replaced by a DIFFERENT jar
//! of the same byte length, within the filesystem's mtime resolution, would hit
//! with a stale digest. That risk already exists in the in-memory `HASH_CACHE`;
//! persisting widens the window from one session to across restarts. Accepted,
//! because the consequence is bounded: the Installed list would show a stale
//! sha1 for that one jar until anything touches it, while `verify` — which reads
//! bytes — still reports the truth.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::fs;

const FILE_VERSION: u32 = 1;
const FILE_NAME: &str = "jar-hashes.json";

/// Unique per-write temp name, for the same reason `installed::write` carries
/// one: several `list()` calls reconcile the same instance concurrently (the
/// Installed view fires `modsListInstalled` + `modsPackOriginSummary` +
/// `mods_dependency_graph` together), and a shared fixed temp path made those
/// writes race on the same name.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// One file's identity stamp plus the digest computed from the bytes that
/// carried it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    /// Whole seconds of the mtime since the Unix epoch.
    pub mtime_secs: u64,
    /// Sub-second part of the mtime, in nanoseconds. Stored SEPARATELY from
    /// `mtime_secs` so the value round-trips losslessly through ordinary JSON
    /// numbers. A single float of nanoseconds would silently round away the
    /// 100 ns ticks NTFS actually reports, and a rounded mtime is a memo that
    /// answers "unchanged" for a changed file.
    pub mtime_nanos: u32,
    pub size: u64,
    /// Lowercase hex SHA-1 of the bytes that carried this stamp.
    pub sha1: String,
}

/// The stamp for a file with this `(mtime, size)` and known digest, or `None`
/// when the platform cannot report a usable modification time. `None` means
/// "record nothing", which means "hash it next time" — the restrictive answer.
pub fn stamp_of(mtime: SystemTime, size: u64, sha1: &str) -> Option<Stamp> {
    let d = mtime.duration_since(UNIX_EPOCH).ok()?;
    Some(Stamp {
        mtime_secs: d.as_secs(),
        mtime_nanos: d.subsec_nanos(),
        size,
        sha1: sha1.to_ascii_lowercase(),
    })
}

/// Whether `stamp` still describes a file with this `(mtime, size)`.
///
/// A pre-epoch or otherwise unrepresentable mtime is not something we can
/// compare, so it is NOT a match and the caller re-hashes.
pub fn matches(stamp: &Stamp, mtime: SystemTime, size: u64) -> bool {
    match mtime.duration_since(UNIX_EPOCH) {
        Ok(d) => {
            stamp.size == size
                && stamp.mtime_secs == d.as_secs()
                && stamp.mtime_nanos == d.subsec_nanos()
        }
        Err(_) => false,
    }
}

/// The whole memo for one instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashMemo {
    #[serde(default = "default_version")]
    version: u32,
    /// Keyed by the file name exactly as `read_dir` yields it, INCLUDING any
    /// `.disabled` suffix. Renaming to disable does not change the bytes or the
    /// mtime, so a base-name key would also hit — but two files differing only
    /// by that suffix can legally coexist, and a key that can collide is worse
    /// than one extra hash after a toggle.
    #[serde(default)]
    entries: BTreeMap<String, Stamp>,
}

/// Hand-written rather than derived: `#[serde(default = ..)]` applies only when
/// DESERIALIZING, so a derived `Default` would stamp `version: 0` onto every
/// freshly-built memo and write that to disk.
impl Default for HashMemo {
    fn default() -> Self {
        Self {
            version: FILE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

fn default_version() -> u32 {
    FILE_VERSION
}

impl HashMemo {
    /// The digest recorded for `file_name`, but only if its stamp still
    /// describes a file with this `(mtime, size)`.
    pub fn get(&self, file_name: &str, mtime: SystemTime, size: u64) -> Option<&str> {
        let stamp = self.entries.get(file_name)?;
        matches(stamp, mtime, size).then(|| stamp.sha1.as_str())
    }

    pub fn insert(&mut self, file_name: &str, stamp: Stamp) {
        self.entries.insert(file_name.to_string(), stamp);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `{instance}/lucerna/jar-hashes.json`.
pub fn memo_path(instance_root: &Path) -> PathBuf {
    crate::mods::installed::registry_dir(instance_root).join(FILE_NAME)
}

/// Read the memo. An absent, unreadable, unparseable or foreign-version file is
/// an EMPTY memo — see the module doc for why those four are not discriminated.
pub async fn load(instance_root: &Path) -> HashMemo {
    let path = memo_path(instance_root);
    let Ok(bytes) = fs::read(&path).await else {
        return HashMemo::default();
    };
    let memo: HashMemo = serde_json::from_slice(&bytes).unwrap_or_default();
    if memo.version != FILE_VERSION {
        // A file from another schema tells us nothing we can trust about which
        // bytes produced which digest. Discard and re-hash; the next save
        // rewrites it at the current version.
        return HashMemo::default();
    }
    memo
}

/// Write the memo atomically, mirroring `installed::write`: a unique per-write
/// temp name so concurrent reconciles never collide on the temp path, then a
/// rename that serialises the visible result. Concurrent writers compute the
/// same content from the same directory listing, so last-writer-wins is benign.
///
/// Returns `io::Result` rather than `crate::error::Error` on purpose: the caller
/// LOGS a failure and carries on. A memo that could not be written costs the
/// next list a re-hash and nothing else, and must never fail a `list()`.
pub(crate) async fn save(instance_root: &Path, memo: &HashMemo) -> std::io::Result<()> {
    let dir = crate::mods::installed::registry_dir(instance_root);
    fs::create_dir_all(&dir).await?;
    let final_path = memo_path(instance_root);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(memo).map_err(std::io::Error::other)?;
    fs::write(&tmp, &bytes).await?;
    fs::rename(&tmp, &final_path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn at(secs: u64, nanos: u32) -> SystemTime {
        UNIX_EPOCH + Duration::new(secs, nanos)
    }

    #[test]
    fn a_matching_stamp_returns_the_recorded_digest() {
        let mut memo = HashMemo::default();
        memo.insert("sodium.jar", stamp_of(at(1000, 0), 42, "AABB").unwrap());
        assert_eq!(memo.get("sodium.jar", at(1000, 0), 42), Some("aabb"));
    }

    #[test]
    fn a_changed_size_or_mtime_is_not_a_match() {
        let mut memo = HashMemo::default();
        memo.insert("sodium.jar", stamp_of(at(1000, 0), 42, "aabb").unwrap());
        assert_eq!(memo.get("sodium.jar", at(1000, 0), 43), None, "size");
        assert_eq!(memo.get("sodium.jar", at(1001, 0), 42), None, "mtime");
        assert_eq!(memo.get("other.jar", at(1000, 0), 42), None, "name");
    }

    /// The decision this pins: the mtime is stored as `(secs, nanos)`, not as a
    /// single narrowed float. A file rewritten 100 ns later — well inside what
    /// NTFS reports and what an f64 of milliseconds would erase — must MISS.
    #[test]
    fn sub_second_mtime_precision_survives_and_a_100ns_change_misses() {
        let mut memo = HashMemo::default();
        memo.insert(
            "a.jar",
            stamp_of(at(1_700_000_000, 123_456_700), 9, "aa").unwrap(),
        );
        assert_eq!(
            memo.get("a.jar", at(1_700_000_000, 123_456_700), 9),
            Some("aa")
        );
        assert_eq!(
            memo.get("a.jar", at(1_700_000_000, 123_456_800), 9),
            None,
            "a 100 ns change must miss, not round to a hit"
        );
    }

    #[test]
    fn a_pre_epoch_mtime_is_never_a_match_and_is_never_recorded() {
        let before = UNIX_EPOCH - Duration::from_secs(1);
        assert!(stamp_of(before, 1, "aa").is_none(), "nothing to record");
        let mut memo = HashMemo::default();
        memo.insert("a.jar", stamp_of(at(10, 0), 1, "aa").unwrap());
        assert_eq!(memo.get("a.jar", before, 1), None, "and never a hit");
    }

    #[tokio::test]
    async fn save_then_load_round_trips_at_the_expected_path() {
        let td = TempDir::new().unwrap();
        let mut memo = HashMemo::default();
        memo.insert("sodium.jar", stamp_of(at(5, 7), 128, "deadbeef").unwrap());
        save(td.path(), &memo).await.unwrap();

        assert_eq!(
            memo_path(td.path()),
            td.path().join("lucerna").join("jar-hashes.json")
        );
        assert!(memo_path(td.path()).exists());
        assert_eq!(load(td.path()).await, memo);
    }

    #[tokio::test]
    async fn an_absent_or_corrupt_file_is_an_empty_memo_not_an_error() {
        let td = TempDir::new().unwrap();
        assert!(load(td.path()).await.is_empty(), "absent");

        tokio::fs::create_dir_all(memo_path(td.path()).parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(memo_path(td.path()), b"{ not json")
            .await
            .unwrap();
        assert!(load(td.path()).await.is_empty(), "corrupt");
    }

    #[tokio::test]
    async fn a_memo_from_another_schema_version_is_discarded() {
        let td = TempDir::new().unwrap();
        tokio::fs::create_dir_all(memo_path(td.path()).parent().unwrap())
            .await
            .unwrap();
        let foreign = br#"{"version":99,"entries":{"a.jar":{"mtime_secs":1,"mtime_nanos":0,"size":2,"sha1":"aa"}}}"#;
        tokio::fs::write(memo_path(td.path()), foreign).await.unwrap();
        assert!(
            load(td.path()).await.is_empty(),
            "a foreign version says nothing trustworthy about which bytes made which digest"
        );
    }

    /// A freshly-built memo must carry the CURRENT version, not `u32::default()`
    /// — the reason `Default` is hand-written rather than derived.
    #[tokio::test]
    async fn a_saved_memo_carries_the_current_schema_version() {
        let td = TempDir::new().unwrap();
        let mut memo = HashMemo::default();
        memo.insert("a.jar", stamp_of(at(1, 0), 1, "aa").unwrap());
        save(td.path(), &memo).await.unwrap();
        let raw = String::from_utf8(tokio::fs::read(memo_path(td.path())).await.unwrap()).unwrap();
        assert!(raw.contains("\"version\": 1"), "got {raw}");
        assert_eq!(load(td.path()).await.len(), 1, "and reloads, not discarded");
    }

    /// Mirrors `mods::store`'s residue test: an atomic write leaves no temp file.
    #[tokio::test]
    async fn save_leaves_no_temp_residue() {
        let td = TempDir::new().unwrap();
        save(td.path(), &HashMemo::default()).await.unwrap();
        let residue: Vec<String> = std::fs::read_dir(memo_path(td.path()).parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(residue.is_empty(), "temp residue: {residue:?}");
    }
}
