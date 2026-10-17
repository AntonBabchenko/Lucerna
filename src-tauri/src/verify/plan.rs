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
    /// Who made the file, which decides what a SHA-1 mismatch means.
    pub origin: Origin,
}

/// Who made a planned file. A published file whose bytes differ from its
/// publisher's is corrupt. A Forge processor output is built on this machine,
/// so one that differs from the installer's declared bytes was built
/// differently (`verify::scan::status_for`, spec 2026-10-16 §5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Downloaded from Mojang, a maven repository or a loader's site.
    Published,
    /// Made by a Forge or NeoForge installer's processors (`forge_artifacts`).
    ForgeProcessor,
}

fn opt_url(u: String) -> Option<String> {
    if u.trim().is_empty() {
        None
    } else {
        Some(u)
    }
}

/// The vanilla client jar, always at `versions/<parent_mc>/<parent_mc>.jar`.
/// A named version jar a launch derives from it (`launch::version_jar`) is
/// checked and rebuilt by the launch itself, not planned here.
pub fn client_artifact(effective_id: &str, sha1: &str, url: &str) -> PlannedArtifact {
    let mc = parse_synth_id(effective_id)
        .map(|(_l, _v, mc)| mc)
        .unwrap_or_else(|| effective_id.to_string());
    PlannedArtifact {
        category: VerifyCategory::Client,
        rel_path: format!("{mc}/{mc}.jar"),
        expected_sha: sha1.to_string(),
        url: opt_url(url.to_string()),
        origin: Origin::Published,
    }
}

pub fn library_artifact(rel_path: String, sha1: String, url: String) -> PlannedArtifact {
    PlannedArtifact {
        category: VerifyCategory::Libraries,
        rel_path,
        expected_sha: sha1,
        url: opt_url(url),
        origin: Origin::Published,
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
        origin: Origin::Published,
    }
}

/// Plan all library artefacts for the current platform. The patched-client
/// entry Lucerna <= 0.27 injected is left out: `forge_artifacts` checks that
/// jar through the installer's `PATCHED`, and the injected entry would send a
/// missing jar to a download URL that does not exist.
pub fn library_artifacts(details: &VersionDetails, os: &str, arch: &str) -> Vec<PlannedArtifact> {
    details
        .libraries
        .iter()
        .filter(|lib| !crate::launch::classpath::is_injected_patched_client(lib))
        .flat_map(|lib| artifacts_to_install(lib, os, arch))
        .map(|(rel, url, sha1, _size)| library_artifact(rel, sha1, url))
        .collect()
}

/// What a Forge/NeoForge installer declares about the files its processors
/// make (spec §5.3.3): each declared client output, with its SHA-1, and the
/// patched client jar `data.PATCHED`. That jar is checked against its SHA-1
/// only when a processor declares it, and for presence otherwise (Forge
/// 1.17–1.20.x, NeoForge). Every row is a locally produced library: `url: None`,
/// so a problem leads Repair to a full install, which rebuilds it. Relative to
/// the libraries root.
fn forge_output(rel_path: String, sha1: String) -> PlannedArtifact {
    PlannedArtifact {
        category: VerifyCategory::Libraries,
        rel_path,
        expected_sha: sha1,
        url: None,
        origin: Origin::ForgeProcessor,
    }
}

pub fn forge_artifacts(
    profile: &crate::forge::installer::transitional::InstallProfile,
) -> Result<Vec<PlannedArtifact>, String> {
    use crate::forge::installer::processors::client_declared_outputs;
    let root = std::path::Path::new("");
    let mut out: Vec<PlannedArtifact> = Vec::new();
    for o in client_declared_outputs(profile, root)? {
        let rel = o.path.to_string_lossy().replace('\\', "/");
        if !out.iter().any(|a| a.rel_path == rel) {
            out.push(forge_output(rel, o.sha1));
        }
    }
    if let Some(patched) = profile
        .data
        .get("PATCHED")
        .and_then(|e| e.client.strip_prefix('['))
        .and_then(|s| s.strip_suffix(']'))
        .and_then(crate::forge::patcher::maven_coord_to_relative_path)
    {
        if !out.iter().any(|a| a.rel_path == patched) {
            out.push(forge_output(patched, String::new()));
        }
    }
    Ok(out)
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

    #[test]
    fn library_plan_skips_the_injected_patched_client() {
        let details = crate::versions::version_json::parse(
            r#"{"id":"1.20.1-forge-47.4.10","mainClass":"m","libraries":[
                {"name":"net.minecraftforge:forge:1.20.1-47.4.10:client","url":""}]}"#,
        )
        .unwrap();
        assert!(library_artifacts(&details, "windows", "x64").is_empty());
    }

    fn forge_profile(
        patched_declared: bool,
    ) -> crate::forge::installer::transitional::InstallProfile {
        let outputs = if patched_declared {
            r#""outputs":{"{PATCHED}":"{PATCHED_SHA}"}"#
        } else {
            r#""outputs":{}"#
        };
        crate::forge::installer::transitional::parse_install_profile(&format!(
            r#"{{"spec":1,"profile":"forge","version":"v","minecraft":"1.20.1","data":{{
                "MC_SLIM":{{"client":"[net.minecraft:client:1.20.1-x:slim]","server":"s"}},
                "MC_SLIM_SHA":{{"client":"'aa'","server":"'s'"}},
                "PATCHED":{{"client":"[net.minecraftforge:forge:1.20.1-47.4.10:client]","server":"s"}},
                "PATCHED_SHA":{{"client":"'bb'","server":"'s'"}}}},
              "processors":[
                {{"jar":"j:s:1","args":[],"outputs":{{"{{MC_SLIM}}":"{{MC_SLIM_SHA}}"}}}},
                {{"jar":"j:b:1","args":[],{outputs}}}]}}"#
        ))
        .unwrap()
    }

    #[test]
    fn forge_plan_checks_declared_outputs_and_the_patched_jar() {
        let arts = forge_artifacts(&forge_profile(false)).unwrap();
        let by_path: std::collections::HashMap<_, _> = arts
            .iter()
            .map(|a| (a.rel_path.as_str(), a.expected_sha.as_str()))
            .collect();
        assert_eq!(
            by_path["net/minecraft/client/1.20.1-x/client-1.20.1-x-slim.jar"],
            "aa"
        );
        // PATCHED_SHA exists in data but no processor declares it: presence only.
        assert_eq!(
            by_path["net/minecraftforge/forge/1.20.1-47.4.10/forge-1.20.1-47.4.10-client.jar"],
            ""
        );
        assert!(arts
            .iter()
            .all(|a| a.url.is_none() && a.category == VerifyCategory::Libraries));
        assert!(arts.iter().all(|a| a.origin == Origin::ForgeProcessor));
    }

    #[test]
    fn downloaded_files_are_published_ones() {
        let lib = library_artifact("org/x/y.jar".into(), "aa".into(), "https://l".into());
        assert_eq!(lib.origin, Origin::Published);
        assert_eq!(
            client_artifact("1.20.1", "aa", "https://c").origin,
            Origin::Published
        );
        assert_eq!(asset_artifact("aabb").origin, Origin::Published);
    }

    #[test]
    fn forge_plan_uses_patched_sha_when_a_processor_declares_it() {
        let arts = forge_artifacts(&forge_profile(true)).unwrap();
        let patched = arts
            .iter()
            .find(|a| a.rel_path.ends_with("-client.jar"))
            .unwrap();
        assert_eq!(patched.expected_sha, "bb");
        assert_eq!(arts.len(), 2, "PATCHED planned once");
    }
}
