//! Impure side of client-mod quarantine: gather each jar's facts (offline
//! descriptor reads), apply a [`ClassifyResult`] to a server's `mods/` dir
//! (rename quarantined jars to `*.disabled` + record why in a sidecar), and the
//! dependency-safety guard that stops a removal/disable from stripping a mod
//! another *kept* mod requires.
//!
//! The decision logic lives in [`super::mod_classify`] (pure). This module only
//! does I/O around it.
//!
//! The sidecar is read-modify-written by two writers ([`apply_quarantine`]
//! inserts, [`forget_reason`] removes) on genuinely different threads:
//! `server_quarantine_client_mods` is an async command that hashes every jar
//! and makes two Modrinth round-trips before it gets here, while
//! `server_enable_mod` is a synchronous command running on the main thread, and
//! the UI gates neither against the other. So every access goes through
//! [`lock_sidecar`], whose guard OWNS the read and the write — an unlocked
//! read-modify-write does not compile.

use crate::error::{Error, Result};
use crate::servers_runtime::mod_classify::{
    classify_server_mods, norm_id, ClassifyResult, ModFacts, ServerSideSupport,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Sidecar in the server's `mods/` recording why each disabled jar was set
/// aside, so the UI can label it. Cosmetic; safe to delete; tolerant of absence.
pub const SIDECAR: &str = ".lucerna-quarantine.json";
/// Reason value for an auto-quarantined client-only mod.
pub const REASON_CLIENT_ONLY: &str = "client_only";

/// Read every enabled `.jar` in `mods_dir` into [`ModFacts`], attaching
/// `server_side` from `server_side_by_filename` (defaulting to `Unknown`).
/// `.jar.disabled` files are skipped (already set aside). Best-effort: an
/// unreadable jar is omitted, a missing dir yields an empty vec.
pub fn gather_facts(
    mods_dir: &Path,
    server_side_by_filename: &HashMap<String, ServerSideSupport>,
) -> Vec<ModFacts> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.to_ascii_lowercase().ends_with(".jar") {
            continue;
        }
        let Ok(bytes) = std::fs::read(e.path()) else {
            continue;
        };
        let env = crate::mods::local::read_jar_environment(&bytes);
        let mut provides: Vec<String> = crate::mods::local::read_jar_manifest_deps(&bytes)
            .map(|m| m.provided.into_iter().map(|p| p.mod_id).collect())
            .unwrap_or_default();
        for p in crate::mods::local::read_jar_embedded_providers(&bytes) {
            provides.push(p.mod_id);
        }
        let required_deps = crate::mods::local::read_jar_dependency_ids(&bytes).unwrap_or_default();
        let server_side = server_side_by_filename
            .get(&name)
            .copied()
            .unwrap_or(ServerSideSupport::Unknown);
        out.push(ModFacts {
            filename: name,
            env,
            server_side,
            provides,
            required_deps,
        });
    }
    out
}

/// Rename each quarantined `<name>.jar` → `<name>.jar.disabled` and record the
/// reason in the sidecar. Returns the disabled filenames (with `.disabled`).
/// Idempotent: an already-disabled / absent jar is skipped without error.
/// Path-safe: unsafe names and path-escapes are skipped.
/// Refuses outright when the sidecar cannot be read: setting a mod aside while
/// unable to record why leaves the user with a disabled jar and no explanation
/// beside it, so the restrictive answer is to change nothing.
pub fn apply_quarantine(mods_dir: &Path, result: &ClassifyResult) -> Result<Vec<String>> {
    let sidecar = lock_sidecar(mods_dir);
    // Read BEFORE the first rename, so the refusal above costs nothing.
    let mut reasons = sidecar.read()?;
    let mut disabled = Vec::new();
    for filename in &result.quarantine {
        if !crate::servers_runtime::runtime::is_safe_mod_name(filename) {
            continue;
        }
        let src = mods_dir.join(filename);
        let disabled_name = format!("{filename}.disabled");
        let dst = mods_dir.join(&disabled_name);
        if !src.starts_with(mods_dir) || !dst.starts_with(mods_dir) {
            continue;
        }
        match std::fs::rename(&src, &dst) {
            Ok(()) => {
                reasons.insert(disabled_name.clone(), REASON_CLIENT_ONLY.to_string());
                disabled.push(disabled_name);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(dst.display().to_string(), e)),
        }
    }
    if !disabled.is_empty() {
        sidecar.write(&reasons)?;
    }
    Ok(disabled)
}

/// End-to-end client-mod quarantine for one server's `mods/` dir, given a
/// `filename -> server_side` map (built by the command layer from platform
/// metadata). Returns `(disabled, classification)`. Pure-of-network.
pub fn quarantine_with_metadata(
    mods_dir: &Path,
    server_side_by_filename: &HashMap<String, ServerSideSupport>,
) -> Result<(Vec<String>, ClassifyResult)> {
    let facts = gather_facts(mods_dir, server_side_by_filename);
    let result = classify_server_mods(&facts);
    let disabled = apply_quarantine(mods_dir, &result)?;
    Ok((disabled, result))
}

/// Would removing/disabling `targets` strip a mod that a *remaining* enabled mod
/// requires? Returns `Some((target, required_by))` for the first such conflict,
/// `None` when the removal is dependency-safe. Reuses [`gather_facts`].
pub fn first_required_conflict(mods_dir: &Path, targets: &[String]) -> Option<(String, String)> {
    let facts = gather_facts(mods_dir, &HashMap::new());
    let target_set: HashSet<&str> = targets.iter().map(|s| s.as_str()).collect();
    // Normalized dep-id -> a remaining (stayer) mod filename that requires it.
    let mut needed: HashMap<String, String> = HashMap::new();
    for f in &facts {
        if target_set.contains(f.filename.as_str()) {
            continue; // a target is leaving — its needs don't constrain removal
        }
        for d in &f.required_deps {
            needed
                .entry(norm_id(d))
                .or_insert_with(|| f.filename.clone());
        }
    }
    for f in &facts {
        if !target_set.contains(f.filename.as_str()) {
            continue;
        }
        for p in &f.provides {
            if let Some(req_by) = needed.get(&norm_id(p)) {
                return Some((f.filename.clone(), req_by.clone()));
            }
        }
    }
    None
}

/// Read the `disabled-filename -> reason` sidecar map for a server's `mods/`.
/// The command layer uses this to label disabled rows ("set aside:
/// client-only") instead of guessing from the `.disabled` suffix alone.
///
/// Infallible, and the ONE place in this module where an unreadable sidecar is
/// allowed to answer "empty": the result is rendered as a badge and never
/// written back, and for a label "say nothing" IS the restrictive answer. The
/// failure is logged rather than swallowed.
pub fn read_reasons(mods_dir: &Path) -> BTreeMap<String, String> {
    let sidecar = lock_sidecar(mods_dir);
    sidecar.read().unwrap_or_else(|e| {
        crate::diag!("servers: quarantine reasons unreadable, labelling none: {e}");
        BTreeMap::new()
    })
}

/// Drop a disabled jar's sidecar reason (e.g. after re-enabling it). A no-op
/// when the entry isn't present.
///
/// Fallible on purpose. The caller (`server_enable_mod`) runs this AFTER a
/// rename that already succeeded, so it must not turn the failure into a failed
/// enable — but it must not pretend it worked either. Returning the error lets
/// the caller log the truth.
pub fn forget_reason(mods_dir: &Path, disabled_filename: &str) -> Result<()> {
    let sidecar = lock_sidecar(mods_dir);
    let mut reasons = sidecar.read()?;
    if reasons.remove(disabled_filename).is_none() {
        return Ok(());
    }
    sidecar.write(&reasons)
}

fn sidecar_path(mods_dir: &Path) -> PathBuf {
    mods_dir.join(SIDECAR)
}

/// Process-lifetime write counter, giving each write a unique temp name.
/// Mirrors `servers_runtime::installed::save`.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// The mutex guarding one sidecar FILE, created on first use and kept for the
/// process lifetime.
///
/// Keyed by the sidecar's path, not by the mods dir: `.lucerna-quarantine.json`
/// and the neighbouring `.lucerna-installed.json` have disjoint writers, and a
/// directory key would serialise unrelated work and create a lock-order pair to
/// reason about. Same choice as `mods::registry_lock` ("one mutex per registry
/// FILE").
///
/// Deliberately a plain mutex with no `Condvar`: a keyed lock built on
/// `Condvar::wait_while` hands the key to two holders once the inner mutex is
/// poisoned, because `wait_while` returns on poison WITHOUT re-checking its
/// predicate. There is no such edge here.
///
/// Process-local is enough — `tauri-plugin-single-instance` means one launcher
/// process at a time.
fn sidecar_mutex(path: &Path) -> &'static Mutex<()> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, &'static Mutex<()>>>> = OnceLock::new();
    let mut map = LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Deref-copies the `&'static` out of the entry, so nothing borrowed from
    // `map` (a guard on a local) escapes this function.
    *map.entry(path.to_path_buf()).or_insert_with(|| {
        // Leaked so the guard can be `'static`. Bounded by the number of
        // distinct server mods dirs seen in the process — the same growth the
        // map entry itself already costs.
        let m: &'static Mutex<()> = Box::leak(Box::new(Mutex::new(())));
        m
    })
}

/// Exclusive access to one server's quarantine sidecar.
///
/// Holds the read and the write as METHODS, so a read-modify-write without the
/// lock is not expressible. Not re-entrant: a holder must never call another
/// function that locks the same sidecar. The three holders —
/// [`apply_quarantine`], [`forget_reason`] and [`read_reasons`] — are all in
/// this file and none calls another.
struct SidecarGuard {
    path: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

fn lock_sidecar(mods_dir: &Path) -> SidecarGuard {
    let path = sidecar_path(mods_dir);
    let lock = sidecar_mutex(&path)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    SidecarGuard { path, _lock: lock }
}

impl SidecarGuard {
    /// Read the map, telling "absent" apart from "could not read".
    ///
    /// Absent is a fact: a mods dir that never had a quarantine holds no
    /// sidecar, and "no reasons" is the true answer. Any other read failure is
    /// ignorance, and both write paths feed this straight into a whole-file
    /// write — so answering "empty" would persist the loss of every reason.
    ///
    /// A PARSE failure on bytes that read fine stays fail-open, the same
    /// deliberate asymmetry `servers_runtime::installed::load` documents: the
    /// file is provably corrupt and holds no recoverable reasons, and erroring
    /// would wedge quarantine forever on one bad byte. It is logged, never
    /// silent.
    fn read(&self) -> Result<BTreeMap<String, String>> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(Error::io(self.path.display().to_string(), e)),
        };
        match serde_json::from_slice(&bytes) {
            Ok(map) => Ok(map),
            Err(e) => {
                crate::diag!(
                    "servers: quarantine sidecar {} is corrupt, starting empty: {e}",
                    self.path.display()
                );
                Ok(BTreeMap::new())
            }
        }
    }

    /// Write via temp-then-rename, so a crash or power loss mid-write cannot
    /// leave a truncated map for the next read to inherit and re-persist.
    fn write(&self, map: &BTreeMap<String, String>) -> Result<()> {
        let json =
            serde_json::to_vec_pretty(map).expect("BTreeMap<String,String> always serializes");
        let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = self
            .path
            .with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
        std::fs::write(&tmp, &json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            // The rename IS the commit, and it failed — the temp is now garbage
            // that would otherwise sit beside the jars forever. This is cleanup
            // on an already-failed path: a failed removal only leaves a stray
            // file, while the error that matters is the one returned below.
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::io(self.path.display().to_string(), e));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    /// Build an in-memory `.jar` (zip) from (name, bytes) entries.
    fn jar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    /// A Fabric jar declaring id + environment + (optional) depends.
    fn fabric_jar(id: &str, env: &str, depends: &[&str]) -> Vec<u8> {
        let deps = depends
            .iter()
            .map(|d| format!("\"{d}\":\"*\""))
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(r#"{{"id":"{id}","environment":"{env}","depends":{{{deps}}}}}"#);
        jar(&[("fabric.mod.json", json.as_bytes())])
    }

    /// A Forge jar declaring [[mods]] modId + a required dependency.
    fn forge_jar(id: &str, required: &[&str]) -> Vec<u8> {
        let mut toml = format!("[[mods]]\nmodId=\"{id}\"\n");
        for r in required {
            toml.push_str(&format!(
                "[[dependencies.{id}]]\nmodId=\"{r}\"\nmandatory=true\nversionRange=\"[1,)\"\n"
            ));
        }
        jar(&[("META-INF/mods.toml", toml.as_bytes())])
    }

    fn write_jar(dir: &Path, name: &str, bytes: &[u8]) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn gather_facts_reads_env_provides_and_deps() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "modmenu.jar", &fabric_jar("modmenu", "client", &[]));
        write_jar(
            dir,
            "bettervillage.jar",
            &forge_jar("bettervillage", &["libraryferret"]),
        );
        // a .disabled jar must be ignored by gather_facts
        write_jar(dir, "old.jar.disabled", &fabric_jar("old", "client", &[]));

        let facts = gather_facts(dir, &HashMap::new());
        assert_eq!(facts.len(), 2, "only enabled jars: {facts:?}");
        let mm = facts.iter().find(|f| f.filename == "modmenu.jar").unwrap();
        assert_eq!(mm.env, crate::mods::local::ModEnvironment::Client);
        assert!(mm.provides.iter().any(|p| p == "modmenu"));
        let bv = facts
            .iter()
            .find(|f| f.filename == "bettervillage.jar")
            .unwrap();
        assert!(bv.required_deps.iter().any(|d| d == "libraryferret"));
    }

    #[test]
    fn apply_quarantine_renames_and_writes_sidecar() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "betterf3.jar", b"x");
        write_jar(dir, "jei.jar", b"y");
        let result = ClassifyResult {
            quarantine: vec!["betterf3.jar".into()],
            kept: vec!["jei.jar".into()],
            kept_because_required: vec![],
        };
        let disabled = apply_quarantine(dir, &result).unwrap();
        assert_eq!(disabled, vec!["betterf3.jar.disabled".to_string()]);
        assert!(!dir.join("betterf3.jar").exists());
        assert!(dir.join("betterf3.jar.disabled").exists());
        assert!(dir.join("jei.jar").exists(), "kept jar untouched");
        let sidecar = read_sidecar(dir);
        assert_eq!(
            sidecar.get("betterf3.jar.disabled").map(String::as_str),
            Some(REASON_CLIENT_ONLY)
        );
    }

    #[test]
    fn apply_quarantine_is_idempotent_on_missing() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        // Quarantine names a jar that isn't there → skipped, no error, no sidecar.
        let result = ClassifyResult {
            quarantine: vec!["ghost.jar".into()],
            kept: vec![],
            kept_because_required: vec![],
        };
        let disabled = apply_quarantine(dir, &result).unwrap();
        assert!(disabled.is_empty());
        assert!(!dir.join(SIDECAR).exists());
    }

    #[test]
    fn read_reasons_surfaces_sidecar_after_quarantine() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "betterf3.jar", b"x");
        let result = ClassifyResult {
            quarantine: vec!["betterf3.jar".into()],
            kept: vec![],
            kept_because_required: vec![],
        };
        apply_quarantine(dir, &result).unwrap();
        let reasons = read_reasons(dir);
        assert_eq!(
            reasons.get("betterf3.jar.disabled").map(String::as_str),
            Some(REASON_CLIENT_ONLY)
        );
        // A dir with no sidecar yields an empty map (no panic).
        let empty = tempfile::tempdir().unwrap();
        assert!(read_reasons(empty.path()).is_empty());
    }

    /// Bytes that READ fine but do not parse are a proven fact: the file is
    /// corrupt and holds no recoverable reasons. Unlike an unreadable sidecar
    /// this stays fail-open on purpose — erroring would wedge quarantine
    /// forever on one bad byte. The deliberate asymmetry, pinned.
    #[test]
    fn apply_quarantine_tolerates_a_corrupt_sidecar() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "betterf3.jar", b"x");
        std::fs::write(dir.join(SIDECAR), b"{ not json at all").unwrap();

        let result = ClassifyResult {
            quarantine: vec!["betterf3.jar".into()],
            kept: vec![],
            kept_because_required: vec![],
        };
        let disabled = apply_quarantine(dir, &result).expect("corrupt is recoverable, not fatal");

        assert_eq!(disabled, vec!["betterf3.jar.disabled".to_string()]);
        assert_eq!(
            read_reasons(dir)
                .get("betterf3.jar.disabled")
                .map(String::as_str),
            Some(REASON_CLIENT_ONLY),
            "the corrupt map is replaced, not inherited"
        );
    }

    /// `read_reasons` is the one label-only caller, so an unreadable sidecar
    /// answers "no reasons" there — for a badge, saying nothing IS the
    /// restrictive answer, and it writes nothing back. Pinned so a later
    /// "make it consistent" change has to argue with a test.
    #[test]
    fn read_reasons_is_empty_when_the_sidecar_cannot_be_read() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        std::fs::create_dir(dir.join(SIDECAR)).unwrap();

        assert!(read_reasons(dir).is_empty());
    }

    /// The write paths do NOT get that latitude: `forget_reason` reports the
    /// failure so `server_enable_mod` can log the truth instead of pretending
    /// the reason was dropped.
    #[test]
    fn forget_reason_reports_an_unreadable_sidecar() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        std::fs::create_dir(dir.join(SIDECAR)).unwrap();

        assert!(forget_reason(dir, "betterf3.jar.disabled").is_err());
    }

    #[test]
    fn forget_reason_drops_only_its_own_entry() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "betterf3.jar", b"x");
        write_jar(dir, "modmenu.jar", b"y");
        let result = ClassifyResult {
            quarantine: vec!["betterf3.jar".into(), "modmenu.jar".into()],
            kept: vec![],
            kept_because_required: vec![],
        };
        apply_quarantine(dir, &result).unwrap();

        forget_reason(dir, "betterf3.jar.disabled").unwrap();

        let reasons = read_reasons(dir);
        assert!(!reasons.contains_key("betterf3.jar.disabled"));
        assert_eq!(
            reasons.get("modmenu.jar.disabled").map(String::as_str),
            Some(REASON_CLIENT_ONLY),
            "the neighbour's reason survives"
        );
        // Forgetting something that isn't there is a no-op, not an error.
        forget_reason(dir, "ghost.jar.disabled").unwrap();
    }

    /// The write commits through a temp file and a rename. Pins the mechanism:
    /// a temp that is written but never renamed (or never cleaned up) would
    /// leave residue beside the jars.
    ///
    /// Crash-safety itself is not unit-testable without fault injection — this
    /// asserts the shape, not the power-loss guarantee.
    #[test]
    fn a_committed_write_leaves_no_temp_file_behind() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "betterf3.jar", b"x");
        let result = ClassifyResult {
            quarantine: vec!["betterf3.jar".into()],
            kept: vec![],
            kept_because_required: vec![],
        };
        apply_quarantine(dir, &result).unwrap();
        forget_reason(dir, "betterf3.jar.disabled").unwrap();

        let residue: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(residue.is_empty(), "temp files left behind: {residue:?}");
    }

    /// Both writers at once on one map: half the threads insert their own jar's
    /// reason, the other half forget a pre-seeded one. Every insert must survive
    /// and every forget must stick — an interleaved read-modify-write loses one
    /// or resurrects the other.
    #[test]
    fn concurrent_quarantine_and_forget_never_clobber_each_other() {
        const PAIRS: usize = 8;

        let td = tempfile::tempdir().unwrap();
        let dir = td.path().to_path_buf();
        for i in 0..PAIRS {
            write_jar(&dir, &format!("new{i}.jar"), b"x");
            write_jar(&dir, &format!("old{i}.jar"), b"y");
        }
        // Seed the `old*` reasons so the forgetting threads have something real
        // to remove.
        let seed = ClassifyResult {
            quarantine: (0..PAIRS).map(|i| format!("old{i}.jar")).collect(),
            kept: vec![],
            kept_because_required: vec![],
        };
        apply_quarantine(&dir, &seed).unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(PAIRS * 2));
        let mut handles = Vec::new();
        for i in 0..PAIRS {
            for insert in [true, false] {
                let dir = dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                handles.push(std::thread::spawn(move || {
                    barrier.wait();
                    if insert {
                        let result = ClassifyResult {
                            quarantine: vec![format!("new{i}.jar")],
                            kept: vec![],
                            kept_because_required: vec![],
                        };
                        apply_quarantine(&dir, &result).map(|_| ())
                    } else {
                        forget_reason(&dir, &format!("old{i}.jar.disabled"))
                    }
                }));
            }
        }
        for h in handles {
            h.join().expect("writer panicked").expect("write failed");
        }

        let reasons = read_reasons(&dir);
        let lost: Vec<String> = (0..PAIRS)
            .map(|i| format!("new{i}.jar.disabled"))
            .filter(|n| !reasons.contains_key(n))
            .collect();
        let resurrected: Vec<String> = (0..PAIRS)
            .map(|i| format!("old{i}.jar.disabled"))
            .filter(|n| reasons.contains_key(n))
            .collect();
        assert!(lost.is_empty(), "inserts lost: {lost:?}");
        assert!(
            resurrected.is_empty(),
            "forgotten reasons came back: {resurrected:?}"
        );
    }

    #[test]
    fn first_required_conflict_blocks_removing_a_dependency() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(
            dir,
            "bettervillage.jar",
            &forge_jar("bettervillage", &["libraryferret"]),
        );
        write_jar(dir, "libraryferret.jar", &forge_jar("libraryferret", &[]));
        // Removing libraryferret while bettervillage stays → conflict.
        let c = first_required_conflict(dir, &["libraryferret.jar".to_string()]);
        assert_eq!(
            c,
            Some(("libraryferret.jar".into(), "bettervillage.jar".into()))
        );
    }

    /// A sidecar that cannot be READ is ignorance, not a fact. `apply_quarantine`
    /// feeds the read straight into a whole-file write, so fail-opening to an
    /// empty map erases every stored reason — and, worse, sets mods aside while
    /// unable to record why. The restrictive direction is: change nothing.
    ///
    /// The unreadable sidecar is a DIRECTORY at the sidecar's path. That is
    /// portable (Linux `IsADirectory`, Windows/macOS access denied) and is
    /// definitively not `NotFound`, which is the one error that IS a fact.
    ///
    /// The `is_err()` alone proves nothing here — today's code also ends in an
    /// `Err` when its write hits the directory. The jar still standing is the
    /// assertion that carries the defect.
    #[test]
    fn apply_quarantine_refuses_when_the_sidecar_cannot_be_read() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "betterf3.jar", b"x");
        std::fs::create_dir(dir.join(SIDECAR)).unwrap();

        let result = ClassifyResult {
            quarantine: vec!["betterf3.jar".into()],
            kept: vec![],
            kept_because_required: vec![],
        };
        let outcome = apply_quarantine(dir, &result);

        assert!(
            outcome.is_err(),
            "an unreadable sidecar must not read as empty"
        );
        assert!(
            dir.join("betterf3.jar").exists(),
            "nothing may be set aside when the reason cannot be recorded"
        );
        assert!(
            !dir.join("betterf3.jar.disabled").exists(),
            "the jar must not be renamed on the refusal path"
        );
    }

    /// Read-modify-write on one shared map from several threads. Every writer
    /// owns a distinct jar, so the only way to lose an entry is an interleave:
    /// two writers read the same map and the later write clobbers the earlier.
    ///
    /// Concurrency is real here — `server_quarantine_client_mods` is an async
    /// command that hashes every jar and makes two Modrinth round-trips before
    /// reaching `apply_quarantine`, while `server_enable_mod` is a sync command
    /// on the main thread. The UI gates neither against the other.
    #[test]
    fn concurrent_quarantine_keeps_every_reason() {
        const WRITERS: usize = 16;

        let td = tempfile::tempdir().unwrap();
        let dir = td.path().to_path_buf();
        for i in 0..WRITERS {
            write_jar(&dir, &format!("m{i}.jar"), b"x");
        }

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));
        let mut handles = Vec::new();
        for i in 0..WRITERS {
            let dir = dir.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let result = ClassifyResult {
                    quarantine: vec![format!("m{i}.jar")],
                    kept: vec![],
                    kept_because_required: vec![],
                };
                barrier.wait();
                apply_quarantine(&dir, &result)
            }));
        }
        for h in handles {
            h.join().expect("writer panicked").expect("write failed");
        }

        let reasons = read_reasons(&dir);
        let missing: Vec<String> = (0..WRITERS)
            .map(|i| format!("m{i}.jar.disabled"))
            .filter(|name| !reasons.contains_key(name))
            .collect();
        assert!(
            missing.is_empty(),
            "{} of {WRITERS} reasons lost to an interleaved read-modify-write: {missing:?}",
            missing.len()
        );
    }

    #[test]
    fn first_required_conflict_allows_removing_a_leaf() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(dir, "betterf3.jar", &fabric_jar("betterf3", "client", &[]));
        write_jar(
            dir,
            "bettervillage.jar",
            &forge_jar("bettervillage", &["libraryferret"]),
        );
        write_jar(dir, "libraryferret.jar", &forge_jar("libraryferret", &[]));
        // betterf3 is a leaf nobody depends on → safe.
        assert!(first_required_conflict(dir, &["betterf3.jar".to_string()]).is_none());
    }

    #[test]
    fn first_required_conflict_allows_removing_dependency_with_its_dependent() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path();
        write_jar(
            dir,
            "bettervillage.jar",
            &forge_jar("bettervillage", &["libraryferret"]),
        );
        write_jar(dir, "libraryferret.jar", &forge_jar("libraryferret", &[]));
        // Removing BOTH the dependent and its dependency together → safe.
        let targets = vec![
            "bettervillage.jar".to_string(),
            "libraryferret.jar".to_string(),
        ];
        assert!(first_required_conflict(dir, &targets).is_none());
    }
}
