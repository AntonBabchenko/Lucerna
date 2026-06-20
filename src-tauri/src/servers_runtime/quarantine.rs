//! Impure side of client-mod quarantine: gather each jar's facts (offline
//! descriptor reads), apply a [`ClassifyResult`] to a server's `mods/` dir
//! (rename quarantined jars to `*.disabled` + record why in a sidecar), and the
//! dependency-safety guard that stops a removal/disable from stripping a mod
//! another *kept* mod requires.
//!
//! The decision logic lives in [`super::mod_classify`] (pure). This module only
//! does I/O around it.

use crate::error::{Error, Result};
use crate::servers_runtime::mod_classify::{
    classify_server_mods, norm_id, ClassifyResult, ModFacts, ServerSideSupport,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

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
pub fn apply_quarantine(mods_dir: &Path, result: &ClassifyResult) -> Result<Vec<String>> {
    let mut disabled = Vec::new();
    let mut sidecar = read_sidecar(mods_dir);
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
                sidecar.insert(disabled_name.clone(), REASON_CLIENT_ONLY.to_string());
                disabled.push(disabled_name);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(dst.display().to_string(), e)),
        }
    }
    if !disabled.is_empty() {
        write_sidecar(mods_dir, &sidecar)?;
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
/// Empty when the sidecar is absent/unreadable. The command layer uses this to
/// label disabled rows ("set aside: client-only") instead of guessing from the
/// `.disabled` suffix alone.
pub fn read_reasons(mods_dir: &Path) -> BTreeMap<String, String> {
    read_sidecar(mods_dir)
}

/// Drop a disabled jar's sidecar reason (e.g. after re-enabling it). Best-effort
/// and non-fatal: a missing sidecar or a write failure is silently ignored —
/// the sidecar is cosmetic. A no-op when the entry isn't present.
pub fn forget_reason(mods_dir: &Path, disabled_filename: &str) {
    let mut sidecar = read_sidecar(mods_dir);
    if sidecar.remove(disabled_filename).is_some() {
        let _ = write_sidecar(mods_dir, &sidecar);
    }
}

fn sidecar_path(mods_dir: &Path) -> std::path::PathBuf {
    mods_dir.join(SIDECAR)
}

fn read_sidecar(mods_dir: &Path) -> BTreeMap<String, String> {
    std::fs::read(sidecar_path(mods_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_sidecar(mods_dir: &Path, map: &BTreeMap<String, String>) -> Result<()> {
    let path = sidecar_path(mods_dir);
    let json =
        serde_json::to_vec_pretty(map).expect("BTreeMap<String,String> always serializes to JSON");
    std::fs::write(&path, json).map_err(|e| Error::io(path.display().to_string(), e))
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
