//! Local mod-jar install: read a `.jar`'s mod descriptor, judge
//! loader/MC compatibility with an instance, and install it into
//! `{instance}/.minecraft/mods/` as a manual (`source: None`) mod.

use std::io::{Cursor, Read};
use std::path::Path;

use chrono::Utc;
use sha1::{Digest, Sha1};
use tokio::fs;

use crate::error::Error;
use crate::mods::compat::ModLocalCompat;
use crate::mods::installed;
use crate::mods::platform::{InstalledMod, LoaderKind};

/// Which loader family a mod jar targets, detected from its descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderFamily {
    /// Fabric or Quilt.
    Fabric,
    /// Forge or NeoForge (including legacy Forge).
    Forge,
}

/// Best-effort metadata read from a mod `.jar`.
#[derive(Debug, Clone, Default)]
pub struct JarMeta {
    /// Loader family from the descriptor, or `None` when the jar has no
    /// recognised descriptor (coremod / library — undeterminable).
    pub family: Option<LoaderFamily>,
    /// Display loader name — "Fabric" / "Quilt" / "Forge" / "NeoForge".
    pub loader_label: Option<String>,
    /// Declared Minecraft version reduced to `major.minor` (e.g. "1.12"),
    /// or `None` when not declared / not parseable.
    pub mc_version: Option<String>,
    /// The mod's display name from the descriptor, or `None`.
    pub display_name: Option<String>,
}

/// Extract the first `major.minor` (e.g. "1.20") substring from `s`.
/// Returns `None` when there is no `<digits>.<digits>` run (a bare `*`,
/// a snapshot id like "21w13a", an empty string).
fn first_major_minor(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i < b.len() && b[i] == b'.' {
            let mid = i + 1;
            let mut j = mid;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > mid {
                return Some(s[start..j].to_string());
            }
            // a dot with no digits after it — step past the dot
            i += 1;
        }
        // not a major.minor pattern — continue from the current position
    }
    None
}

/// Read a zip entry's text contents, or `None` if absent / unreadable.
fn entry_text(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// `fabric.mod.json` `depends.minecraft` is a string or an array of
/// strings — pull the first string out.
fn json_first_string(v: &serde_json::Value) -> Option<&str> {
    match v {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Array(a) => a.iter().find_map(|x| x.as_str()),
        _ => None,
    }
}

/// Read a mod jar's descriptor. Best-effort: a jar with no recognised
/// descriptor yields an all-`None` `JarMeta` (not an error). Only an
/// unreadable zip is an error.
pub fn read_jar_meta(jar_bytes: &[u8]) -> Result<JarMeta, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar_bytes)).map_err(|e| Error::ModsDecode {
        platform: "local jar".into(),
        details: e.to_string(),
    })?;

    // Loader family + label by descriptor presence, priority order.
    let (family, label): (Option<LoaderFamily>, Option<&str>) =
        if zip.by_name("quilt.mod.json").is_ok() {
            (Some(LoaderFamily::Fabric), Some("Quilt"))
        } else if zip.by_name("fabric.mod.json").is_ok() {
            (Some(LoaderFamily::Fabric), Some("Fabric"))
        } else if zip.by_name("META-INF/neoforge.mods.toml").is_ok() {
            (Some(LoaderFamily::Forge), Some("NeoForge"))
        } else if zip.by_name("META-INF/mods.toml").is_ok() {
            (Some(LoaderFamily::Forge), Some("Forge"))
        } else if zip.by_name("mcmod.info").is_ok() {
            (Some(LoaderFamily::Forge), Some("Forge"))
        } else {
            (None, None)
        };

    // Best-effort MC version + display name — only from the simple JSON
    // descriptors (`fabric.mod.json`, legacy `mcmod.info`). `mods.toml` /
    // `quilt.mod.json` are not parsed for content in v1.
    let mut mc_version = None;
    let mut display_name = None;
    if let Some(txt) = entry_text(&mut zip, "fabric.mod.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            display_name = v.get("name").and_then(|x| x.as_str()).map(String::from);
            mc_version = v
                .get("depends")
                .and_then(|d| d.get("minecraft"))
                .and_then(json_first_string)
                .and_then(first_major_minor);
        }
    } else if let Some(txt) = entry_text(&mut zip, "mcmod.info") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            // mcmod.info is a JSON array of mod objects, or { modList: [...] }.
            let first = v.as_array().and_then(|a| a.first()).or_else(|| {
                v.get("modList")
                    .and_then(|m| m.as_array())
                    .and_then(|a| a.first())
            });
            if let Some(m) = first {
                display_name = m.get("name").and_then(|x| x.as_str()).map(String::from);
                mc_version = m
                    .get("mcversion")
                    .and_then(|x| x.as_str())
                    .and_then(first_major_minor);
            }
        }
    }

    Ok(JarMeta {
        family,
        loader_label: label.map(String::from),
        mc_version,
        display_name,
    })
}

/// Compatibility verdict for a local mod jar against a target instance.
/// Crosses the IPC boundary. A jar is "compatible" iff neither flag is set.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CompatVerdict {
    /// Display loader name detected in the jar ("Fabric" / "Forge" / …),
    /// or `None` when the jar has no recognised descriptor.
    pub detected_loader: Option<String>,
    /// `major.minor` Minecraft version detected in the jar, or `None`.
    pub detected_mc: Option<String>,
    /// The mod's display name from the jar, or `None`.
    pub detected_name: Option<String>,
    /// The jar's loader family is the opposite of the instance's.
    pub loader_mismatch: bool,
    /// The jar's declared MC `major.minor` differs from the instance's.
    pub mc_mismatch: bool,
}

/// Map an instance `LoaderKind` to a loader family. `Vanilla` has no
/// family — callers must not invoke `compat_verdict` for a vanilla
/// instance (the UI disables the dropzone there).
fn instance_family(loader: LoaderKind) -> Option<LoaderFamily> {
    match loader {
        LoaderKind::Fabric | LoaderKind::Quilt => Some(LoaderFamily::Fabric),
        LoaderKind::Forge | LoaderKind::NeoForge => Some(LoaderFamily::Forge),
        LoaderKind::Vanilla => None,
    }
}

/// Judge a jar's loader/MC compatibility with an instance. Conservative:
/// a mismatch is reported only when both sides are confidently known and
/// they differ — absent or ambiguous metadata never produces a warning.
pub fn compat_verdict(
    jar: &JarMeta,
    instance_loader: LoaderKind,
    instance_mc: &str,
) -> CompatVerdict {
    let loader_mismatch = match (jar.family, instance_family(instance_loader)) {
        (Some(jf), Some(inf)) => jf != inf,
        _ => false,
    };
    let mc_mismatch = match (jar.mc_version.as_deref(), first_major_minor(instance_mc)) {
        (Some(jmc), Some(imc)) => jmc != imc,
        _ => false,
    };
    CompatVerdict {
        detected_loader: jar.loader_label.clone(),
        detected_mc: jar.mc_version.clone(),
        detected_name: jar.display_name.clone(),
        loader_mismatch,
        mc_mismatch,
    }
}

/// Offline loader-family compatibility scan of an instance's installed mods.
/// Layer 1 of the proactive incompatibility check: for each registered mod,
/// read its jar's descriptor and judge loader family against the instance.
/// Network-free. A mod whose jar is missing/unreadable, or has no recognised
/// descriptor, yields `loader_mismatch = false` (conservative — never a false
/// alarm). `mc` is passed to `compat_verdict` (its signature needs it) but only
/// the loader outputs are surfaced.
pub async fn scan_instance(
    instance_root: &Path,
    instance_loader: LoaderKind,
    mc: &str,
) -> Result<Vec<ModLocalCompat>, Error> {
    let mods = installed::list(instance_root).await?;
    let dir = installed::mods_dir(instance_root);
    let mut out = Vec::with_capacity(mods.len());
    for m in &mods {
        let verdict = read_jar_for(&dir, &m.filename)
            .await
            .and_then(|bytes| read_jar_meta(&bytes).ok())
            .map(|meta| compat_verdict(&meta, instance_loader, mc));
        out.push(ModLocalCompat {
            sha1: m.sha1.clone(),
            loader_mismatch: verdict.as_ref().map(|v| v.loader_mismatch).unwrap_or(false),
            detected_loader: verdict.and_then(|v| v.detected_loader),
        });
    }
    Ok(out)
}

/// Read a mod jar's bytes by base filename, trying the `.disabled` variant
/// too. Returns `None` if neither exists or the read fails.
async fn read_jar_for(mods_dir: &Path, filename: &str) -> Option<Vec<u8>> {
    if let Ok(b) = fs::read(mods_dir.join(filename)).await {
        return Some(b);
    }
    fs::read(mods_dir.join(format!("{filename}.disabled")))
        .await
        .ok()
}

/// Install a local mod jar into `{instance}/.minecraft/mods/`: write the
/// bytes, record an `InstalledMod` with `source: None` (a manual mod —
/// no platform). `display_name` becomes the recorded `name`; when absent
/// the filename (without `.jar`) is used.
///
/// Filename-conflict handling mirrors `install::install_one`: a file
/// already present with the same name and the same SHA-1 is an idempotent
/// success; the same name with different bytes is `ModsFilenameConflict`.
pub async fn install_local(
    instance_root: &Path,
    filename: &str,
    bytes: &[u8],
    display_name: Option<&str>,
) -> Result<InstalledMod, Error> {
    let sha = hex::encode(Sha1::digest(bytes));

    let dest_dir = installed::mods_dir(instance_root);
    fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dest_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = dest_dir.join(filename);
    if fs::try_exists(&dest)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?
    {
        let existing = fs::read(&dest).await.map_err(|e| Error::ModsInstancePath {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?;
        let existing_sha = hex::encode(Sha1::digest(&existing));
        if !existing_sha.eq_ignore_ascii_case(&sha) {
            return Err(Error::ModsFilenameConflict {
                filename: filename.to_string(),
                existing_sha,
                incoming_sha: sha,
            });
        }
        // same name + same bytes → idempotent: fall through to record.
    } else {
        fs::write(&dest, bytes)
            .await
            .map_err(|e| Error::ModsInstancePath {
                path: dest.display().to_string(),
                details: e.to_string(),
            })?;
    }

    let name = display_name.map(str::to_string).unwrap_or_else(|| {
        filename
            .strip_suffix(".jar")
            .unwrap_or(filename)
            .to_string()
    });
    let entry = InstalledMod {
        filename: filename.to_string(),
        sha1: sha,
        source: None,
        project_id: None,
        version_id: None,
        name,
        version_number: None,
        installed_at: Utc::now().to_rfc3339(),
        enabled: true,
        enrich_attempted: false,
        requires: Vec::new(),
    };
    installed::add(instance_root, entry.clone()).await?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    /// Build an in-memory `.jar` (zip) with the given (name, contents) entries.
    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn first_major_minor_extracts_from_common_shapes() {
        assert_eq!(first_major_minor("1.12.2").as_deref(), Some("1.12"));
        assert_eq!(first_major_minor(">=1.20.1 <1.21").as_deref(), Some("1.20"));
        assert_eq!(first_major_minor("~1.19").as_deref(), Some("1.19"));
        assert_eq!(first_major_minor("*"), None);
        assert_eq!(first_major_minor("21w13a"), None);
    }

    #[test]
    fn detects_fabric_jar() {
        let j = jar(&[(
            "fabric.mod.json",
            r#"{"name":"Sodium","depends":{"minecraft":">=1.20.1"}}"#,
        )]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.family, Some(LoaderFamily::Fabric));
        assert_eq!(m.loader_label.as_deref(), Some("Fabric"));
        assert_eq!(m.mc_version.as_deref(), Some("1.20"));
        assert_eq!(m.display_name.as_deref(), Some("Sodium"));
    }

    #[test]
    fn detects_quilt_jar() {
        let j = jar(&[("quilt.mod.json", r#"{"quilt_loader":{"id":"x"}}"#)]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.family, Some(LoaderFamily::Fabric));
        assert_eq!(m.loader_label.as_deref(), Some("Quilt"));
    }

    #[test]
    fn quilt_jar_with_both_descriptors_labels_quilt_but_reads_fabric_meta() {
        // Quilt mods commonly ship both descriptors: the label comes from
        // the (priority-first) quilt descriptor, while the MC version and
        // display name are still read from fabric.mod.json.
        let j = jar(&[
            ("quilt.mod.json", r#"{"quilt_loader":{"id":"x"}}"#),
            (
                "fabric.mod.json",
                r#"{"name":"Sodium","depends":{"minecraft":">=1.20.1"}}"#,
            ),
        ]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.loader_label.as_deref(), Some("Quilt"));
        assert_eq!(m.mc_version.as_deref(), Some("1.20"));
        assert_eq!(m.display_name.as_deref(), Some("Sodium"));
    }

    #[test]
    fn detects_modern_forge_jar_without_mc_version() {
        let j = jar(&[("META-INF/mods.toml", "modLoader=\"javafml\"\n")]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.family, Some(LoaderFamily::Forge));
        assert_eq!(m.loader_label.as_deref(), Some("Forge"));
        assert_eq!(m.mc_version, None); // mods.toml is not parsed in v1
    }

    #[test]
    fn detects_neoforge_jar() {
        let j = jar(&[("META-INF/neoforge.mods.toml", "modLoader=\"javafml\"\n")]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.family, Some(LoaderFamily::Forge));
        assert_eq!(m.loader_label.as_deref(), Some("NeoForge"));
    }

    #[test]
    fn detects_legacy_forge_jar_with_mc_version() {
        let j = jar(&[(
            "mcmod.info",
            r#"[{"modid":"srparasites","name":"Scape and Run: Parasites","mcversion":"1.12.2"}]"#,
        )]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.family, Some(LoaderFamily::Forge));
        assert_eq!(m.loader_label.as_deref(), Some("Forge"));
        assert_eq!(m.mc_version.as_deref(), Some("1.12"));
        assert_eq!(m.display_name.as_deref(), Some("Scape and Run: Parasites"));
    }

    #[test]
    fn no_descriptor_yields_empty_meta() {
        let j = jar(&[("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n")]);
        let m = read_jar_meta(&j).unwrap();
        assert_eq!(m.family, None);
        assert_eq!(m.loader_label, None);
        assert_eq!(m.mc_version, None);
    }

    #[test]
    fn invalid_jar_errors() {
        let r = read_jar_meta(b"not a zip at all");
        assert!(r.is_err());
    }

    fn meta(family: Option<LoaderFamily>, mc: Option<&str>) -> JarMeta {
        JarMeta {
            family,
            loader_label: family.map(|f| match f {
                LoaderFamily::Fabric => "Fabric".into(),
                LoaderFamily::Forge => "Forge".into(),
            }),
            mc_version: mc.map(String::from),
            display_name: None,
        }
    }

    #[test]
    fn verdict_compatible_when_loader_and_mc_match() {
        let v = compat_verdict(
            &meta(Some(LoaderFamily::Forge), Some("1.12")),
            LoaderKind::Forge,
            "1.12.2",
        );
        assert!(!v.loader_mismatch);
        assert!(!v.mc_mismatch);
    }

    #[test]
    fn verdict_flags_loader_mismatch() {
        let v = compat_verdict(
            &meta(Some(LoaderFamily::Fabric), None),
            LoaderKind::Forge,
            "1.20.1",
        );
        assert!(v.loader_mismatch);
    }

    #[test]
    fn verdict_no_loader_mismatch_within_forge_family() {
        // A jar detected as Forge-family on a NeoForge instance — same family.
        let v = compat_verdict(
            &meta(Some(LoaderFamily::Forge), None),
            LoaderKind::NeoForge,
            "1.20.1",
        );
        assert!(!v.loader_mismatch);
    }

    #[test]
    fn verdict_flags_mc_mismatch() {
        let v = compat_verdict(
            &meta(Some(LoaderFamily::Forge), Some("1.20")),
            LoaderKind::Forge,
            "1.12.2",
        );
        assert!(v.mc_mismatch);
    }

    #[test]
    fn verdict_silent_when_metadata_absent() {
        // No descriptor at all — never warn.
        let v = compat_verdict(&meta(None, None), LoaderKind::Forge, "1.12.2");
        assert!(!v.loader_mismatch);
        assert!(!v.mc_mismatch);
    }

    #[test]
    fn verdict_silent_when_jar_mc_unknown() {
        let v = compat_verdict(
            &meta(Some(LoaderFamily::Forge), None),
            LoaderKind::Forge,
            "1.12.2",
        );
        assert!(!v.mc_mismatch);
    }

    // ── scan_instance tests ────────────────────────────────────────────────

    /// Build a minimal in-memory zip (jar) from (name, bytes) entries.
    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

    #[tokio::test]
    async fn scan_flags_fabric_jar_in_forge_instance() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("fabric.mod.json", br#"{"id":"x","name":"X"}"#)]);
        fs::write(dir.join("x.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].loader_mismatch);
        assert_eq!(out[0].detected_loader.as_deref(), Some("Fabric"));
    }

    #[tokio::test]
    async fn scan_no_mismatch_for_forge_jar_in_forge_instance() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("META-INF/mods.toml", b"modLoader=\"javafml\"")]);
        fs::write(dir.join("f.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
        assert_eq!(out[0].detected_loader.as_deref(), Some("Forge"));
    }

    #[tokio::test]
    async fn scan_no_mismatch_for_descriptorless_jar() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("data/whatever.txt", b"not a mod")]);
        fs::write(dir.join("lib.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Fabric, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
        assert!(out[0].detected_loader.is_none());
    }

    #[tokio::test]
    async fn scan_vanilla_instance_never_flags() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        let bytes = zip_with(&[("fabric.mod.json", br#"{"id":"x"}"#)]);
        fs::write(dir.join("x.jar"), &bytes).await.unwrap();

        let out = scan_instance(td.path(), LoaderKind::Vanilla, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
    }

    #[tokio::test]
    async fn scan_skips_unreadable_jar_but_still_succeeds() {
        use crate::mods::installed::mods_dir;
        let td = tempfile::TempDir::new().unwrap();
        let dir = mods_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(dir.join("broken.jar"), b"not a zip at all")
            .await
            .unwrap();

        let out = scan_instance(td.path(), LoaderKind::Forge, "1.21")
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].loader_mismatch);
        assert!(out[0].detected_loader.is_none());
    }

    // ── install_local tests ────────────────────────────────────────────────

    use tempfile::TempDir;

    #[tokio::test]
    async fn install_local_copies_jar_and_records_manual_mod() {
        let td_inst = TempDir::new().unwrap();
        let body = b"fabric-mod-bytes";
        let installed = install_local(td_inst.path(), "sodium.jar", body, Some("Sodium"))
            .await
            .unwrap();
        assert_eq!(installed.filename, "sodium.jar");
        assert!(crate::mods::installed::mods_dir(td_inst.path())
            .join("sodium.jar")
            .exists());
        let list = crate::mods::installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source, None); // manual mod
        assert_eq!(list[0].name, "Sodium");
        assert_eq!(list[0].sha1, installed.sha1);
    }

    #[tokio::test]
    async fn install_local_falls_back_to_filename_when_no_display_name() {
        let td_inst = TempDir::new().unwrap();
        install_local(td_inst.path(), "weird-mod.jar", b"x", None)
            .await
            .unwrap();
        let list = crate::mods::installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list[0].name, "weird-mod");
    }

    #[tokio::test]
    async fn install_local_same_name_same_bytes_is_idempotent() {
        let td_inst = TempDir::new().unwrap();
        install_local(td_inst.path(), "a.jar", b"same", Some("A"))
            .await
            .unwrap();
        install_local(td_inst.path(), "a.jar", b"same", Some("A"))
            .await
            .unwrap();
        let list = crate::mods::installed::list(td_inst.path()).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn install_local_same_name_different_bytes_conflicts() {
        let td_inst = TempDir::new().unwrap();
        install_local(td_inst.path(), "a.jar", b"first", Some("A"))
            .await
            .unwrap();
        let err = install_local(td_inst.path(), "a.jar", b"second", Some("A"))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ModsFilenameConflict { .. }));
    }
}
