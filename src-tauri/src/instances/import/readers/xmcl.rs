use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::loader_sniff::sniff_loader_from_mods;
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};

/// X Minecraft Launcher (XMCL). Own instance tree: `<data-root>/instances/
/// <name>/` where the instance folder IS the game dir (mods/saves/config
/// directly inside) and carries an `instance.json` with a `runtime` object.
/// The data root is user-chosen; its absolute path sits in the plain-text
/// `root` file inside XMCL's config dir (`%APPDATA%\xmcl` on Windows).
///
/// XMCL hard-links mod jars from a shared store — hard links read as regular
/// files, so scanning and copying work unchanged. `resourcepacks/` or
/// `shaderpacks/` may instead be a directory junction/symlink to the shared
/// pool; the content scan and the copy pipeline both skip symlinks, so those
/// dirs are (correctly) ignored rather than importing the whole pool.
pub struct XmclReader;

/// Tolerant view of XMCL's `instance.json`. Unknown fields (legacy-era
/// `liteloader`/`yarn` runtime keys, `tags`, `modpackVersion`, …) are
/// ignored by serde; `runtime` is required — its absence means the file is
/// not an XMCL instance (e.g. ATLauncher's same-named metadata).
#[derive(Deserialize)]
struct InstanceJson {
    #[serde(default)]
    name: Option<String>,
    runtime: Runtime,
    #[serde(rename = "maxMemory", default)]
    max_memory: Option<f64>,
    /// `true` | `"auto"` | `false` in XMCL; only literal `true` means the
    /// stored memory figure is actually applied at launch.
    #[serde(rename = "assignMemory", default)]
    assign_memory: serde_json::Value,
    #[serde(rename = "vmOptions", default)]
    vm_options: Vec<String>,
}

/// All plain version strings; empty string = loader not used.
#[derive(Deserialize, Default)]
struct Runtime {
    #[serde(default)]
    minecraft: String,
    #[serde(default)]
    forge: String,
    #[serde(rename = "neoForged", default)]
    neo_forged: String,
    #[serde(rename = "fabricLoader", default)]
    fabric_loader: String,
    #[serde(rename = "quiltLoader", default)]
    quilt_loader: String,
}

/// First non-empty runtime loader key wins. NeoForge before Forge, Quilt
/// before Fabric — the more specific loader takes precedence if a file ever
/// carries both. Versions pass through verbatim.
fn loader_from_runtime(rt: &Runtime) -> (LoaderKind, Option<String>) {
    let candidates = [
        (LoaderKind::NeoForge, &rt.neo_forged),
        (LoaderKind::Forge, &rt.forge),
        (LoaderKind::Quilt, &rt.quilt_loader),
        (LoaderKind::Fabric, &rt.fabric_loader),
    ];
    for (kind, version) in candidates {
        if !version.is_empty() {
            return (kind, Some(version.clone()));
        }
    }
    (LoaderKind::Vanilla, None)
}

impl LauncherReader for XmclReader {
    fn launcher(&self) -> ForeignLauncher {
        ForeignLauncher::Xmcl
    }

    fn default_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        // Primary: the data root XMCL itself points at via its `root` file.
        for cfg in crate::platform::xmcl_config_dirs() {
            if let Ok(raw) = std::fs::read_to_string(cfg.join("root")) {
                let p = PathBuf::from(raw.trim());
                if p.is_absolute() {
                    roots.push(p.join("instances"));
                }
            }
        }
        // Static fallbacks: wizard-default `.minecraftx` + config-dir root.
        roots.extend(
            crate::platform::default_launcher_roots()
                .into_iter()
                .filter(|p| {
                    p.ends_with("instances") && {
                        let s = p.to_string_lossy().to_lowercase();
                        s.contains("xmcl") || s.contains(".minecraftx")
                    }
                }),
        );
        let mut seen = std::collections::HashSet::new();
        roots.retain(|p| seen.insert(p.clone()));
        roots
    }

    fn detect(&self, dir: &Path) -> bool {
        // Shape-checked, not sentinel-only: ATLauncher also names its
        // metadata `instance.json` (a `launcher` object, no `runtime`).
        // Requiring an object `runtime` key rejects ATLauncher dirs
        // regardless of registry order.
        let Ok(raw) = std::fs::read_to_string(dir.join("instance.json")) else {
            return false;
        };
        serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get("runtime").map(|r| r.is_object()))
            .unwrap_or(false)
    }

    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        let invalid = |d: String| Error::ImportInstanceUnreadable {
            launcher: "xmcl".into(),
            details: d,
        };
        let raw = std::fs::read_to_string(dir.join("instance.json"))
            .map_err(|e| invalid(format!("instance.json: {e}")))?;
        let ij: InstanceJson =
            serde_json::from_str(&raw).map_err(|e| invalid(format!("instance.json: {e}")))?;

        let mc_version = ij.runtime.minecraft.clone();
        let (mut loader, loader_version) = loader_from_runtime(&ij.runtime);
        // When the runtime names no loader, fall back to sniffing the mods
        // folder (kind only; the version resolves downstream from kind + MC).
        if loader == LoaderKind::Vanilla {
            if let Some(sniffed) = sniff_loader_from_mods(&dir.join("mods")) {
                loader = sniffed;
            }
        }
        let name = ij.name.filter(|s| !s.is_empty()).unwrap_or_else(|| {
            dir.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        // Only literal `true`: with `"auto"`/`false`/absent XMCL decides at
        // launch and the stored figure is inactive (mirrors the Prism
        // `OverrideMemory` gate).
        let max_heap_mb = if ij.assign_memory == serde_json::Value::Bool(true) {
            ij.max_memory
                .filter(|m| m.is_finite() && *m >= 1.0 && *m <= u32::MAX as f64)
                .map(|m| m as u32)
        } else {
            None
        };
        let joined = ij.vm_options.join(" ");
        let extra_jvm_args = (!joined.trim().is_empty()).then_some(joined);

        Ok(ForeignInstance {
            source: ForeignLauncher::Xmcl,
            name,
            root: dir.to_path_buf(),
            minecraft_dir: dir.to_path_buf(),
            mc_version,
            loader,
            loader_version,
            max_heap_mb,
            extra_jvm_args,
            content: scan_content(dir),
            known_mods: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::import::model::ContentCategory;
    use std::path::Path;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn write_instance(dir: &Path, json: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("instance.json"), json).unwrap();
    }

    #[test]
    fn detects_an_xmcl_instance() {
        assert!(XmclReader.detect(&fixture("xmcl_fabric")));
    }

    #[test]
    fn rejects_non_xmcl_dirs() {
        // ATLauncher's instance.json has a `launcher` object but no `runtime`.
        assert!(!XmclReader.detect(&fixture("atlauncher_neoforge")));
        assert!(!XmclReader.detect(&fixture("raw_minecraft")));
        assert!(!XmclReader.detect(Path::new(env!("CARGO_MANIFEST_DIR"))));
    }

    #[test]
    fn atlauncher_read_fails_on_xmcl_instance() {
        // The other half of the sentinel-collision safety: ATLauncher's
        // sentinel-only detect fires on an XMCL dir, but its read must fail
        // (no `launcher` object) so detect_folder falls through to XmclReader.
        use crate::instances::import::readers::atlauncher::AtlauncherReader;
        assert!(AtlauncherReader.detect(&fixture("xmcl_fabric")));
        assert!(AtlauncherReader.read(&fixture("xmcl_fabric")).is_err());
    }

    #[test]
    fn detect_folder_resolves_xmcl_source() {
        use crate::instances::import::discovery::detect_folder;
        let fi = detect_folder(&fixture("xmcl_fabric")).expect("xmcl fixture detected");
        assert_eq!(fi.source, ForeignLauncher::Xmcl);
    }

    #[test]
    fn reads_fabric_instance() {
        let fi = XmclReader.read(&fixture("xmcl_fabric")).unwrap();
        assert_eq!(fi.source, ForeignLauncher::Xmcl);
        assert_eq!(fi.name, "Fabulously Optimized");
        assert_eq!(fi.mc_version, "1.21.1");
        assert_eq!(fi.loader, LoaderKind::Fabric);
        assert_eq!(fi.loader_version.as_deref(), Some("0.16.5"));
        assert_eq!(fi.max_heap_mb, Some(4096));
        assert_eq!(fi.extra_jvm_args.as_deref(), Some("-XX:+UseG1GC"));
        assert!(fi
            .content
            .iter()
            .any(|c| c.category == ContentCategory::Mods));
    }

    #[test]
    fn neoforge_wins_over_forge_and_versions_pass_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let inst = tmp.path().join("NeoPack");
        write_instance(
            &inst,
            r#"{"name":"NeoPack","runtime":{"minecraft":"1.20.4","forge":"49.0.3","neoForged":"20.4.251"}}"#,
        );
        let fi = XmclReader.read(&inst).unwrap();
        assert_eq!(fi.loader, LoaderKind::NeoForge);
        assert_eq!(fi.loader_version.as_deref(), Some("20.4.251"));
    }

    #[test]
    fn heap_ignored_unless_assign_memory_is_literal_true() {
        let tmp = tempfile::tempdir().unwrap();
        let auto = tmp.path().join("Auto");
        write_instance(
            &auto,
            r#"{"runtime":{"minecraft":"1.21.1"},"maxMemory":8192,"assignMemory":"auto"}"#,
        );
        assert_eq!(XmclReader.read(&auto).unwrap().max_heap_mb, None);

        let absent = tmp.path().join("Absent");
        write_instance(
            &absent,
            r#"{"runtime":{"minecraft":"1.21.1"},"maxMemory":8192}"#,
        );
        assert_eq!(XmclReader.read(&absent).unwrap().max_heap_mb, None);
    }

    #[test]
    fn falls_back_to_mods_sniff_when_runtime_names_no_loader() {
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;
        fn fabric_jar() -> Vec<u8> {
            let mut buf = Vec::new();
            {
                let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
                w.start_file("fabric.mod.json", SimpleFileOptions::default())
                    .unwrap();
                w.write_all(br#"{"id":"sodium","name":"Sodium"}"#).unwrap();
                w.finish().unwrap();
            }
            buf
        }
        let tmp = tempfile::tempdir().unwrap();
        let inst = tmp.path().join("SniffMe");
        write_instance(&inst, r#"{"runtime":{"minecraft":"1.20.1"}}"#);
        std::fs::create_dir_all(inst.join("mods")).unwrap();
        std::fs::write(inst.join("mods/sodium.jar"), fabric_jar()).unwrap();
        let fi = XmclReader.read(&inst).unwrap();
        assert_eq!(fi.loader, LoaderKind::Fabric);
        assert_eq!(fi.loader_version, None);
    }

    #[test]
    fn name_falls_back_to_dir_name() {
        let tmp = tempfile::tempdir().unwrap();
        let inst = tmp.path().join("MyPack");
        write_instance(&inst, r#"{"runtime":{"minecraft":"1.21.1"}}"#);
        assert_eq!(XmclReader.read(&inst).unwrap().name, "MyPack");
    }

    #[test]
    fn legacy_schema_with_extra_keys_parses() {
        // Old (v0.49-era) files carry liteloader/yarn runtime keys plus
        // tags/modpackVersion/playTime at top level — all must be tolerated.
        let tmp = tempfile::tempdir().unwrap();
        let inst = tmp.path().join("OldPack");
        write_instance(
            &inst,
            r#"{"name":"OldPack","runtime":{"minecraft":"1.16.5","forge":"36.2.39","liteloader":"","yarn":""},"tags":[],"modpackVersion":"1.0.0","playTime":12}"#,
        );
        let fi = XmclReader.read(&inst).unwrap();
        assert_eq!(fi.mc_version, "1.16.5");
        assert_eq!(fi.loader, LoaderKind::Forge);
        assert_eq!(fi.loader_version.as_deref(), Some("36.2.39"));
    }
}
