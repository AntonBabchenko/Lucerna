//! Pure decisions about `missing_mods` entries (spec §3.2–§3.4).

use std::collections::HashSet;

use crate::mods::installed::{ManualSupplied, PackOrigin, ResolvedMissing, SupplyVerified};
use crate::mods::modpack::schema::{
    MissingModState, MissingModStatus, ModpackUnresolvable, UnresolvableReason,
};
use crate::mods::platform::InstalledMod;

fn present(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// One stable name per entry, the same before and after a pack update that
/// keeps the file: its CurseForge file id, else its md5, else its sha1, else
/// its name and file name (the pair `ResolvedMissing` already uses).
pub fn entry_key(e: &ModpackUnresolvable) -> String {
    if let Some(id) = present(&e.meta.file_id) {
        return format!("file:{id}");
    }
    if let Some(md5) = present(&e.meta.md5) {
        return format!("md5:{}", md5.to_ascii_lowercase());
    }
    if let Some(sha1) = present(&e.sha1) {
        return format!("sha1:{}", sha1.to_ascii_lowercase());
    }
    format!("name:{}|{}", e.mod_name, e.filename.to_ascii_lowercase())
}

/// The digest a file must have to close an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    Sha1(String),
    Md5(String),
    /// Nothing was published to check against (spec D8).
    Nothing,
}

pub fn expected(e: &ModpackUnresolvable) -> Expected {
    if let Some(s) = present(&e.sha1) {
        return Expected::Sha1(s.to_ascii_lowercase());
    }
    if let Some(m) = present(&e.meta.md5) {
        return Expected::Md5(m.to_ascii_lowercase());
    }
    Expected::Nothing
}

/// `UnsupportedType` rows are a manual step Lucerna cannot perform: listed,
/// never pending, never placeable.
pub fn is_actionable(e: &ModpackUnresolvable) -> bool {
    e.reason != UnresolvableReason::UnsupportedType
}

/// The leading, non-version part of a jar filename — a fuzzy,
/// version-independent mod identity. `srparasites-1.12.2-2.7.1.jar` →
/// `srparasites`. `None` when nothing remains — a `None` stem never matches.
pub(crate) fn filename_stem(filename: &str) -> Option<String> {
    let base = filename.strip_suffix(".disabled").unwrap_or(filename);
    let base = base.strip_suffix(".jar").unwrap_or(base);
    let mut kept: Vec<&str> = Vec::new();
    for seg in base.split(['-', '_']) {
        if seg.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            break;
        }
        if !seg.is_empty() {
            kept.push(seg);
        }
    }
    if kept.is_empty() {
        None
    } else {
        Some(kept.join("-").to_ascii_lowercase())
    }
}

/// Installed jars that look like another version of `e`'s mod: the platform
/// project id, the descriptor name, or the version-independent file-name stem.
/// Fuzzy on purpose — it only ever produces a warning or a Replace question.
pub fn different_version_rows<'a>(
    e: &ModpackUnresolvable,
    installed: &'a [InstalledMod],
) -> Vec<&'a InstalledMod> {
    let stem = filename_stem(&e.filename);
    installed
        .iter()
        .filter(|i| {
            // `is_some()`: two project-id-less mods must not match on `None == None`.
            (e.project_id.is_some() && i.project_id.as_deref() == e.project_id.as_deref())
                || i.name.eq_ignore_ascii_case(&e.mod_name)
                || (stem.is_some() && stem == filename_stem(&i.filename))
        })
        .collect()
}

/// The sha1 on disk that closes `e` by a published hash: its own sha1, or the
/// sha1 noted when a file matched its md5.
pub fn closing_sha1(
    e: &ModpackUnresolvable,
    supplied: &[ManualSupplied],
    on_disk: &HashSet<String>,
) -> Option<String> {
    let key = entry_key(e);
    let own = present(&e.sha1).map(str::to_ascii_lowercase);
    let learned = supplied
        .iter()
        .filter(|s| s.key == key && s.verified == SupplyVerified::Md5)
        .map(|s| s.sha1.to_ascii_lowercase());
    own.into_iter().chain(learned).find(|s| on_disk.contains(s))
}

fn chosen_sha1(
    e: &ModpackUnresolvable,
    supplied: &[ManualSupplied],
    on_disk: &HashSet<String>,
) -> Option<String> {
    let key = entry_key(e);
    supplied
        .iter()
        .filter(|s| s.key == key && s.verified == SupplyVerified::User)
        .map(|s| s.sha1.to_ascii_lowercase())
        .find(|s| on_disk.contains(s))
}

/// Classify one entry (spec §3.3). Never by a file's name.
pub fn entry_state(
    e: &ModpackUnresolvable,
    installed: &[InstalledMod],
    resolved: &[ResolvedMissing],
    supplied: &[ManualSupplied],
    on_disk: &HashSet<String>,
) -> MissingModState {
    // A substitute from another platform closes the entry while its jar is
    // installed; `mod_name` matches byte for byte by design (it was copied).
    let substituted = resolved.iter().any(|r| {
        r.filename.eq_ignore_ascii_case(&e.filename)
            && r.mod_name == e.mod_name
            && installed
                .iter()
                .any(|i| i.sha1.eq_ignore_ascii_case(&r.sha1))
    });
    if substituted {
        return MissingModState::Substituted;
    }
    if closing_sha1(e, supplied, on_disk).is_some() {
        return MissingModState::Installed;
    }
    if chosen_sha1(e, supplied, on_disk).is_some() {
        return MissingModState::ChosenUnverified;
    }
    if different_version_rows(e, installed).is_empty() {
        MissingModState::Missing
    } else {
        MissingModState::DifferentVersion
    }
}

pub fn statuses(
    origin: &PackOrigin,
    installed: &[InstalledMod],
    on_disk: &HashSet<String>,
) -> Vec<MissingModStatus> {
    origin
        .missing_mods
        .iter()
        .map(|m| MissingModStatus {
            entry: m.clone(),
            state: entry_state(
                m,
                installed,
                &origin.resolved_missing,
                &origin.manual_supplied,
                on_disk,
            ),
        })
        .collect()
}

/// SHA-1s of files closing entries that are not themselves `origin.files`
/// (a jar the pack also bundles stays a bundled pack file, spec §3.4).
pub fn supplied_shas(
    origin: &PackOrigin,
    statuses: &[MissingModStatus],
    on_disk: &HashSet<String>,
) -> Vec<String> {
    let files: HashSet<String> = origin
        .files
        .iter()
        .map(|f| f.sha1.to_ascii_lowercase())
        .collect();
    let mut out: Vec<String> = Vec::new();
    for st in statuses {
        let sha = match st.state {
            MissingModState::Installed => closing_sha1(&st.entry, &origin.manual_supplied, on_disk),
            MissingModState::ChosenUnverified => {
                chosen_sha1(&st.entry, &origin.manual_supplied, on_disk)
            }
            _ => None,
        };
        if let Some(sha) = sha {
            if !files.contains(&sha) && !out.contains(&sha) {
                out.push(sha);
            }
        }
    }
    out
}

/// A supplied file as the update diff sees it (spec §4.5).
#[derive(Debug, Clone)]
pub struct SuppliedOld {
    pub key: String,
    pub md5: Option<String>,
    pub file: crate::mods::installed::PackOriginFile,
}

/// The supplied files of an origin, each described where it actually is: a
/// mods row with its sha1 → `mods/<its name>`; an assets row → its folder;
/// else the entry's own path.
pub fn supplied_old(
    origin: &PackOrigin,
    statuses: &[MissingModStatus],
    on_disk: &HashSet<String>,
    installed: &[InstalledMod],
    assets: &[crate::mods::platform::InstalledAsset],
) -> Vec<SuppliedOld> {
    let files: HashSet<String> = origin
        .files
        .iter()
        .map(|f| f.sha1.to_ascii_lowercase())
        .collect();
    let mut out = Vec::new();
    for st in statuses {
        let sha = match st.state {
            MissingModState::Installed => closing_sha1(&st.entry, &origin.manual_supplied, on_disk),
            MissingModState::ChosenUnverified => {
                chosen_sha1(&st.entry, &origin.manual_supplied, on_disk)
            }
            _ => None,
        };
        let Some(sha) = sha else { continue };
        if files.contains(&sha) {
            continue;
        }
        let install_path = installed
            .iter()
            .find(|r| r.sha1.eq_ignore_ascii_case(&sha))
            .map(|r| format!("mods/{}", r.filename))
            .or_else(|| {
                assets
                    .iter()
                    .find(|a| a.sha1.eq_ignore_ascii_case(&sha))
                    .map(|a| crate::mods::install::asset_subpath(a.kind, &a.filename))
            })
            .or_else(|| st.entry.meta.install_path.clone())
            .unwrap_or_else(|| format!("mods/{}", st.entry.filename));
        out.push(SuppliedOld {
            key: entry_key(&st.entry),
            md5: present(&st.entry.meta.md5).map(str::to_ascii_lowercase),
            file: crate::mods::installed::PackOriginFile {
                sha1: sha,
                name: st.entry.mod_name.clone(),
                filename: install_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&install_path)
                    .to_string(),
                install_path,
                url: String::new(),
                size: st.entry.size,
                project_id: st.entry.project_id.clone().unwrap_or_default(),
                version_id: st.entry.meta.file_id.clone().unwrap_or_default(),
                env_client: crate::mods::modpack::schema::EnvSupport::Required,
                source: origin.source,
            },
        });
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mods::modpack::schema::{ManualFileMeta, ModpackUnresolvable, UnresolvableReason};

    pub(crate) fn entry(
        file_id: Option<&str>,
        md5: Option<&str>,
        sha1: Option<&str>,
    ) -> ModpackUnresolvable {
        ModpackUnresolvable {
            reason: UnresolvableReason::DistributionDisabled,
            mod_name: "Scape and Run".into(),
            manual_action_url: "https://www.curseforge.com/projects/1".into(),
            filename: "SRP.jar".into(),
            size: 3.0,
            sha1: sha1.map(str::to_string),
            project_id: Some("1".into()),
            meta: ManualFileMeta {
                file_id: file_id.map(str::to_string),
                md5: md5.map(str::to_string),
                ..ManualFileMeta::default()
            },
        }
    }

    #[test]
    fn an_entry_is_keyed_by_file_id_then_md5_then_sha1_then_name() {
        assert_eq!(entry_key(&entry(Some("9"), Some("M"), Some("S"))), "file:9");
        assert_eq!(entry_key(&entry(None, Some("MM"), Some("S"))), "md5:mm");
        assert_eq!(entry_key(&entry(None, None, Some("SS"))), "sha1:ss");
        assert_eq!(
            entry_key(&entry(None, None, None)),
            "name:Scape and Run|srp.jar"
        );
        assert_eq!(
            entry_key(&entry(Some("  "), None, Some("S"))),
            "sha1:s",
            "blank is absent"
        );
    }

    #[test]
    fn filename_stem_strips_the_version_tail() {
        assert_eq!(
            filename_stem("srparasites-1.12.2-2.7.1.jar").as_deref(),
            Some("srparasites")
        );
        assert_eq!(
            filename_stem("jei_1.12.2-4.16.1.302.jar").as_deref(),
            Some("jei")
        );
        assert_eq!(
            filename_stem("RTG-1.12.2-6.1.0.0-snapshot.1.jar").as_deref(),
            Some("rtg")
        );
        // Multi-word leading name is kept whole.
        assert_eq!(
            filename_stem("sodium-extra-0.5.4.jar").as_deref(),
            Some("sodium-extra")
        );
        // A filename starting with a digit yields no stem.
        assert_eq!(filename_stem("2019-mod-1.0.jar"), None);
        // Disabled jars: the .disabled suffix is stripped too.
        assert_eq!(
            filename_stem("srparasites-1.12.2-2.7.1.jar.disabled").as_deref(),
            Some("srparasites")
        );
    }

    #[test]
    fn a_supplied_file_is_described_where_it_actually_is() {
        use crate::mods::installed::PackOrigin;
        use crate::mods::modpack::schema::MissingModStatus;
        use crate::mods::platform::{ContentKind, ModSource};
        let mut e = entry(Some("5"), None, Some("aa"));
        e.filename = "shader.zip".into();
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Curseforge,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![],
            missing_mods: vec![e.clone()],
            skipped_overrides: vec![],
            resolved_missing: vec![],
            manual_supplied: vec![],
            inert_loader_jars: vec![],
        };
        let statuses = vec![MissingModStatus {
            entry: e,
            state: MissingModState::Installed,
        }];
        let assets = vec![crate::mods::assets::make_asset(
            ContentKind::Shader,
            "shader.zip",
            "AA",
            None,
            None,
            None,
            "S",
            String::new(),
        )];
        let on_disk: HashSet<String> = ["aa".to_string()].into();
        let old = supplied_old(&origin, &statuses, &on_disk, &[], &assets);
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].key, "file:5");
        assert_eq!(old[0].file.install_path, "shaderpacks/shader.zip");
        assert_eq!(old[0].file.version_id, "5");
    }
}
