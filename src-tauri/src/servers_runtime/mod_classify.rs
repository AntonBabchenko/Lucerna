//! Pure classifier deciding which of a server's mods are client-only (and
//! should be quarantined) vs. kept. No I/O, no network — the command layer
//! gathers each mod's [`ModFacts`] (offline jar descriptor reads + platform
//! `server_side` metadata) and feeds them here.
//!
//! The decisive property is the **dependency rescue**: a mod that any *kept*
//! mod declares as a required dependency is never quarantined, even if some
//! signal independently marked it client-only. This is the structural reason a
//! shared library like `libraryferret` (a dependency of `bettervillage`) cannot
//! be set aside — preventing the data-loss footgun where removing a flagged
//! "client" mod breaks a real server mod.

use crate::mods::local::ModEnvironment;
pub use crate::mods::platform::ServerSideSupport;
use std::collections::HashSet;

/// Everything the classifier needs about one jar, gathered by the command layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ModFacts {
    pub filename: String,
    /// Offline Fabric/Quilt `environment` (Forge yields `Unknown`).
    pub env: ModEnvironment,
    /// Platform `server_side` metadata; `Unknown` when unavailable.
    pub server_side: ServerSideSupport,
    /// Mod-ids this jar provides — its own id plus JIJ submodule ids.
    pub provides: Vec<String>,
    /// Required dependency mod-ids this jar declares.
    pub required_deps: Vec<String>,
}

/// The classifier's verdict. `quarantine` + `kept` partition every input
/// filename; `kept_because_required` is the subset of `kept` that was rescued
/// only because another kept mod requires it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassifyResult {
    pub quarantine: Vec<String>,
    pub kept: Vec<String>,
    pub kept_because_required: Vec<String>,
}

/// Case- and separator-insensitive id normalization, matching
/// [`crate::mods::dep_resolve`]'s id handling so provides/deps compare
/// cross-source. Shared with [`crate::servers_runtime::quarantine`].
pub(crate) fn norm_id(id: &str) -> String {
    id.trim().to_ascii_lowercase().replace('-', "_")
}

/// Raw per-mod verdict before the dependency-rescue pass. Conservative:
/// anything not confidently client-only is a keep candidate.
fn is_raw_client_only(f: &ModFacts) -> bool {
    match f.server_side {
        ServerSideSupport::Unsupported => true,
        ServerSideSupport::Required | ServerSideSupport::Optional => false,
        // No platform signal — fall back to the (reliable) Fabric/Quilt env.
        ServerSideSupport::Unknown => f.env == ModEnvironment::Client,
    }
}

/// Partition `mods` into quarantine vs. kept.
///
/// 1. Raw verdict per mod ([`is_raw_client_only`]).
/// 2. Dependency rescue to a fixpoint: any raw-client mod that *provides* an id
///    a kept mod *requires* is moved into the kept set (and its own required
///    deps then count for the next pass). Monotone (only client→kept flips),
///    so it terminates in ≤ N passes.
pub fn classify_server_mods(mods: &[ModFacts]) -> ClassifyResult {
    let mut client: Vec<bool> = mods.iter().map(is_raw_client_only).collect();
    let mut rescued: Vec<bool> = vec![false; mods.len()];

    loop {
        // Required dep-ids declared by every currently-kept mod.
        let mut needed: HashSet<String> = HashSet::new();
        for (i, f) in mods.iter().enumerate() {
            if !client[i] {
                for d in &f.required_deps {
                    needed.insert(norm_id(d));
                }
            }
        }
        // Rescue any client mod that provides one of those ids.
        let mut changed = false;
        for (i, f) in mods.iter().enumerate() {
            if client[i] && f.provides.iter().any(|p| needed.contains(&norm_id(p))) {
                client[i] = false;
                rescued[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut out = ClassifyResult::default();
    for (i, f) in mods.iter().enumerate() {
        if client[i] {
            out.quarantine.push(f.filename.clone());
        } else {
            out.kept.push(f.filename.clone());
            if rescued[i] {
                out.kept_because_required.push(f.filename.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(
        filename: &str,
        env: ModEnvironment,
        server_side: ServerSideSupport,
        provides: &[&str],
        required_deps: &[&str],
    ) -> ModFacts {
        ModFacts {
            filename: filename.to_string(),
            env,
            server_side,
            provides: provides.iter().map(|s| s.to_string()).collect(),
            required_deps: required_deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn unsupported_server_side_is_quarantined() {
        let m = facts(
            "betterf3.jar",
            ModEnvironment::Unknown,
            ServerSideSupport::Unsupported,
            &["betterf3"],
            &[],
        );
        let r = classify_server_mods(&[m]);
        assert_eq!(r.quarantine, vec!["betterf3.jar"]);
        assert!(r.kept.is_empty());
    }

    #[test]
    fn required_and_optional_server_side_are_kept() {
        let mods = [
            facts(
                "jei.jar",
                ModEnvironment::Unknown,
                ServerSideSupport::Required,
                &["jei"],
                &[],
            ),
            facts(
                "voicechat.jar",
                ModEnvironment::Unknown,
                ServerSideSupport::Optional,
                &["voicechat"],
                &[],
            ),
        ];
        let r = classify_server_mods(&mods);
        assert!(r.quarantine.is_empty(), "{r:?}");
        assert_eq!(r.kept.len(), 2);
    }

    #[test]
    fn unknown_metadata_falls_back_to_fabric_env() {
        // env=Client with no platform signal → quarantine.
        let client = facts(
            "modmenu.jar",
            ModEnvironment::Client,
            ServerSideSupport::Unknown,
            &["modmenu"],
            &[],
        );
        // env=Both with no platform signal → keep (conservative).
        let both = facts(
            "lib.jar",
            ModEnvironment::Both,
            ServerSideSupport::Unknown,
            &["lib"],
            &[],
        );
        // env=Unknown (Forge, no metadata) → keep (conservative).
        let forge = facts(
            "mystery-forge.jar",
            ModEnvironment::Unknown,
            ServerSideSupport::Unknown,
            &["mystery"],
            &[],
        );
        let r = classify_server_mods(&[client, both, forge]);
        assert_eq!(r.quarantine, vec!["modmenu.jar"]);
        assert!(r.kept.contains(&"lib.jar".to_string()));
        assert!(r.kept.contains(&"mystery-forge.jar".to_string()));
    }

    #[test]
    fn libraryferret_required_by_kept_mod_is_never_quarantined() {
        // The real footgun: a shared lib that some signal flags client-only, but
        // a kept server mod (bettervillage) requires it. Must be kept.
        let bettervillage = facts(
            "bettervillage-forge-1.20.1-3.3.1-all.jar",
            ModEnvironment::Unknown,
            ServerSideSupport::Required,
            &["bettervillage"],
            &["libraryferret"],
        );
        let libraryferret = facts(
            "libraryferret-forge-1.20.1-4.0.0.jar",
            ModEnvironment::Client,         // mis-signalled as client
            ServerSideSupport::Unsupported, // even the platform got it wrong
            &["libraryferret"],
            &[],
        );
        let r = classify_server_mods(&[bettervillage, libraryferret]);
        assert!(
            r.quarantine.is_empty(),
            "libraryferret must be rescued: {r:?}"
        );
        assert!(r
            .kept_because_required
            .contains(&"libraryferret-forge-1.20.1-4.0.0.jar".to_string()));
    }

    #[test]
    fn dependency_rescue_is_transitive_two_hops() {
        // A (kept) requires B (client-marked) requires C (client-marked).
        // Both B and C must be rescued.
        let a = facts(
            "a.jar",
            ModEnvironment::Unknown,
            ServerSideSupport::Required,
            &["a"],
            &["b"],
        );
        let b = facts(
            "b.jar",
            ModEnvironment::Client,
            ServerSideSupport::Unsupported,
            &["b"],
            &["c"],
        );
        let c = facts(
            "c.jar",
            ModEnvironment::Client,
            ServerSideSupport::Unsupported,
            &["c"],
            &[],
        );
        let r = classify_server_mods(&[a, b, c]);
        assert!(r.quarantine.is_empty(), "both hops rescued: {r:?}");
        assert_eq!(r.kept.len(), 3);
        assert!(r.kept_because_required.contains(&"b.jar".to_string()));
        assert!(r.kept_because_required.contains(&"c.jar".to_string()));
    }

    #[test]
    fn separator_and_case_insensitive_dep_match() {
        let dependent = facts(
            "dependent.jar",
            ModEnvironment::Unknown,
            ServerSideSupport::Required,
            &["dependent"],
            &["Some-Lib"],
        );
        let lib = facts(
            "lib.jar",
            ModEnvironment::Client,
            ServerSideSupport::Unsupported,
            &["some_lib"],
            &[],
        );
        let r = classify_server_mods(&[dependent, lib]);
        assert!(
            r.quarantine.is_empty(),
            "case/sep-insensitive rescue: {r:?}"
        );
    }

    #[test]
    fn empty_input_yields_empty_result() {
        let r = classify_server_mods(&[]);
        assert_eq!(r, ClassifyResult::default());
    }

    #[test]
    fn pure_client_pack_quarantines_all() {
        let mods = [
            facts(
                "oculus.jar",
                ModEnvironment::Unknown,
                ServerSideSupport::Unsupported,
                &["oculus"],
                &[],
            ),
            facts(
                "embeddium.jar",
                ModEnvironment::Unknown,
                ServerSideSupport::Unsupported,
                &["embeddium"],
                &[],
            ),
        ];
        let r = classify_server_mods(&mods);
        assert_eq!(r.quarantine.len(), 2);
        assert!(r.kept.is_empty());
    }
}
