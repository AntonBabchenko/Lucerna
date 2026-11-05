//! Where a supplied file goes, whether that place is free, and the registry
//! row it gets (spec §4.2 steps 4–6).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::mods::modpack::path_safety::{is_safe_filename, is_safe_relative_path};
use crate::mods::modpack::schema::ModpackUnresolvable;
use crate::mods::platform::{ContentKind, InstalledMod, ModSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ext {
    Jar,
    Zip,
}

pub fn ext_of(path: &Path) -> Option<Ext> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "jar" => Some(Ext::Jar),
        "zip" => Some(Ext::Zip),
        _ => None,
    }
}

/// The place for `e`'s file, relative to `.minecraft`, named `name`; `None`
/// when it can't be told (an unsafe name, a non-jar for `mods/`, a zip that is
/// neither a resource pack nor a shader pack). An I/O error reading the zip is
/// "could not tell", returned as `Err`.
pub async fn destination(
    e: &ModpackUnresolvable,
    src: &Path,
    ext: Ext,
    name: &str,
) -> std::io::Result<Option<String>> {
    if !is_safe_filename(name) {
        return Ok(None);
    }
    if let Some(path) = e.meta.install_path.as_deref() {
        let Some((dir, _)) = path.rsplit_once('/') else {
            return Ok(None);
        };
        let rel = format!("{dir}/{name}");
        if !is_safe_relative_path(&rel) || (dir == "mods" && ext != Ext::Jar) {
            return Ok(None);
        }
        return Ok(Some(rel));
    }
    match ext {
        Ext::Jar => Ok(Some(format!("mods/{name}"))),
        Ext::Zip => {
            for (kind, dir) in [
                (ContentKind::ResourcePack, "resourcepacks"),
                (ContentKind::Shader, "shaderpacks"),
            ] {
                let path = src.to_path_buf();
                let is = tokio::task::spawn_blocking(move || {
                    crate::mods::asset_folder::zip_is_pack(&path, kind)
                })
                .await
                .map_err(|j| std::io::Error::other(j.to_string()))??;
                if is {
                    return Ok(Some(format!("{dir}/{name}")));
                }
            }
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Occupancy {
    Free,
    /// These very bytes are already there (either spelling, for a mod).
    Same,
    /// Another file holds the name: what it is called on disk and, for a mod,
    /// its registry row's sha1.
    Taken {
        existing: String,
        row_sha1: Option<String>,
    },
    /// Another of the pack's own files holds the name.
    PackFile {
        existing: String,
    },
}

/// What holds `rel` under `mc` (`.minecraft`) now. A file that cannot be read
/// is an error — never "free".
pub async fn occupancy(
    mc: &Path,
    rel: &str,
    sha1: &str,
    rows: &[InstalledMod],
    pack_files: &HashSet<String>,
) -> std::io::Result<Occupancy> {
    let spellings: Vec<String> = if rel.starts_with("mods/") {
        vec![rel.to_string(), format!("{rel}.disabled")]
    } else {
        vec![rel.to_string()]
    };
    for spelling in spellings {
        let path = mc.join(&spelling);
        let Some(found) = crate::mods::txn::file_sha1(&path).await? else {
            continue;
        };
        let existing = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if found.eq_ignore_ascii_case(sha1) {
            return Ok(Occupancy::Same);
        }
        if pack_files.contains(&found.to_ascii_lowercase()) {
            return Ok(Occupancy::PackFile { existing });
        }
        let base = rel.rsplit('/').next().unwrap_or(rel);
        let row_sha1 = rows
            .iter()
            .find(|r| r.filename.eq_ignore_ascii_case(base))
            .map(|r| r.sha1.clone());
        return Ok(Occupancy::Taken { existing, row_sha1 });
    }
    Ok(Occupancy::Free)
}

/// The registry row for a placed file: a mods row for `mods/*.jar`, an assets
/// row for a resource or shader pack, nothing for any other path. CurseForge
/// ids only for a file checked against a CurseForge hash.
pub async fn register(
    instance_root: &Path,
    rel: &str,
    sha1: &str,
    e: &ModpackUnresolvable,
    verified: bool,
) -> Result<(), Error> {
    let filename = rel.rsplit('/').next().unwrap_or(rel).to_string();
    let cf = verified && e.project_id.is_some();
    let source = cf.then_some(ModSource::Curseforge);
    let project_id = if cf { e.project_id.clone() } else { None };
    let version_id = if cf { e.meta.file_id.clone() } else { None };
    let now = chrono::Utc::now().to_rfc3339();
    if rel.starts_with("mods/") {
        return crate::mods::installed::add(
            instance_root,
            InstalledMod {
                filename,
                sha1: sha1.to_ascii_lowercase(),
                source,
                project_id,
                version_id,
                name: e.mod_name.clone(),
                version_number: None,
                installed_at: now,
                enabled: true,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await;
    }
    if let Some(kind) = crate::mods::install::content_kind_for_install_path(rel) {
        let asset = crate::mods::assets::make_asset(
            kind,
            &filename,
            sha1,
            source,
            project_id,
            version_id,
            &e.mod_name,
            now,
        );
        return crate::mods::assets::add(instance_root, asset).await;
    }
    Ok(())
}

/// `rel` as a path under `.minecraft`.
pub fn under(mc: &Path, rel: &str) -> PathBuf {
    mc.join(rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::manual_downloads::state::tests::entry;
    use std::io::Write;

    fn zip_with(dir: &Path, name: &str, inner: &str) -> PathBuf {
        let p = dir.join(name);
        let mut w = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        w.start_file(inner, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"x").unwrap();
        w.finish().unwrap();
        p
    }

    #[tokio::test]
    async fn the_folder_comes_from_the_source_else_from_the_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = entry(None, None, Some("aa"));
        e.filename = "pack.zip".into();
        let rp = zip_with(dir.path(), "rp.zip", "pack.mcmeta");
        let sh = zip_with(dir.path(), "sh.zip", "shaders/final.fsh");
        let other = zip_with(dir.path(), "o.zip", "readme.txt");
        assert_eq!(
            destination(&e, &rp, Ext::Zip, "pack.zip")
                .await
                .unwrap()
                .as_deref(),
            Some("resourcepacks/pack.zip")
        );
        assert_eq!(
            destination(&e, &sh, Ext::Zip, "pack.zip")
                .await
                .unwrap()
                .as_deref(),
            Some("shaderpacks/pack.zip")
        );
        assert_eq!(
            destination(&e, &other, Ext::Zip, "pack.zip").await.unwrap(),
            None
        );
        e.meta.install_path = Some("mods/pack.zip".into());
        assert_eq!(
            destination(&e, &rp, Ext::Zip, "pack.zip").await.unwrap(),
            None,
            "mods/ takes jars only"
        );
        e.meta.install_path = Some("config/sub/a.cfg".into());
        assert_eq!(
            destination(&e, &rp, Ext::Zip, "a.cfg")
                .await
                .unwrap()
                .as_deref(),
            Some("config/sub/a.cfg")
        );
        assert_eq!(
            destination(&e, &rp, Ext::Jar, "../x.jar").await.unwrap(),
            None,
            "unsafe names refused"
        );
    }

    #[tokio::test]
    async fn an_occupied_place_is_told_apart_from_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mc = dir.path();
        std::fs::create_dir_all(mc.join("mods")).unwrap();
        std::fs::write(mc.join("mods/x.jar.disabled"), b"same").unwrap();
        let same = hex::encode(<sha1::Sha1 as sha1::Digest>::digest(b"same"));
        let none = std::collections::HashSet::new();
        assert!(matches!(
            occupancy(mc, "mods/x.jar", &same, &[], &none)
                .await
                .unwrap(),
            Occupancy::Same
        ));
        assert!(matches!(
            occupancy(mc, "mods/x.jar", "ff", &[], &none).await.unwrap(),
            Occupancy::Taken { .. }
        ));
        let pack: std::collections::HashSet<String> = [same.clone()].into();
        assert!(matches!(
            occupancy(mc, "mods/x.jar", "ff", &[], &pack).await.unwrap(),
            Occupancy::PackFile { .. }
        ));
        assert!(matches!(
            occupancy(mc, "mods/y.jar", "ff", &[], &none).await.unwrap(),
            Occupancy::Free
        ));
    }
}
