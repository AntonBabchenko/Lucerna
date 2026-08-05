//! What a self-completing modpack is still waiting for.
//!
//! A Modrinth `.mrpack` can only bundle files Modrinth hosts and permits
//! redistributing. Packs whose mods forbid it ship a helper mod instead —
//! `missingmodschecker` — which prompts the user to download the rest on first
//! launch. Such a pack therefore starts life with genuinely unmet mandatory
//! dependencies BY DESIGN, and blocking its launch demands a decision where
//! there is nothing to decide.
//!
//! The helper leaves a machine-readable manifest at
//! `<instance>/.minecraft/config/missing_mods_checker.json`, so we can say
//! exactly how many of its files have yet to arrive instead of guessing.
//!
//! ## This file is data, never instruction
//!
//! The manifest is authored by a third party and lives inside the instance. We
//! parse it, and the UI renders it. A `url` from it is opened only by a user's
//! click, and a `destination` is refused unless it is a single plain directory
//! name — see [`safe_destination`].

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Where the helper writes its manifest, relative to `<instance>/.minecraft`.
const MANIFEST_REL: &str = "config/missing_mods_checker.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PendingFile {
    /// Human-readable name, as the pack author wrote it.
    pub display_name: String,
    /// Filename the helper will produce, verbatim from the manifest.
    pub pattern: String,
    /// Where the user can obtain it. DISPLAY ONLY — never opened without a click.
    pub url: Option<String>,
    /// Single directory name under `.minecraft`, e.g. `mods` or `resourcepacks`.
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PackCompletion {
    /// Entries the manifest declares in total.
    pub total: u32,
    /// Those whose file is not on disk yet.
    pub outstanding: Vec<PendingFile>,
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    pattern: Option<String>,
    url: Option<String>,
    destination: Option<String>,
}

/// `destination` comes from a third-party file. Admit only a single plain
/// directory name: no absolute paths, no separators, no `..`. Anything else is
/// skipped rather than fatal — one bad entry must not hide the other thirty.
fn safe_destination(raw: &str) -> Option<String> {
    let d = raw.trim();
    if d.is_empty()
        || d.contains('/')
        || d.contains('\\')
        || d.contains("..")
        || d.contains(':')
        || d == "."
    {
        return None;
    }
    Some(d.to_string())
}

/// True when `name` satisfies `pattern`. Every measured entry is a literal
/// filename; `*` is honoured when present because the field is called `pattern`,
/// and nothing else is interpreted.
fn matches(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.to_ascii_lowercase(), name.to_ascii_lowercase());
    let Some((head, tail)) = p.split_once('*') else {
        return p == n;
    };
    // Only a single `*` is meaningful; a second one is treated as literal text
    // inside `tail`, which is stricter than glob and never over-matches.
    n.len() >= head.len() + tail.len() && n.starts_with(&head) && n.ends_with(&tail)
}

/// Read the manifest and report what has yet to arrive.
///
/// `None` when the instance has no such pack helper, or its manifest is
/// unreadable or not JSON — best-effort, exactly like every jar reader here.
pub fn read(instance_root: &Path) -> Option<PackCompletion> {
    let mc = instance_root.join(".minecraft");
    let raw = std::fs::read_to_string(mc.join(MANIFEST_REL)).ok()?;
    let entries: Vec<RawEntry> = serde_json::from_str(&raw).ok()?;

    let mut total = 0u32;
    let mut outstanding = Vec::new();
    for e in entries {
        let (Some(pattern), Some(destination)) = (e.pattern, e.destination) else {
            continue; // an entry we cannot check is not an entry we can report
        };
        let Some(destination) = safe_destination(&destination) else {
            continue;
        };
        if pattern.trim().is_empty() {
            continue;
        }
        total += 1;
        let dir = mc.join(&destination);
        let present = std::fs::read_dir(&dir).ok().is_some_and(|rd| {
            rd.filter_map(|x| x.ok()).any(|x| {
                let name = x.file_name().to_string_lossy().into_owned();
                // A jar the LAUNCHER disabled is still installed — it is renamed
                // to `<name>.disabled`, exactly as `installed::reconcile` reads
                // it. Treating that as absent would report the file outstanding
                // forever and permanently disarm the launch gate for the
                // instance, which is the opposite of what a user disabling one
                // mod asked for.
                let base = name.strip_suffix(".disabled").unwrap_or(&name);
                matches(&pattern, base)
            })
        });
        if !present {
            outstanding.push(PendingFile {
                display_name: e.display_name.unwrap_or_else(|| pattern.clone()),
                pattern,
                url: e.url,
                destination,
            });
        }
    }
    Some(PackCompletion { total, outstanding })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// The measured shape, from Better MC [FABRIC] BMC2 v40.
    #[test]
    fn reports_only_the_files_that_have_not_arrived() {
        let td = tempfile::TempDir::new().unwrap();
        let root = td.path();
        write(
            root,
            ".minecraft/config/missing_mods_checker.json",
            r#"[
              {"displayName":"Balm (Fabric Edition)","pattern":"balm-fabric-1.20.1-7.3.38.jar",
               "url":"https://www.curseforge.com/minecraft/mc-mods/balm-fabric/download/7420614",
               "destination":"mods"},
              {"displayName":"Stay Clear","pattern":"Stay Clear V1.1 [1.20.1].zip",
               "url":"https://example.invalid/x","destination":"resourcepacks"}
            ]"#,
        );
        write(root, ".minecraft/mods/balm-fabric-1.20.1-7.3.38.jar", "x");
        std::fs::create_dir_all(root.join(".minecraft/resourcepacks")).unwrap();

        let c = read(root).expect("manifest present");
        assert_eq!(c.total, 2);
        assert_eq!(c.outstanding.len(), 1);
        assert_eq!(c.outstanding[0].display_name, "Stay Clear");
        assert_eq!(c.outstanding[0].destination, "resourcepacks");

        // The last file arrives.
        write(
            root,
            ".minecraft/resourcepacks/Stay Clear V1.1 [1.20.1].zip",
            "x",
        );
        assert!(read(root).unwrap().outstanding.is_empty());
    }

    #[test]
    fn a_missing_or_malformed_manifest_is_none_not_an_error() {
        let td = tempfile::TempDir::new().unwrap();
        assert!(read(td.path()).is_none(), "no helper in this instance");
        write(
            td.path(),
            ".minecraft/config/missing_mods_checker.json",
            "{ not json",
        );
        assert!(read(td.path()).is_none());
    }

    #[test]
    fn matching_is_case_insensitive_and_supports_one_star() {
        assert!(matches("Balm-1.0.jar", "balm-1.0.JAR"));
        assert!(!matches("balm-1.0.jar", "balm-1.1.jar"));
        assert!(matches("balm-*.jar", "balm-9.9.9.jar"));
        assert!(!matches("balm-*.jar", "other-9.9.9.jar"));
        // A literal that happens to be a prefix must not match a longer name.
        // `.disabled` is stripped by the CALLER, not here — see the read-level
        // test below, which is what pins the launcher-disabled case.
        assert!(!matches("balm.jar", "balm.jar.disabled"));
    }

    /// A mod the launcher disabled is installed, not missing. Reporting it
    /// outstanding would keep the pack permanently "incomplete" and disarm the
    /// launch gate for good.
    #[test]
    fn a_launcher_disabled_jar_is_not_outstanding() {
        let td = tempfile::TempDir::new().unwrap();
        write(
            td.path(),
            ".minecraft/config/missing_mods_checker.json",
            r#"[{"displayName":"Balm","pattern":"balm.jar","destination":"mods"}]"#,
        );
        write(td.path(), ".minecraft/mods/balm.jar.disabled", "x");
        assert!(
            read(td.path()).unwrap().outstanding.is_empty(),
            "disabled is installed-but-off, not absent"
        );
    }

    /// `destination` is third-party input. An escape attempt is skipped, and the
    /// entries around it still count.
    #[test]
    fn a_destination_that_escapes_the_instance_is_refused() {
        for bad in [
            "..",
            "../..",
            "mods/../..",
            "C:\\Windows",
            "/etc",
            ".",
            "a/b",
        ] {
            assert!(safe_destination(bad).is_none(), "must refuse {bad}");
        }
        assert_eq!(safe_destination(" mods ").as_deref(), Some("mods"));

        let td = tempfile::TempDir::new().unwrap();
        write(
            td.path(),
            ".minecraft/config/missing_mods_checker.json",
            r#"[{"displayName":"evil","pattern":"x.jar","destination":"../.."},
                {"displayName":"fine","pattern":"y.jar","destination":"mods"}]"#,
        );
        let c = read(td.path()).unwrap();
        assert_eq!(c.total, 1, "the refused entry is not counted");
        assert_eq!(c.outstanding.len(), 1);
        assert_eq!(c.outstanding[0].display_name, "fine");
    }

    #[test]
    fn an_entry_without_a_pattern_is_skipped_without_losing_the_others() {
        let td = tempfile::TempDir::new().unwrap();
        write(
            td.path(),
            ".minecraft/config/missing_mods_checker.json",
            r#"[{"displayName":"no pattern","destination":"mods"},
                {"pattern":"y.jar","destination":"mods"}]"#,
        );
        let c = read(td.path()).unwrap();
        assert_eq!(c.total, 1);
        // No displayName: the pattern stands in, rather than an empty row.
        assert_eq!(c.outstanding[0].display_name, "y.jar");
    }
}
