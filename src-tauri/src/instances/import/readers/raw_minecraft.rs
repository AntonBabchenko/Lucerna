use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::loader_sniff::sniff_loader_from_mods;
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};

/// Fallback reader for a bare `.minecraft`-shaped folder. Carries no
/// version/loader metadata — the UI requires the user to supply those.
pub struct RawMinecraftReader;

impl RawMinecraftReader {
    /// A dir "looks like" a `.minecraft` if it has any of the canonical
    /// content subdirs/files.
    fn looks_like_minecraft(dir: &Path) -> bool {
        ["mods", "saves", "config", "resourcepacks"]
            .iter()
            .any(|d| dir.join(d).is_dir())
            || dir.join("options.txt").is_file()
    }
}

impl LauncherReader for RawMinecraftReader {
    fn launcher(&self) -> ForeignLauncher {
        ForeignLauncher::RawMinecraft
    }
    fn default_roots(&self) -> Vec<PathBuf> {
        // No auto-discovery: a bare .minecraft is manual-pick only.
        vec![]
    }
    fn detect(&self, dir: &Path) -> bool {
        Self::looks_like_minecraft(dir)
    }
    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        // Best-effort hint from `versions/`; the user confirms/overrides it
        // in the dropdown (a `.minecraft` doesn't record "the" version).
        let mc_version = detect_mc_version_hint(dir).unwrap_or_default();
        let name = instance_name_for(dir, &mc_version);
        Ok(ForeignInstance {
            source: ForeignLauncher::RawMinecraft,
            name,
            root: dir.to_path_buf(),
            minecraft_dir: dir.to_path_buf(),
            mc_version,
            // Best-effort loader from the mods folder; user may override in the
            // wizard. `None` (no/ambiguous descriptors) stays Vanilla.
            loader: sniff_loader_from_mods(&dir.join("mods")).unwrap_or(LoaderKind::Vanilla),
            loader_version: None,
            max_heap_mb: None,
            extra_jvm_args: None,
            content: scan_content(dir),
            known_mods: vec![],
        })
    }
}

/// A sensible default instance name for a manually-picked folder.
/// - A meaningful folder name (anything but the generic `.minecraft`) is used
///   directly (e.g. a modpack folder, or a TLauncher `versions/<name>`).
/// - When the folder *is* `.minecraft`, its parent is used — unless the parent
///   is a generic OS location (`Roaming`, `AppData`, …) which carries no
///   meaning, in which case a friendly default (`Minecraft <version>`, or just
///   `Minecraft`) is used instead.
/// The user can rename it before importing.
fn instance_name_for(dir: &Path, mc_version: &str) -> String {
    const GENERIC: &[&str] = &[
        "roaming",
        "appdata",
        "local",
        "application support",
        "home",
        "minecraft",
        ".minecraft",
    ];
    if let Some(self_name) = dir.file_name().and_then(|s| s.to_str()) {
        if !self_name.eq_ignore_ascii_case(".minecraft") && !self_name.is_empty() {
            return self_name.to_string();
        }
    }
    if let Some(parent) = dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
    {
        if !parent.is_empty() && !GENERIC.iter().any(|g| parent.eq_ignore_ascii_case(g)) {
            return parent.to_string();
        }
    }
    if mc_version.is_empty() {
        "Minecraft".to_string()
    } else {
        format!("Minecraft {mc_version}")
    }
}

/// Best-effort: the most likely Minecraft version installed in a
/// `.minecraft`, inferred from `versions/<id>/`. The last-used profile
/// (`lastVersionId` in `launcher_profiles.json`, written by the official /
/// TLauncher launchers) wins when it is installed here: a vanilla id
/// directly, a modded profile (the Forge installer's `1.20.1-forge-47.2.0`)
/// through the version its own JSON inherits from. Otherwise the highest
/// release-looking version. `None` when nothing version-shaped is present
/// (e.g. the picked folder is itself a single game dir with no `versions/`).
pub(crate) fn detect_mc_version_hint(dir: &Path) -> Option<String> {
    let versions = dir.join("versions");
    let installed: Vec<String> = std::fs::read_dir(&versions)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    // Alias ids like `latest-release` match no installed folder; requiring a
    // match also keeps the profile read below inside `versions/`.
    if let Some(last) = last_version_id(dir).filter(|l| installed.contains(l)) {
        if is_version_like(&last) {
            return Some(last);
        }
        if let Some(mc) = inherited_mc_version(&versions.join(&last), &last) {
            return Some(mc);
        }
    }
    let names: Vec<&str> = installed
        .iter()
        .map(String::as_str)
        .filter(|n| is_version_like(n))
        .collect();
    // Else the highest release (no pre/rc/snapshot suffix), else highest overall.
    names
        .iter()
        .filter(|n| !n.contains('-'))
        .max_by(|a, b| version_key(a).cmp(&version_key(b)))
        .or_else(|| {
            names
                .iter()
                .max_by(|a, b| version_key(a).cmp(&version_key(b)))
        })
        .map(|n| n.to_string())
}

/// The Minecraft version a modded profile builds on: `inheritsFrom` of its
/// own `versions/<id>/<id>.json`, when that is a Mojang version id. `None`
/// for an unreadable JSON or a flattened profile that inherits nothing.
fn inherited_mc_version(profile_dir: &Path, id: &str) -> Option<String> {
    let raw = std::fs::read_to_string(profile_dir.join(format!("{id}.json"))).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let parent = json.get("inheritsFrom")?.as_str()?;
    is_version_like(parent).then(|| parent.to_string())
}

/// `true` for a name shaped like a Mojang version id: dotted numeric
/// (`26.1.2`, `1.20.4`, `26.1`), optionally followed by one pre-release
/// suffix in the shapes the version manifest uses — `-pre1` / `-rc1` (1.x),
/// `-pre-1` / `-rc-1` / `-snapshot-1` (26.x); all 917 manifest ids checked
/// 2026-09-29. Rejects loader-named dirs (`Forge 26.1.2`, `test`) and
/// profile ids whose head merely is a version (the Forge installer's
/// `1.20.1-forge-47.2.0`, OptiFine's `1.20.1-OptiFine_HD_U_I6`).
pub(crate) fn is_version_like(name: &str) -> bool {
    let (head, suffix) = match name.split_once('-') {
        Some((head, suffix)) => (head, Some(suffix)),
        None => (name, None),
    };
    head.contains('.')
        && head
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        && suffix.is_none_or(is_pre_release_suffix)
}

/// One of Mojang's pre-release suffixes (see `is_version_like`).
fn is_pre_release_suffix(suffix: &str) -> bool {
    ["pre-", "rc-", "snapshot-", "pre", "rc"]
        .iter()
        .any(|prefix| {
            suffix
                .strip_prefix(prefix)
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
}

/// Numeric-tuple sort key from the leading dotted segments (suffix ignored).
fn version_key(name: &str) -> Vec<u32> {
    name.split('-')
        .next()
        .unwrap_or("")
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// The `lastVersionId` of the most-recently-used profile in
/// `launcher_profiles.json`, ignoring alias ids (`latest-release` etc.).
fn last_version_id(dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(dir.join("launcher_profiles.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let profiles = v.get("profiles")?.as_object()?;
    let mut best: Option<(String, String)> = None; // (lastUsed, lastVersionId)
    for p in profiles.values() {
        let id = p
            .get("lastVersionId")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if id.is_empty() || id.starts_with("latest-") {
            continue;
        }
        let used = p
            .get("lastUsed")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if best.as_ref().map(|(u, _)| used > *u).unwrap_or(true) {
            best = Some((used, id.to_string()));
        }
    }
    best.map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::import::model::ContentCategory;
    use crate::instances::schema::LoaderKind;
    use std::path::Path;

    fn raw() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw_minecraft")
    }

    #[test]
    fn detects_a_minecraft_folder() {
        assert!(RawMinecraftReader.detect(&raw()));
    }

    #[test]
    fn rejects_a_dir_without_minecraft_markers() {
        assert!(!RawMinecraftReader.detect(Path::new(env!("CARGO_MANIFEST_DIR"))));
    }

    #[test]
    fn reads_content_with_unknown_version_and_loader() {
        let fi = RawMinecraftReader.read(&raw()).unwrap();
        // Version/loader are user-supplied later — reader leaves them blank.
        assert_eq!(fi.mc_version, "");
        assert_eq!(fi.loader, LoaderKind::Vanilla);
        assert_eq!(fi.loader_version, None);
        assert!(fi
            .content
            .iter()
            .any(|c| c.category == ContentCategory::Mods));
        assert!(fi
            .content
            .iter()
            .any(|c| c.category == ContentCategory::OptionsTxt));
    }

    #[test]
    fn detects_highest_release_version_from_versions_dir() {
        // Mirrors a real official-launcher .minecraft: a release + a snapshot
        // installed, lastVersionId is the `latest-release` alias (no match).
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path();
        std::fs::create_dir_all(mc.join("versions/26.1.2")).unwrap();
        std::fs::create_dir_all(mc.join("versions/26.2-rc-2")).unwrap();
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        std::fs::write(
            mc.join("launcher_profiles.json"),
            r#"{"profiles":{"a":{"lastVersionId":"latest-release","lastUsed":"2026-06-14T16:00:00Z"}}}"#,
        )
        .unwrap();
        let fi = RawMinecraftReader.read(mc).unwrap();
        assert_eq!(fi.mc_version, "26.1.2"); // release beats the -rc snapshot
    }

    #[test]
    fn prefers_concrete_last_version_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path();
        std::fs::create_dir_all(mc.join("versions/1.20.4")).unwrap();
        std::fs::create_dir_all(mc.join("versions/1.21.1")).unwrap();
        std::fs::create_dir_all(mc.join("saves")).unwrap();
        std::fs::write(
            mc.join("launcher_profiles.json"),
            r#"{"profiles":{"a":{"lastVersionId":"1.20.4","lastUsed":"2026-01-01T00:00:00Z"}}}"#,
        )
        .unwrap();
        let fi = RawMinecraftReader.read(mc).unwrap();
        assert_eq!(fi.mc_version, "1.20.4"); // concrete lastVersionId wins over higher 1.21.1
    }

    #[test]
    fn version_like_accepts_every_mojang_id_shape() {
        // The six shapes of the live version manifest.
        for id in [
            "1.20.1",
            "26.1",
            "1.20.4-pre1",
            "1.16-rc1",
            "26.1-pre-1",
            "26.2-rc-2",
            "26.1-snapshot-1",
        ] {
            assert!(is_version_like(id), "{id} must be version-like");
        }
    }

    #[test]
    fn version_like_rejects_loader_and_foreign_suffixes() {
        // A version head with any other suffix is a profile id, not a version.
        for id in [
            "1.20.1-forge-47.2.0",
            "1.20.1-OptiFine_HD_U_I6",
            "1.7.10-Forge10.13.4.1614-1.7.10",
            "1.20.4-pre",
            "1.20-",
            "Forge 1.21.1",
            "test",
        ] {
            assert!(!is_version_like(id), "{id} must not be version-like");
        }
    }

    #[test]
    fn hint_resolves_last_used_forge_profile_to_its_minecraft() {
        // Official launcher + Forge installer, Forge played last, two vanilla
        // versions installed: the answer is the Forge profile's Minecraft.
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path();
        std::fs::create_dir_all(mc.join("versions/1.20.1")).unwrap();
        std::fs::create_dir_all(mc.join("versions/1.21.1")).unwrap();
        let forge = mc.join("versions/1.20.1-forge-47.2.0");
        std::fs::create_dir_all(&forge).unwrap();
        std::fs::write(
            forge.join("1.20.1-forge-47.2.0.json"),
            r#"{"id":"1.20.1-forge-47.2.0","inheritsFrom":"1.20.1"}"#,
        )
        .unwrap();
        std::fs::write(
            mc.join("launcher_profiles.json"),
            r#"{"profiles":{
                "forge":{"lastVersionId":"1.20.1-forge-47.2.0","lastUsed":"2026-09-01T10:00:00Z"},
                "a":{"lastVersionId":"latest-release","lastUsed":"2026-08-01T10:00:00Z"}}}"#,
        )
        .unwrap();
        assert_eq!(detect_mc_version_hint(mc).as_deref(), Some("1.20.1"));
    }

    #[test]
    fn hint_is_none_when_only_a_loader_profile_folder_exists() {
        // No vanilla folder and no last-used profile to follow: nothing here
        // names a Minecraft version, so the user is asked.
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path();
        std::fs::create_dir_all(mc.join("versions/1.20.1-forge-47.2.0")).unwrap();
        assert_eq!(detect_mc_version_hint(mc), None);
    }

    #[test]
    fn names_from_meaningful_parent_when_dir_is_dot_minecraft() {
        // …/MyPack/.minecraft → use the meaningful parent "MyPack".
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join("MyPack/.minecraft");
        std::fs::create_dir_all(mc.join("saves")).unwrap();
        let fi = RawMinecraftReader.read(&mc).unwrap();
        assert_eq!(fi.name, "MyPack");
    }

    #[test]
    fn names_friendly_default_for_generic_parent() {
        // …/Roaming/.minecraft → parent is a generic OS dir; fall back to a
        // friendly default with the detected version, not "Roaming".
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join("Roaming/.minecraft");
        std::fs::create_dir_all(mc.join("versions/1.20.4")).unwrap();
        std::fs::create_dir_all(mc.join("saves")).unwrap();
        let fi = RawMinecraftReader.read(&mc).unwrap();
        assert_eq!(fi.name, "Minecraft 1.20.4");
    }

    #[test]
    fn read_sniffs_loader_from_mods() {
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
        let mc = tmp.path();
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        std::fs::write(mc.join("mods/sodium.jar"), fabric_jar()).unwrap();
        let fi = RawMinecraftReader.read(mc).unwrap();
        assert_eq!(fi.loader, LoaderKind::Fabric);
        assert_eq!(fi.loader_version, None);
    }
}
