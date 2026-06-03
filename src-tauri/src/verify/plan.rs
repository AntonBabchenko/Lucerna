//! Pure construction of the expected-artefact list from merged version
//! details. No filesystem, no network — reuses the same planners the
//! install pipeline uses so verify checks exactly what launch reads.

use crate::verify::VerifyCategory;
use crate::versions::libraries::artifacts_to_install;
use crate::versions::loaders::parse_synth_id;
use crate::versions::version_json::VersionDetails;

/// One file verify will hash. Paths are relative to the artefact's root
/// dir (client/libraries → versions or libraries dir; assets → objects dir).
#[derive(Debug, Clone)]
pub struct PlannedArtifact {
    pub category: VerifyCategory,
    /// Relative path under the category's root.
    pub rel_path: String,
    /// Empty string ⇒ presence-only (no authoritative SHA).
    pub expected_sha: String,
    /// `None` ⇒ locally produced / not downloadable.
    pub url: Option<String>,
}

fn opt_url(u: String) -> Option<String> {
    if u.trim().is_empty() {
        None
    } else {
        Some(u)
    }
}

/// The vanilla client jar, always at `versions/<parent_mc>/<parent_mc>.jar`
/// (never duplicated under a synth dir — v0.4.1 invariant).
pub fn client_artifact(effective_id: &str, sha1: &str, url: &str) -> PlannedArtifact {
    let mc = parse_synth_id(effective_id)
        .map(|(_l, _v, mc)| mc)
        .unwrap_or_else(|| effective_id.to_string());
    PlannedArtifact {
        category: VerifyCategory::Client,
        rel_path: format!("{mc}/{mc}.jar"),
        expected_sha: sha1.to_string(),
        url: opt_url(url.to_string()),
    }
}

pub fn library_artifact(rel_path: String, sha1: String, url: String) -> PlannedArtifact {
    PlannedArtifact {
        category: VerifyCategory::Libraries,
        rel_path,
        expected_sha: sha1,
        url: opt_url(url),
    }
}

pub fn asset_artifact(hash: &str) -> PlannedArtifact {
    // Objects are stored at `<objects>/<2hex>/<full-hash>` and named by hash.
    let prefix = &hash[..2.min(hash.len())];
    PlannedArtifact {
        category: VerifyCategory::Assets,
        rel_path: format!("{prefix}/{hash}"),
        expected_sha: hash.to_string(),
        // url is None by design: asset problems are repaired via a full install_version (ensure_assets), not a targeted download — see repair.rs.
        url: None,
    }
}

/// Plan all library artefacts for the current platform.
pub fn library_artifacts(details: &VersionDetails, os: &str, arch: &str) -> Vec<PlannedArtifact> {
    details
        .libraries
        .iter()
        .flat_map(|lib| artifacts_to_install(lib, os, arch))
        .map(|(rel, url, sha1, _size)| library_artifact(rel, sha1, url))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::VerifyCategory;

    #[test]
    fn client_planned_at_parent_mc_path_for_synth() {
        let p = client_artifact("fabric-loader-0.16.5-1.20.4", "deadbeef", "https://c");
        assert_eq!(p.category, VerifyCategory::Client);
        assert_eq!(p.rel_path, "1.20.4/1.20.4.jar");
        assert_eq!(p.expected_sha, "deadbeef");
    }

    #[test]
    fn client_planned_at_mc_path_for_vanilla() {
        let p = client_artifact("1.20.4", "deadbeef", "https://c");
        assert_eq!(p.rel_path, "1.20.4/1.20.4.jar");
    }

    #[test]
    fn empty_sha_library_is_presence_only() {
        let p = library_artifact(
            "net/fabricmc/x.jar".into(),
            String::new(),
            "https://l".into(),
        );
        assert!(p.expected_sha.is_empty());
        assert_eq!(p.category, VerifyCategory::Libraries);
    }

    #[test]
    fn empty_url_library_has_no_url() {
        let p = library_artifact("forge/patched.jar".into(), "aa".into(), String::new());
        assert!(p.url.is_none());
    }
}
