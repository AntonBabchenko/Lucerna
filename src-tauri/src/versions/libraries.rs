//! Library rule evaluation + downloads.
//!
//! Mojang's rules array uses allow/disallow logic with OS-name and
//! arch matchers. `should_install` collapses the array to a single
//! boolean. Default action is "install" when there are no rules.

use crate::error::Result;
use crate::network::download_with_sha;
use crate::paths::libraries_dir;
use crate::versions::version_json::{Library, Rule, RuleAction};
use futures_util::stream::{self, StreamExt, TryStreamExt};

/// Concurrent download fan-out width. Matches `assets`/`jre` so the three
/// install phases saturate the network the same way.
const CONCURRENCY: usize = 8;

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

    if let Some(dl) = lib
        .downloads
        .as_ref()
        .filter(|dl| dl.artifact.is_some() || dl.classifiers.is_some())
    {
        // Vanilla path — full downloads block with SHA-1. Empty URL means
        // the artifact is produced locally (e.g. modern-era Forge ships
        // `net.minecraftforge:forge:<mc>-<fv>:client` in version.json with
        // `url=""`; this is the `{PATCHED}` output of binarypatcher, not
        // a downloadable jar). We STILL emit the tuple — it's needed for
        // classpath construction at launch. The download path
        // (`ensure_libraries`) skips empty-URL entries separately.
        if let Some(art) = dl.artifact.as_ref() {
            out.push((
                art.path.clone(),
                art.url.clone(),
                art.sha1.clone(),
                art.size,
            ));
        }
        // Natives — modern Mojang uses `classifiers.<key>` where the key
        // is derived from `natives.<os>` substituted with `${arch}`.
        if let (Some(natives_map), Some(classifiers)) =
            (lib.natives.as_ref(), dl.classifiers.as_ref())
        {
            if let Some(classifier_key) = natives_map.get(os) {
                let key = classifier_key.replace("${arch}", arch);
                if let Some(art) = classifiers.get(&key) {
                    out.push((
                        art.path.clone(),
                        art.url.clone(),
                        art.sha1.clone(),
                        art.size,
                    ));
                }
            }
        }
    } else if let Some(base) = lib.url.as_deref().filter(|s| !s.is_empty()) {
        // Fabric/Quilt path — synthesise maven path from `name`.
        if let Some((rel_path, full_url)) = maven_path_and_url(&lib.name, base) {
            // SHA empty + size 0 → download_with_sha skips verification
            // and Content-Length is treated as advisory only.
            out.push((rel_path, full_url, String::new(), 0));
        }
    } else {
        // Library entry with neither a `downloads.artifact` block nor a
        // `url`. Legacy Forge's `versionInfo.libraries` ships entries
        // like `{"name": "net.minecraft:launchwrapper:1.12", "serverreq": true}`
        // with no download info at all — Forge assumes the file is
        // available from Mojang's library server. The same fallback
        // also covers child-overrides like Forge bumping `guava` to a
        // version vanilla doesn't ship: `dedupe_by_maven_coord` lets
        // the child entry win and the child entry has no downloads
        // block. libraries.minecraft.net hosts every required jar.
        if let Some((rel_path, full_url)) =
            maven_path_and_url(&lib.name, MOJANG_LIBRARIES_FALLBACK_URL)
        {
            out.push((rel_path, full_url, String::new(), 0));
        }
    }

    out
}

const MOJANG_LIBRARIES_FALLBACK_URL: &str = "https://libraries.minecraft.net/";

/// Parse a `group:artifact:version[:classifier][@ext]` Maven coord into the
/// relative maven path and a full URL under `base`.
///
/// Example: `("net.fabricmc:fabric-loader:0.15.7", "https://maven.fabricmc.net/")`
///       → `("net/fabricmc/fabric-loader/0.15.7/fabric-loader-0.15.7.jar",
///           "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.7/fabric-loader-0.15.7.jar")`
///
/// The `@ext` annotation (e.g. `9.3@jar`, `1.0:all@jar`) is stripped — `@jar`
/// is the default artifact type and should not appear in the path or filename.
/// Non-`jar` extensions (e.g. `@zip`) replace the `.jar` suffix.
fn maven_path_and_url(name: &str, base: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0];
    let artifact = parts[1];

    // Strip `@ext` from version (and, if present, classifier).
    // NeoForge installertools 2.1.2 appends `@jar` to every classpath entry,
    // e.g. `org.ow2.asm:asm:9.3@jar`. The `@jar` is the Maven default type
    // and must be stripped before building the path.
    let (version, ext) = strip_at_ext(parts[2]);
    // Optional 4th segment is a classifier (`g:a:v:classifier`); we
    // include it in the filename if present. Fabric/Quilt rarely use
    // classifiers but handle the case for future-proofing.
    // Classifier may also carry an `@ext` suffix.
    let classifier = parts.get(3).map(|c| {
        let (c_stripped, _) = strip_at_ext(c);
        c_stripped.to_string()
    });

    let group_path = group.replace('.', "/");
    let file_name = match classifier.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => format!("{artifact}-{version}-{c}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    let rel_path = format!("{group_path}/{artifact}/{version}/{file_name}");

    let base_trimmed = base.trim_end_matches('/');
    let full_url = format!("{base_trimmed}/{rel_path}");
    Some((rel_path, full_url))
}

/// Strip the `@ext` suffix from a maven coord segment.
/// Returns `(stripped, ext)` where ext defaults to `"jar"`.
/// Examples: `"9.3@jar"` → `("9.3", "jar")`,  `"1.0"` → `("1.0", "jar")`,
///           `"1.0@zip"` → `("1.0", "zip")`.
fn strip_at_ext(s: &str) -> (&str, &str) {
    if let Some(at) = s.rfind('@') {
        let (base, ext) = s.split_at(at);
        let ext = &ext[1..]; // strip the '@'
        (base, if ext.is_empty() { "jar" } else { ext })
    } else {
        (s, "jar")
    }
}

/// Flatten the libraries that should install on the current platform into a
/// single list of `(rel_path, url, sha1, size)` download work-items. Pure —
/// the resolution of rules / artifacts / natives / maven fallback all lives in
/// `artifacts_to_install`; this just concatenates the per-library results so
/// `ensure_libraries` can fan them out concurrently. Each work-item has a
/// distinct `rel_path` (its content-addressed maven path), which is the
/// invariant that makes concurrent direct-to-dest downloads safe.
pub(crate) fn collect_library_downloads(
    libs: &[Library],
    os: &str,
    arch: &str,
) -> Vec<(String, String, String, u64)> {
    // Deduplicate by `rel_path` (the content-addressed maven dest). The
    // resolved list is already path-unique in practice — `dedupe_by_maven_coord`
    // collapses parent/child duplicates upstream — but enforcing it here makes
    // the concurrent direct-to-dest downloads in `ensure_libraries` race-free
    // for ANY caller: two tasks can never target the same file. A maven-coord
    // collision resolves to identical bytes, so keeping the first is correct.
    let mut seen = std::collections::HashSet::new();
    libs.iter()
        .flat_map(|lib| artifacts_to_install(lib, os, arch))
        .filter(|(rel_path, _, _, _)| seen.insert(rel_path.clone()))
        .collect()
}

/// What `ensure_libraries` did with one library, so the caller can build an
/// install-report row without re-deriving any of it. Everything but
/// `downloaded` already comes out of `collect_library_downloads`; only the
/// download closure knows whether bytes actually moved.
// `pub`, not `pub(crate)`, to match `ensure_libraries`' own visibility — a
// public fn returning a crate-private type is a privacy warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryOutcome {
    pub rel_path: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
    /// `false` when the SHA precheck matched, or when a locally-produced
    /// artifact was found already in place.
    pub downloaded: bool,
}

/// Download all libraries that should install on the current platform,
/// up to `CONCURRENCY` at a time. Reuses `network::download_with_sha`.
/// Idempotent — files that exist with matching SHA-1 are skipped. Short-
/// circuits on the first error (in-flight downloads are dropped).
///
/// Returns one [`LibraryOutcome`] per work-item, sorted by `rel_path`, for the
/// install report.
pub async fn ensure_libraries(
    libs: &[Library],
    os: &str,
    arch: &str,
    app: &tauri::AppHandle,
) -> Result<Vec<LibraryOutcome>> {
    let root = libraries_dir(app).map_err(|e| crate::error::Error::io("<libraries_dir>", e))?;
    let downloads = collect_library_downloads(libs, os, arch);

    let app = app.clone();
    let root = std::sync::Arc::new(root);
    let mut outcomes = stream::iter(downloads)
        .map(|(rel_path, url, sha1, size)| {
            let app = app.clone();
            let root = std::sync::Arc::clone(&root);
            async move {
                let dest = root.join(&rel_path);
                let outcome = |downloaded: bool| LibraryOutcome {
                    rel_path: rel_path.clone(),
                    url: url.clone(),
                    sha1: sha1.clone(),
                    size,
                    downloaded,
                };
                if url.is_empty() {
                    // Locally-produced artifact (e.g. modern-era Forge's
                    // `{PATCHED}` client jar =
                    // `net.minecraftforge:forge:<mc>-<fv>:client`). The
                    // published SHA1 in version.json refers to Forge's
                    // reference installer's exact JAR output; our local Java
                    // binarypatcher produces semantically-identical class bytes
                    // but JAR packing (entry order, deflate parameters, zip
                    // timestamps) is non-deterministic across JVMs / runs, so
                    // the file SHA1 won't bytewise-match the reference. Trust
                    // by existence (TOFU) — the install pipeline made it.
                    if tokio::fs::metadata(&dest).await.is_ok() {
                        return Ok(outcome(false));
                    }
                    return Err(crate::error::Error::io(
                        dest.display().to_string(),
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "locally-produced library missing — Forge install may not have completed",
                        ),
                    ));
                }
                if file_matches_sha(&dest, &sha1).await {
                    return Ok(outcome(false));
                }
                download_with_sha(&app, &url, &dest, &sha1, "libraries").await?;
                Ok(outcome(true))
            }
        })
        .buffer_unordered(CONCURRENCY)
        // `try_collect` stops consuming on the first `Err` and drops the
        // remaining in-flight futures (short-circuit), unlike a plain
        // `collect` that would run every download to completion first.
        .try_collect::<Vec<LibraryOutcome>>()
        .await?;
    // `buffer_unordered` yields in completion order, which is a download race.
    // The report must read the same way on every run, so sort by the one field
    // that is unique per work-item — `collect_library_downloads` dedupes on it.
    outcomes.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(outcomes)
}

async fn file_matches_sha(path: &std::path::Path, expected_sha_hex: &str) -> bool {
    if expected_sha_hex.is_empty() {
        // TOFU for Fabric/Quilt loader libs (no per-file checksum in
        // meta). Any pre-existing file is trusted. The first install
        // already fetched it over HTTPS from the documented allowlist
        // host.
        return tokio::fs::metadata(path).await.is_ok();
    }
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
    use crate::versions::version_json::{
        Artifact, Library, LibraryDownloads, OsRule, Rule, RuleAction,
    };

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
            url: None,
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
            url: None,
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
            features: Some(std::collections::HashMap::from([(
                "is_demo_user".to_string(),
                true,
            )])),
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

    #[test]
    fn collect_library_downloads_flattens_all_per_library_artifacts() {
        // A representative mix: a plain vanilla artifact, a fabric-style maven
        // synthesise, and a rule-excluded library (contributes nothing). The
        // flattened list must equal the per-library `artifacts_to_install`
        // concatenation in order — the concurrent fan-out downloads exactly
        // this set, no more, no less.
        let vanilla = lib_no_rules();
        let fabric = Library {
            name: "net.fabricmc:fabric-loader:0.15.7".into(),
            downloads: None,
            url: Some("https://maven.fabricmc.net/".into()),
            rules: None,
            natives: None,
        };
        let excluded = lib_with_rules(vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("linux".into()),
                version: None,
                arch: None,
            }),
            features: None,
        }]);
        let libs = vec![vanilla.clone(), fabric.clone(), excluded.clone()];

        let got = collect_library_downloads(&libs, "windows", "x64");

        let mut expected = Vec::new();
        expected.extend(artifacts_to_install(&vanilla, "windows", "x64"));
        expected.extend(artifacts_to_install(&fabric, "windows", "x64"));
        expected.extend(artifacts_to_install(&excluded, "windows", "x64"));

        assert_eq!(got, expected);
        // Sanity: vanilla (1) + fabric (1) + excluded (0) = 2.
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn library_outcome_carries_every_column_the_report_needs() {
        // A report row needs the path, the URL (for its host), the sha1 and
        // the size. Dropping any of them from `LibraryOutcome` would empty a
        // column of the install report silently rather than fail a build, so
        // pin all four here.
        let outcome = LibraryOutcome {
            rel_path: "net/example/lib/1/lib-1.jar".into(),
            url: "https://libraries.minecraft.net/net/example/lib/1/lib-1.jar".into(),
            sha1: "deadbeef".into(),
            size: 100,
            downloaded: true,
        };
        assert_eq!(outcome.rel_path, "net/example/lib/1/lib-1.jar");
        assert_eq!(outcome.sha1, "deadbeef");
        assert_eq!(outcome.size, 100);
        assert!(outcome.downloaded);
        assert!(outcome.url.starts_with("https://"));
    }

    #[test]
    fn collect_library_downloads_dedups_identical_rel_paths() {
        // Two libraries resolving to the SAME content-addressed dest must
        // collapse to a single download work-item, so the concurrent fan-out
        // never points two tasks at one file. `lib_no_rules` resolves to
        // `test/lib/1/lib-1.jar`; two of them must yield exactly one item.
        let dup_a = lib_no_rules();
        let dup_b = lib_no_rules();
        let libs = vec![dup_a, dup_b];

        let got = collect_library_downloads(&libs, "windows", "x64");

        assert_eq!(got.len(), 1, "duplicate rel_paths must be deduplicated");
        assert_eq!(got[0].0, "test/lib/1/lib-1.jar");
    }

    #[test]
    fn fabric_style_library_synthesises_maven_path_with_empty_sha() {
        let lib = Library {
            name: "net.fabricmc:fabric-loader:0.15.7".into(),
            downloads: None,
            url: Some("https://maven.fabricmc.net/".into()),
            rules: None,
            natives: None,
        };
        let arts = artifacts_to_install(&lib, "windows", "x64");
        assert_eq!(arts.len(), 1);
        let (rel_path, url, sha, size) = &arts[0];
        assert_eq!(
            rel_path,
            "net/fabricmc/fabric-loader/0.15.7/fabric-loader-0.15.7.jar"
        );
        assert_eq!(
            url,
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.7/fabric-loader-0.15.7.jar"
        );
        assert_eq!(sha, "", "Fabric libs have no per-file SHA in meta");
        assert_eq!(*size, 0u64, "size unknown until fetch");
    }

    #[test]
    fn fabric_style_library_with_plus_in_version() {
        let lib = Library {
            name: "net.fabricmc:sponge-mixin:0.13.4+mixin.0.8.5".into(),
            downloads: None,
            url: Some("https://maven.fabricmc.net/".into()),
            rules: None,
            natives: None,
        };
        let arts = artifacts_to_install(&lib, "windows", "x64");
        assert_eq!(arts.len(), 1);
        let (rel_path, url, _, _) = &arts[0];
        assert_eq!(
            rel_path,
            "net/fabricmc/sponge-mixin/0.13.4+mixin.0.8.5/sponge-mixin-0.13.4+mixin.0.8.5.jar"
        );
        assert!(url.ends_with("/sponge-mixin-0.13.4+mixin.0.8.5.jar"));
    }

    #[test]
    fn fabric_style_library_url_without_trailing_slash() {
        let lib = Library {
            name: "net.fabricmc:fabric-loader:0.15.7".into(),
            downloads: None,
            url: Some("https://maven.fabricmc.net".into()), // no trailing slash
            rules: None,
            natives: None,
        };
        let arts = artifacts_to_install(&lib, "windows", "x64");
        assert_eq!(arts.len(), 1);
        // Must produce exactly one slash between base and path.
        assert_eq!(
            arts[0].1,
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.7/fabric-loader-0.15.7.jar"
        );
    }

    #[test]
    fn library_with_no_downloads_and_no_url_falls_back_to_mojang_libraries() {
        // Legacy Forge libraries arrive as `{"name": "g:a:v", "serverreq": true}`
        // with neither a `downloads.artifact` block nor a `url`. Mojang
        // hosts every required jar at libraries.minecraft.net, so we
        // synthesise the URL there. Without this fallback MC fails
        // ClassNotFound on `net.minecraft.launchwrapper.Launch` at boot.
        let lib = Library {
            name: "net.minecraft:launchwrapper:1.12".into(),
            downloads: None,
            url: None,
            rules: None,
            natives: None,
        };
        let out = artifacts_to_install(&lib, "windows", "x64");
        assert_eq!(out.len(), 1, "expected exactly one artifact, got {out:?}");
        let (rel, url, sha, size) = &out[0];
        assert_eq!(
            rel,
            "net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
        );
        assert_eq!(
            url,
            "https://libraries.minecraft.net/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
        );
        // TOFU mode: SHA empty + size 0 → download_with_sha skips
        // verification and Content-Length is treated as advisory.
        assert!(sha.is_empty());
        assert_eq!(*size, 0);
    }

    #[test]
    fn artifacts_to_install_emits_empty_url_tuple_for_locally_produced_artifact() {
        // Modern-era Forge version.json carries the patched-client artifact
        // (`net.minecraftforge:forge:<mc>-<fv>:client`) with an EMPTY url —
        // it's produced locally by the install pipeline, not downloaded.
        // `artifacts_to_install` STILL emits the tuple so that `build_classpath`
        // at launch time gets the path. `ensure_libraries` filters the download
        // attempt itself.
        let lib = Library {
            name: "net.minecraftforge:forge:1.20.4-49.0.49:client".into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(Artifact {
                    path: "net/minecraftforge/forge/1.20.4-49.0.49/forge-1.20.4-49.0.49-client.jar"
                        .into(),
                    url: String::new(), // empty — locally-produced artifact
                    sha1: "bb498cfb18f84b4ef090a8241cae252062d39257".into(),
                    size: 0,
                }),
                classifiers: None,
            }),
            url: None,
            rules: None,
            natives: None,
        };
        let out = artifacts_to_install(&lib, "windows", "x64");
        assert_eq!(
            out.len(),
            1,
            "URL-less artifact must still yield one tuple for classpath; got {out:?}"
        );
        assert_eq!(
            out[0].0,
            "net/minecraftforge/forge/1.20.4-49.0.49/forge-1.20.4-49.0.49-client.jar"
        );
        assert_eq!(
            out[0].1, "",
            "URL is preserved as empty string for downstream filter"
        );
    }

    #[test]
    fn artifacts_to_install_skips_empty_top_level_url() {
        // Defensive: a Library with `url = ""` (empty string, not None) and no
        // `downloads` block. Without the empty-string filter, the fabric-style
        // branch would synthesise a maven URL starting with `/` and download
        // would fail.
        let lib = Library {
            name: "net.minecraftforge:forge:1.20.4-49.0.49:client".into(),
            downloads: None,
            url: Some(String::new()),
            rules: None,
            natives: None,
        };
        let out = artifacts_to_install(&lib, "windows", "x64");
        // Falls through to the libraries.minecraft.net fallback — that's the
        // existing Phase 1 behavior for `url: None`. With `url: Some("")` we
        // also want the fallback path, NOT a `https:///g/path/...` URL.
        assert_eq!(out.len(), 1);
        assert!(
            out[0].1.starts_with("https://libraries.minecraft.net/"),
            "empty url should hit Mojang fallback; got {}",
            out[0].1
        );
    }

    #[tokio::test]
    async fn file_matches_sha_with_empty_expected_trusts_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loader.jar");
        tokio::fs::write(&path, b"any bytes").await.unwrap();
        assert!(file_matches_sha(&path, "").await);
    }

    #[tokio::test]
    async fn file_matches_sha_with_empty_expected_returns_false_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.jar");
        assert!(!file_matches_sha(&path, "").await);
    }
}
