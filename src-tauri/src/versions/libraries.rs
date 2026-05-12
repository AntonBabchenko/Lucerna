//! Library rule evaluation + downloads.
//!
//! Mojang's rules array uses allow/disallow logic with OS-name and
//! arch matchers. `should_install` collapses the array to a single
//! boolean. Default action is "install" when there are no rules.

use crate::error::Result;
use crate::network::download_with_sha;
use crate::paths::libraries_dir;
use crate::versions::version_json::{Library, Rule, RuleAction};

/// Evaluate the `rules` array for a library against a (os, arch) pair.
///
/// Algorithm (per Mojang's spec):
/// - No rules → install.
/// - Walk rules in order. Each rule's predicate either matches the
///   current platform or doesn't. If it matches, the rule's action
///   (Allow / Disallow) becomes the new state. Final state at the
///   end of the array decides.
pub fn should_install(lib: &Library, os: &str, arch: &str) -> bool {
    let Some(rules) = lib.rules.as_ref() else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, os, arch) {
            allowed = matches!(rule.action, RuleAction::Allow);
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, os: &str, arch: &str) -> bool {
    // Features are always false for us — Mojang uses them for things
    // like `is_demo_user` which we never set.
    if rule.features.is_some() {
        return false;
    }
    let Some(os_rule) = rule.os.as_ref() else {
        // Rule with no OS predicate matches everything.
        return true;
    };
    if let Some(name) = os_rule.name.as_deref() {
        // Mojang uses both "osx" and "mac" historically; treat both as
        // current Mac. Map our internal "macos" to either.
        let want = name;
        let have = match os {
            "macos" => &["osx", "mac", "macos"][..],
            _ => &[os][..],
        };
        if !have.contains(&want) {
            return false;
        }
    }
    if let Some(want_arch) = os_rule.arch.as_deref() {
        // Mojang uses "x86" for 32-bit, "x64" for 64-bit historically;
        // modern manifests use "x86" for 64-bit too (legacy). We treat
        // anything that isn't "x86" specifically as 64-bit / aarch64.
        if want_arch != arch {
            return false;
        }
    }
    true
}

/// Compute (relative path, url, sha1, size) for the artifacts this
/// library would install on the current platform. Empty if filtered
/// out by rules.
pub fn artifacts_to_install(
    lib: &Library,
    os: &str,
    arch: &str,
) -> Vec<(String, String, String, u64)> {
    if !should_install(lib, os, arch) {
        return vec![];
    }
    let mut out = Vec::with_capacity(2);
    let Some(dl) = lib.downloads.as_ref() else {
        return out;
    };
    if let Some(art) = dl.artifact.as_ref() {
        out.push((art.path.clone(), art.url.clone(), art.sha1.clone(), art.size));
    }
    // Natives — modern Mojang uses `classifiers.<key>` where the key
    // is derived from `natives.<os>` substituted with `${arch}`.
    if let (Some(natives_map), Some(classifiers)) =
        (lib.natives.as_ref(), dl.classifiers.as_ref())
    {
        if let Some(classifier_key) = natives_map.get(os) {
            // Substitute `${arch}` if present.
            let key = classifier_key.replace("${arch}", arch);
            if let Some(art) = classifiers.get(&key) {
                out.push((art.path.clone(), art.url.clone(), art.sha1.clone(), art.size));
            }
        }
    }
    out
}

/// Download all libraries that should install on the current platform.
/// Reuses `network::download_with_sha`. Idempotent — files that exist
/// with matching SHA-1 are skipped.
pub async fn ensure_libraries(
    libs: &[Library],
    os: &str,
    arch: &str,
    app: &tauri::AppHandle,
) -> Result<()> {
    let root = libraries_dir(app).map_err(|e| crate::error::Error::io("<libraries_dir>", e))?;
    for lib in libs {
        for (rel_path, url, sha1, _size) in artifacts_to_install(lib, os, arch) {
            let dest = root.join(&rel_path);
            if file_matches_sha(&dest, &sha1).await {
                continue;
            }
            download_with_sha(app, &url, &dest, &sha1, "libraries").await?;
        }
    }
    Ok(())
}

async fn file_matches_sha(path: &std::path::Path, expected_sha_hex: &str) -> bool {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    use sha1::{Digest, Sha1};
    let got = hex::encode(Sha1::digest(&bytes));
    got == expected_sha_hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::version_json::{Artifact, Library, LibraryDownloads, OsRule, Rule, RuleAction};

    fn lib_with_rules(rules: Vec<Rule>) -> Library {
        Library {
            name: "test:lib:1".into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(Artifact {
                    path: "test/lib/1/lib-1.jar".into(),
                    url: "https://example/lib.jar".into(),
                    sha1: "deadbeef".into(),
                    size: 100,
                }),
                classifiers: None,
            }),
            rules: Some(rules),
            natives: None,
        }
    }

    fn lib_no_rules() -> Library {
        Library {
            name: "test:lib:1".into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(Artifact {
                    path: "test/lib/1/lib-1.jar".into(),
                    url: "https://example/lib.jar".into(),
                    sha1: "deadbeef".into(),
                    size: 100,
                }),
                classifiers: None,
            }),
            rules: None,
            natives: None,
        }
    }

    #[test]
    fn no_rules_installs_everywhere() {
        let lib = lib_no_rules();
        assert!(should_install(&lib, "windows", "x64"));
        assert!(should_install(&lib, "linux", "x64"));
        assert!(should_install(&lib, "macos", "aarch64"));
    }

    #[test]
    fn allow_windows_rejects_linux() {
        let lib = lib_with_rules(vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("windows".into()),
                version: None,
                arch: None,
            }),
            features: None,
        }]);
        assert!(should_install(&lib, "windows", "x64"));
        assert!(!should_install(&lib, "linux", "x64"));
    }

    #[test]
    fn disallow_osx_blocks_mac() {
        let lib = lib_with_rules(vec![
            Rule {
                action: RuleAction::Allow,
                os: None,
                features: None,
            },
            Rule {
                action: RuleAction::Disallow,
                os: Some(OsRule {
                    name: Some("osx".into()),
                    version: None,
                    arch: None,
                }),
                features: None,
            },
        ]);
        assert!(should_install(&lib, "windows", "x64"));
        assert!(should_install(&lib, "linux", "x64"));
        assert!(!should_install(&lib, "macos", "aarch64"));
    }

    #[test]
    fn features_rule_never_matches() {
        // Mojang's `is_demo_user` rules — we never set features so they
        // shouldn't match.
        let lib = lib_with_rules(vec![Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some(std::collections::HashMap::from([
                ("is_demo_user".to_string(), true),
            ])),
        }]);
        // No rule matched → not allowed (stays at the default false).
        assert!(!should_install(&lib, "windows", "x64"));
    }

    #[test]
    fn artifacts_to_install_filters_out_excluded() {
        let lib = lib_with_rules(vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".into()),
                version: None,
                arch: None,
            }),
            features: None,
        }]);
        assert!(artifacts_to_install(&lib, "windows", "x64").is_empty());
        assert_eq!(artifacts_to_install(&lib, "linux", "x64").len(), 1);
    }
}
