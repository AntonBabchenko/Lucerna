//! Server plugin management. Plugins live in `runtime/plugins/` and are only
//! loaded by Bukkit-family cores (Paper/Purpur). A plugin jar carries
//! `plugin.yml` (or Paper's `paper-plugin.yml`) at the jar root — presence is
//! checked by zip entry name, no YAML parsing (no new crates).
//! Enable/disable is the same `.jar <-> .jar.disabled` rename convention the
//! mods pipeline uses (Bukkit only loads `*.jar`).

use crate::error::{Error, Result};
use std::io::Read;
use std::path::Path;

/// True iff `bytes` is a zip carrying `plugin.yml` or `paper-plugin.yml` at
/// its root. Best-effort: non-zip yields `false`. Nested descriptors do not
/// count (Bukkit would not load such a jar either).
pub fn jar_is_plugin(bytes: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return false;
    };
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        if entry.name() == "plugin.yml" || entry.name() == "paper-plugin.yml" {
            return true;
        }
    }
    false
}

/// Validate `src_jar` is a plugin and copy it into `dir` (created if absent),
/// returning the installed filename. Overwrites an existing same-name jar.
pub fn install_local_plugin(dir: &Path, src_jar: &Path) -> Result<String> {
    let filename = src_jar
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::io("<plugin>", "source path has no filename"))?;
    if !crate::servers_runtime::runtime::is_safe_mod_name(&filename) {
        return Err(Error::io("<plugin>", "invalid filename"));
    }
    if !filename.to_ascii_lowercase().ends_with(".jar") {
        return Err(Error::io("<plugin>", "plugin must be a .jar"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(src_jar)
        .and_then(|mut f| f.read_to_end(&mut bytes).map(|_| ()))
        .map_err(|e| Error::io(src_jar.display().to_string(), e))?;
    if !jar_is_plugin(&bytes) {
        return Err(Error::io(
            "<plugin>",
            "not a plugin jar (no plugin.yml at the jar root)",
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let dest = dir.join(&filename);
    if !dest.starts_with(dir) {
        return Err(Error::io("<plugin>", "path escapes plugins dir"));
    }
    std::fs::write(&dest, &bytes).map_err(|e| Error::io(dest.display().to_string(), e))?;
    Ok(filename)
}

/// (filename, disabled) for every `.jar` / `.jar.disabled` in `dir`, sorted.
/// Missing dir yields an empty list.
pub fn list_plugins(dir: &Path) -> Vec<(String, bool)> {
    let mut names = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let low = n.to_ascii_lowercase();
            if low.ends_with(".jar") || low.ends_with(".jar.disabled") {
                let disabled = low.ends_with(".jar.disabled");
                names.push((n, disabled));
            }
        }
    }
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

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

    #[test]
    fn plugin_yml_at_root_is_a_plugin() {
        assert!(jar_is_plugin(&jar(&[(
            "plugin.yml",
            b"name: X\nmain: a.B\n"
        )])));
        assert!(jar_is_plugin(&jar(&[("paper-plugin.yml", b"name: X\n")])));
    }

    #[test]
    fn nested_plugin_yml_or_mod_jar_is_not_a_plugin() {
        assert!(!jar_is_plugin(&jar(&[("sub/plugin.yml", b"name: X\n")])));
        assert!(!jar_is_plugin(&jar(&[("fabric.mod.json", b"{}")])));
        assert!(!jar_is_plugin(b"not a zip"));
    }

    #[test]
    fn install_local_plugin_writes_validated_jar() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("LuckPerms.jar");
        std::fs::write(&src, jar(&[("plugin.yml", b"name: LuckPerms\n")])).unwrap();
        let dir = td.path().join("plugins");
        let name = install_local_plugin(&dir, &src).unwrap();
        assert_eq!(name, "LuckPerms.jar");
        assert!(dir.join("LuckPerms.jar").exists());
    }

    #[test]
    fn install_local_plugin_rejects_mod_jar() {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("sodium.jar");
        std::fs::write(&src, jar(&[("fabric.mod.json", b"{}")])).unwrap();
        let dir = td.path().join("plugins");
        assert!(install_local_plugin(&dir, &src).is_err());
        assert!(!dir.join("sodium.jar").exists());
    }

    #[test]
    fn list_plugins_reports_disabled_state_sorted() {
        let td = tempfile::tempdir().unwrap();
        let dir = td.path().join("plugins");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.jar"), b"x").unwrap();
        std::fs::write(dir.join("a.jar.disabled"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let list = list_plugins(&dir);
        assert_eq!(
            list,
            vec![
                ("a.jar.disabled".to_string(), true),
                ("b.jar".to_string(), false)
            ]
        );
    }
}
