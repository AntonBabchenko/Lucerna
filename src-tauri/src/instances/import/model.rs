use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::instances::schema::{ForeignLauncher, LoaderKind};
use crate::mods::platform::ModSource;

/// One copyable content category in a foreign instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ContentCategory {
    Mods,
    Config,
    Saves,
    ResourcePacks,
    Shaderpacks,
    OptionsTxt,
}

impl ContentCategory {
    /// Canonical sub-path under the instance's `.minecraft/` for this
    /// category (a dir, except `OptionsTxt` which is a single file).
    pub fn rel_path(self) -> &'static str {
        match self {
            ContentCategory::Mods => "mods",
            ContentCategory::Config => "config",
            ContentCategory::Saves => "saves",
            ContentCategory::ResourcePacks => "resourcepacks",
            ContentCategory::Shaderpacks => "shaderpacks",
            ContentCategory::OptionsTxt => "options.txt",
        }
    }
    pub fn is_file(self) -> bool {
        matches!(self, ContentCategory::OptionsTxt)
    }
}

/// A category present on disk in the source, with size info for the preview.
///
/// `total_bytes` is `f64` — specta forbids exporting `u64` across the IPC
/// boundary (BigInt precision loss). A directory's byte total stays well
/// within `f64`'s 2^53 exact-integer range, so no precision is lost.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
pub struct ContentEntry {
    pub category: ContentCategory,
    pub file_count: u32,
    pub total_bytes: f64,
}

/// A mod whose platform identity is already known from the source's
/// manifest (CurseForge App / Modrinth App). Empty for Prism / raw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct KnownMod {
    pub filename: String,
    pub source: ModSource,
    pub project_id: String,
    pub version_id: Option<String>,
}

/// Normalized foreign instance — the contract between readers and the
/// pipeline. The pipeline never branches on `source`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ForeignInstance {
    pub source: ForeignLauncher,
    pub name: String,
    pub root: PathBuf,
    pub minecraft_dir: PathBuf,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub max_heap_mb: Option<u32>,
    pub extra_jvm_args: Option<String>,
    pub content: Vec<ContentEntry>,
    pub known_mods: Vec<KnownMod>,
}

/// Result of an auto-discovery sweep. `empty_launchers` lists launchers whose
/// install root exists on disk but yielded no importable instance — so the UI
/// can say "found X, but nothing to import" instead of a generic empty state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct DiscoverResult {
    pub instances: Vec<ForeignInstance>,
    pub empty_launchers: Vec<ForeignLauncher>,
}

/// Pure, resolved plan: mapped instance fields + the categories to copy.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub max_heap_mb: u32,
    pub extra_jvm_args: String,
    pub copy_categories: Vec<ContentCategory>,
}

/// Typed progress streamed to the UI during an import.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ImportProgress {
    CreatingInstance {
        name: String,
    },
    Copying {
        category: ContentCategory,
        current: u32,
        total: u32,
    },
    RecoveringIdentities,
    /// Terminal phase marker — deliberately **payload-free**, for the same
    /// reason as `ModpackProgress::Done` (see that variant's doc comment): a
    /// channel message is delivered out-of-band from the command's response
    /// and can land after it, so anything the UI must READ belongs on the
    /// return value. Here that is [`LauncherImportOutcome::untracked_mods`].
    Done,
}

/// What `launcher_import_run` returns.
///
/// `untracked_mods` used to ride `ImportProgress::Done`, where the runner read
/// it out of a captured local right after awaiting the command — a read that is
/// only ever correct by luck of transport timing.
#[derive(Debug, Clone, Serialize, Type)]
pub struct LauncherImportOutcome {
    pub instance: crate::instances::schema::InstanceWithStatus,
    /// Copied jars whose identity could not be recovered from Modrinth or
    /// CurseForge — they stay in the instance but have no known provenance.
    pub untracked_mods: u32,
}

/// Strip `-Xmx`/`-Xms` tokens (those map to `max_heap_mb`) and collapse
/// surrounding whitespace.
fn strip_heap_flags(args: &str) -> String {
    args.split_whitespace()
        .filter(|t| {
            let l = t.to_ascii_lowercase();
            !l.starts_with("-xmx") && !l.starts_with("-xms")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the import plan. `heap_min`/`heap_max` are the adaptive bounds and
/// `heap_default` the adaptive default (all from the `memory` module). The
/// source heap is clamped into the bounds; when the source did not specify
/// one, the adaptive default is used — NOT the minimum, so a heavy modpack
/// imported from a launcher that doesn't store a per-instance heap still
/// gets a sane default (matching a freshly-created instance).
pub fn build_import_plan(
    foreign: &ForeignInstance,
    selected: &[ContentCategory],
    target_name: &str,
    heap_min: u32,
    heap_max: u32,
    heap_default: u32,
) -> ImportPlan {
    let requested = foreign.max_heap_mb.unwrap_or(heap_default);
    let max_heap_mb = requested.clamp(heap_min, heap_max);
    let extra_jvm_args = foreign
        .extra_jvm_args
        .as_deref()
        .map(strip_heap_flags)
        .unwrap_or_default();
    ImportPlan {
        name: target_name.to_string(),
        mc_version: foreign.mc_version.clone(),
        loader: foreign.loader,
        loader_version: foreign.loader_version.clone(),
        max_heap_mb,
        extra_jvm_args,
        copy_categories: selected.to_vec(),
    }
}

use std::fs;
use std::path::Path;

/// True for mods a launcher injects into its game dir that are launcher cruft,
/// not user content — currently TLauncher's auto-added skin/cape mod
/// (`tl_skin_cape_*`). Excluded from import so the new instance stays clean.
pub fn is_injected_mod(filename: &str) -> bool {
    filename.to_ascii_lowercase().starts_with("tl_skin_cape")
}

/// Inspect a `.minecraft`-shaped dir and report which content categories
/// exist, with a recursive file count and byte total (for the preview).
/// Never errors — a missing dir yields an empty list.
pub fn scan_content(minecraft_dir: &Path) -> Vec<ContentEntry> {
    let all = [
        ContentCategory::Mods,
        ContentCategory::Config,
        ContentCategory::Saves,
        ContentCategory::ResourcePacks,
        ContentCategory::Shaderpacks,
        ContentCategory::OptionsTxt,
    ];
    all.into_iter()
        .filter_map(|cat| {
            let path = minecraft_dir.join(cat.rel_path());
            if cat.is_file() {
                let bytes = fs::metadata(&path).ok()?.len();
                Some(ContentEntry {
                    category: cat,
                    file_count: 1,
                    total_bytes: bytes as f64,
                })
            } else {
                let (file_count, total_bytes) = dir_stats(&path)?;
                (file_count > 0).then_some(ContentEntry {
                    category: cat,
                    file_count,
                    total_bytes: total_bytes as f64,
                })
            }
        })
        .collect()
}

/// Recursive (count, bytes) for a dir. `None` if the dir does not exist.
fn dir_stats(dir: &Path) -> Option<(u32, u64)> {
    if !dir.is_dir() {
        return None;
    }
    let mut count = 0u32;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            // Do not follow symlinks while measuring (matches the copy guard).
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                count = count.saturating_add(1);
                bytes = bytes.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
            }
        }
    }
    Some((count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::{ForeignLauncher, LoaderKind};
    use std::path::PathBuf;

    fn foreign() -> ForeignInstance {
        ForeignInstance {
            source: ForeignLauncher::Prism,
            name: "ATM9".into(),
            root: PathBuf::from("/src/ATM9"),
            minecraft_dir: PathBuf::from("/src/ATM9/.minecraft"),
            mc_version: "1.20.1".into(),
            loader: LoaderKind::Forge,
            loader_version: Some("47.2.0".into()),
            max_heap_mb: Some(8192),
            extra_jvm_args: Some("-Xmx8192m -XX:+UseG1GC".into()),
            content: vec![
                ContentEntry {
                    category: ContentCategory::Mods,
                    file_count: 120,
                    total_bytes: 200_000_000.0,
                },
                ContentEntry {
                    category: ContentCategory::Saves,
                    file_count: 2,
                    total_bytes: 3_000_000_000.0,
                },
            ],
            known_mods: vec![],
        }
    }

    #[test]
    fn flags_tlauncher_injected_mod_only() {
        assert!(is_injected_mod("tl_skin_cape_forge_1.21.1-1.39.jar"));
        assert!(!is_injected_mod("sodium-fabric-0.5.jar"));
    }

    #[test]
    fn plan_maps_fields_and_strips_xmx_from_jvm_args() {
        let plan = build_import_plan(
            &foreign(),
            &[ContentCategory::Mods],
            "ATM9 (Prism)",
            4096,
            12288,
            4096,
        );
        assert_eq!(plan.mc_version, "1.20.1");
        assert_eq!(plan.loader, LoaderKind::Forge);
        assert_eq!(plan.loader_version.as_deref(), Some("47.2.0"));
        // -Xmx/-Xms removed; the rest preserved and trimmed.
        assert_eq!(plan.extra_jvm_args, "-XX:+UseG1GC");
    }

    #[test]
    fn plan_clamps_heap_to_bounds() {
        // source asked 8192; bounds are [4096, 6000] -> clamped to 6000.
        let plan = build_import_plan(&foreign(), &[ContentCategory::Mods], "n", 4096, 6000, 4096);
        assert_eq!(plan.max_heap_mb, 6000);
    }

    #[test]
    fn plan_only_includes_selected_categories() {
        let plan = build_import_plan(&foreign(), &[ContentCategory::Mods], "n", 2048, 12288, 2048);
        assert_eq!(plan.copy_categories, vec![ContentCategory::Mods]);
    }

    #[test]
    fn plan_uses_adaptive_default_when_source_has_no_heap() {
        // A source with no per-instance heap (e.g. Prism OverrideMemory=false)
        // must get the adaptive DEFAULT (3000), not the minimum (1024).
        let mut fi = foreign();
        fi.max_heap_mb = None;
        let plan = build_import_plan(&fi, &[ContentCategory::Mods], "n", 1024, 12288, 3000);
        assert_eq!(plan.max_heap_mb, 3000);
    }
}
