//! The sha1 of every file a pack's bookkeeping could point at, as the bytes are
//! NOW (spec §3.3). Not the registries' recorded digests: the mods registry
//! keeps a row by file name when the bytes change (`installed::reconcile`), and
//! the assets registry is not reconciled on this path.

use std::collections::HashSet;
use std::path::Path;

use crate::mods::installed::{self, PackOrigin};
use crate::mods::modpack::path_safety::is_safe_relative_path;
use crate::mods::platform::InstalledMod;

pub async fn on_disk_shas(
    instance_root: &Path,
    origin: &PackOrigin,
    rows: &[InstalledMod],
) -> HashSet<String> {
    let mut out = HashSet::new();
    let mods_dir = installed::mods_dir(instance_root);
    for row in rows {
        if let Some(sha) = installed::on_disk_sha1(&mods_dir, &row.filename).await {
            out.insert(sha.to_ascii_lowercase());
        }
    }
    let mc = instance_root.join(".minecraft");
    match crate::mods::assets::list_all(instance_root).await {
        Ok(assets) => {
            for a in assets {
                let rel = crate::mods::install::asset_subpath(a.kind, &a.filename);
                if let Some(sha) = installed::path_sha1(&mc.join(rel)).await {
                    out.insert(sha);
                }
            }
        }
        // Could not tell: their entries read as not satisfied (restrictive).
        Err(e) => crate::diag!("[manual-downloads] assets registry unreadable: {e}"),
    }
    for e in &origin.missing_mods {
        let Some(rel) = e.meta.install_path.as_deref() else {
            continue;
        };
        if rel.starts_with("mods/") || !is_safe_relative_path(rel) {
            continue;
        }
        if let Some(sha) = installed::path_sha1(&mc.join(rel)).await {
            out.insert(sha);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::installed::PackOrigin;
    use crate::mods::platform::ModSource;
    use sha1::{Digest, Sha1};

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha1::digest(b))
    }

    #[tokio::test]
    async fn the_evidence_is_the_bytes_not_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mods = root.join(".minecraft/mods");
        std::fs::create_dir_all(&mods).unwrap();
        // The registry says "aa…", but the jar under that name holds other bytes.
        std::fs::write(mods.join("srp.jar"), b"replaced").unwrap();
        let row = crate::mods::platform::InstalledMod {
            filename: "srp.jar".into(),
            sha1: "aa".into(),
            source: None,
            project_id: None,
            version_id: None,
            name: "SRP".into(),
            version_number: None,
            installed_at: String::new(),
            enabled: true,
            enrich_attempted: false,
            requires: vec![],
        };
        std::fs::create_dir_all(root.join(".minecraft/config")).unwrap();
        std::fs::write(root.join(".minecraft/config/x.cfg"), b"cfg").unwrap();
        let mut entry = crate::mods::manual_downloads::state::tests::entry(None, None, None);
        entry.meta.install_path = Some("config/x.cfg".into());
        let origin = PackOrigin {
            project_id: None,
            source: ModSource::Curseforge,
            project_name: "P".into(),
            version: "1".into(),
            files: vec![],
            missing_mods: vec![entry],
            skipped_overrides: vec![],
            resolved_missing: vec![],
            manual_supplied: vec![],
            inert_loader_jars: vec![],
        };
        let got = on_disk_shas(root, &origin, &[row]).await;
        assert!(got.contains(&sha(b"replaced")));
        assert!(!got.contains("aa"));
        assert!(got.contains(&sha(b"cfg")), "an entry's own path counts");
    }
}
