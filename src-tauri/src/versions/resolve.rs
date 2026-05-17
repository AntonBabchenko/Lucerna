//! Merge a child VersionDetails (with `inheritsFrom`) into its parent.
//! Used by `versions::install::ensure_version_json` for Fabric/Quilt
//! synthesised ids and (later) Forge.

use crate::versions::version_json::{Arguments, Library, VersionDetails};
use std::collections::HashMap;

/// Single-step merge. The recursion (parent has its own `inheritsFrom`)
/// is handled by the caller — usually `ensure_version_json` which
/// already recurses on the parent id.
pub fn merge_inherits(child: VersionDetails, parent: VersionDetails) -> VersionDetails {
    let mut result = parent.clone();
    result.id = child.id;
    result.main_class = child.main_class;
    result.java_version = child.java_version.or(result.java_version);
    result.minecraft_arguments = child.minecraft_arguments.or(result.minecraft_arguments);
    result.libraries = dedupe_by_maven_coord(parent.libraries, child.libraries);
    result.arguments = merge_arguments(parent.arguments, child.arguments);
    // asset_index, assets, downloads — vanilla parent wins (loaders
    // don't replace assets or the client jar).
    result.inherits_from = None;
    result
}

fn merge_arguments(parent: Option<Arguments>, child: Option<Arguments>) -> Option<Arguments> {
    match (parent, child) {
        (Some(mut p), Some(c)) => {
            p.jvm.extend(c.jvm);
            p.game.extend(c.game);
            Some(p)
        }
        (Some(p), None) => Some(p),
        (None, Some(c)) => Some(c),
        (None, None) => None,
    }
}

/// `parent ++ child` deduped by maven coord. Child wins ties.
///
/// Dedup key:
///   - For entries with NO rules: `group:artifact[:classifier]` (version
///     dropped). This is the common case — child profiles intentionally
///     override parent's plain libraries (e.g. Fabric bumps `asm` from 9.3
///     to 9.6). Child wins.
///   - For entries WITH rules: `group:artifact[:classifier]:version`. Some
///     vanilla MC versions (notably 1.7–1.12) ship multiple library entries
///     with the SAME `group:artifact` but different VERSIONS gated by
///     platform rules — e.g. 1.12.2 ships `lwjgl:2.9.4-nightly-20150209` for
///     Windows/Linux AND `lwjgl:2.9.2-nightly-20140822` for macOS. A
///     version-blind dedup collapses them into the last-inserted entry,
///     which on Windows ends up being the osx-only one → lwjgl missing
///     from classpath → `NoClassDefFoundError: org/lwjgl/opengl/...`.
fn dedupe_by_maven_coord(parent: Vec<Library>, child: Vec<Library>) -> Vec<Library> {
    let mut by_coord: HashMap<String, Library> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for lib in parent.into_iter().chain(child.into_iter()) {
        let coord = coord_of_lib(&lib);
        if !by_coord.contains_key(&coord) {
            order.push(coord.clone());
        }
        by_coord.insert(coord, lib); // later inserts (child) win
    }
    order
        .into_iter()
        .filter_map(|c| by_coord.remove(&c))
        .collect()
}

fn coord_of_lib(lib: &Library) -> String {
    // Modern MC (1.13+) stores LWJGL natives as separate 4-segment library
    // entries (`org.lwjgl:lwjgl-glfw:3.3.2:natives-windows-x86`) — dedup by
    // `group:artifact` alone would collapse 7 entries into one slot.
    // Including the classifier keeps them distinct.
    //
    // Legacy MC (1.7–1.12) sometimes ships multiple entries at the same
    // `group:artifact[:classifier]` but with different VERSIONS, gated by
    // rules. Treat rule-bearing entries as version-distinct.
    let mut parts = lib.name.splitn(4, ':');
    let g = parts.next().unwrap_or("");
    let a = parts.next().unwrap_or("");
    let v = parts.next().unwrap_or("");
    let classifier = parts.next();
    let has_rules = lib.rules.as_ref().is_some_and(|r| !r.is_empty());
    match (classifier, has_rules) {
        (Some(c), true) => format!("{g}:{a}:{c}:{v}"),
        (Some(c), false) => format!("{g}:{a}:{c}"),
        (None, true) => format!("{g}:{a}::{v}"),
        (None, false) => format!("{g}:{a}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::version_json::{
        Argument, ArgumentValue, AssetIndexRef, DownloadEntry, Downloads, JavaVersion, Library,
        Rule, RuleAction,
    };

    fn vanilla_lib(name: &str) -> Library {
        Library {
            name: name.into(),
            downloads: None,
            url: None,
            rules: None,
            natives: None,
        }
    }

    fn vanilla_parent() -> VersionDetails {
        VersionDetails {
            id: "1.20.4".into(),
            inherits_from: None,
            main_class: "net.minecraft.client.main.Main".into(),
            java_version: Some(JavaVersion {
                component: "java-runtime-gamma".into(),
                major_version: 17,
            }),
            asset_index: Some(AssetIndexRef {
                id: "12".into(),
                url: "https://x/12.json".into(),
                sha1: "aaa".into(),
                total_size: Some(100),
                size: 50,
            }),
            assets: Some("12".into()),
            libraries: vec![
                vanilla_lib("net.minecraft:client:1.20.4"),
                vanilla_lib("org.ow2.asm:asm:9.3"),
            ],
            downloads: Some(Downloads {
                client: DownloadEntry {
                    url: "https://x/c.jar".into(),
                    sha1: "ccc".into(),
                    size: 26_000_000,
                },
                other: Default::default(),
            }),
            arguments: Some(Arguments {
                jvm: vec![Argument::Plain("-Xmx2G".into())],
                game: vec![Argument::Plain("--username".into())],
            }),
            minecraft_arguments: None,
        }
    }

    fn fabric_child() -> VersionDetails {
        VersionDetails {
            id: "fabric-loader-0.15.7-1.20.4".into(),
            inherits_from: Some("1.20.4".into()),
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
            java_version: None,
            // Real Fabric loader profiles omit these — vanilla parent provides them.
            asset_index: None,
            assets: None,
            libraries: vec![
                Library {
                    name: "net.fabricmc:fabric-loader:0.15.7".into(),
                    downloads: None,
                    url: Some("https://maven.fabricmc.net/".into()),
                    rules: None,
                    natives: None,
                },
                // Conflicts with parent on coord `org.ow2.asm:asm`
                Library {
                    name: "org.ow2.asm:asm:9.6".into(),
                    downloads: None,
                    url: Some("https://maven.fabricmc.net/".into()),
                    rules: None,
                    natives: None,
                },
            ],
            downloads: None,
            arguments: Some(Arguments {
                jvm: vec![Argument::Plain("-DFabricMcEmu=net.minecraft.client.main.Main".into())],
                game: vec![],
            }),
            minecraft_arguments: None,
        }
    }

    #[test]
    fn child_main_class_wins() {
        let merged = merge_inherits(fabric_child(), vanilla_parent());
        assert_eq!(merged.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
    }

    #[test]
    fn parent_asset_index_wins() {
        let merged = merge_inherits(fabric_child(), vanilla_parent());
        let ai = merged.asset_index.as_ref().expect("vanilla parent supplies assetIndex");
        assert_eq!(ai.id, "12");
        assert_eq!(ai.sha1, "aaa");
        assert_eq!(merged.assets.as_deref().expect("vanilla parent supplies assets"), "12");
        assert_eq!(
            merged.downloads.as_ref().expect("vanilla parent supplies downloads").client.sha1,
            "ccc"
        );
    }

    #[test]
    fn libraries_deduped_by_coord_child_wins() {
        let merged = merge_inherits(fabric_child(), vanilla_parent());
        let names: Vec<&str> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        // Both parent libs come first (in their original order), with
        // the asm version replaced by the child's. Then the loader lib
        // that did not collide.
        assert_eq!(names.len(), 3, "got {names:?}");
        assert!(names.contains(&"net.minecraft:client:1.20.4"));
        assert!(
            names.contains(&"org.ow2.asm:asm:9.6"),
            "child's asm 9.6 must replace parent's 9.3"
        );
        assert!(!names.contains(&"org.ow2.asm:asm:9.3"));
        assert!(names.contains(&"net.fabricmc:fabric-loader:0.15.7"));
    }

    #[test]
    fn arguments_jvm_and_game_appended() {
        let merged = merge_inherits(fabric_child(), vanilla_parent());
        let args = merged.arguments.expect("arguments present");
        // [parent.jvm, child.jvm] order
        match (&args.jvm[0], &args.jvm[1]) {
            (Argument::Plain(a), Argument::Plain(b)) => {
                assert_eq!(a, "-Xmx2G");
                assert!(b.contains("FabricMcEmu"));
            }
            _ => panic!("unexpected jvm shape"),
        }
        // game args: parent only (child had empty)
        assert_eq!(args.game.len(), 1);
        match &args.game[0] {
            Argument::Plain(s) => assert_eq!(s, "--username"),
            _ => panic!("expected plain"),
        }
    }

    #[test]
    fn java_version_child_or_parent() {
        // Child has None → parent wins.
        let merged = merge_inherits(fabric_child(), vanilla_parent());
        assert!(merged.java_version.is_some());
        assert_eq!(merged.java_version.unwrap().component, "java-runtime-gamma");

        // Child sets its own → child wins.
        let mut child = fabric_child();
        child.java_version = Some(JavaVersion {
            component: "java-runtime-delta".into(),
            major_version: 21,
        });
        let merged = merge_inherits(child, vanilla_parent());
        assert_eq!(merged.java_version.unwrap().component, "java-runtime-delta");
    }

    #[test]
    fn id_and_inherits_from_set_correctly() {
        let merged = merge_inherits(fabric_child(), vanilla_parent());
        assert_eq!(merged.id, "fabric-loader-0.15.7-1.20.4");
        assert!(merged.inherits_from.is_none(), "merged form has no further inheritance");
    }

    // ArgumentValue is referenced for visibility — keep this so the
    // import isn't dead.
    #[test]
    fn argument_value_types_exist() {
        let _ = ArgumentValue::Single("x".into());
    }

    /// Regression for the loader-matrix bug: vanilla 1.12.2 manifest ships
    /// two `org.lwjgl.lwjgl:lwjgl` entries with DIFFERENT versions, gated
    /// by platform rules — 2.9.4-nightly-20150209 (allow all, disallow osx)
    /// and 2.9.2-nightly-20140822 (osx only). Version-blind dedup collapses
    /// them to the last-inserted (osx-only) → on Windows the lwjgl jar is
    /// filtered out by `should_install` and `NoClassDefFoundError:
    /// org/lwjgl/opengl/OpenGLException` blows up at MC startup.
    #[test]
    fn dedupe_keeps_rule_bearing_same_coord_different_versions() {
        let osx_only_rule = Rule {
            action: RuleAction::Allow,
            os: Some(crate::versions::version_json::OsRule {
                name: Some("osx".into()),
                version: None,
                arch: None,
            }),
            features: None,
        };
        let non_osx_rules = vec![
            Rule { action: RuleAction::Allow, os: None, features: None },
            Rule {
                action: RuleAction::Disallow,
                os: Some(crate::versions::version_json::OsRule {
                    name: Some("osx".into()),
                    version: None,
                    arch: None,
                }),
                features: None,
            },
        ];
        let lwjgl_main = Library {
            name: "org.lwjgl.lwjgl:lwjgl:2.9.4-nightly-20150209".into(),
            downloads: None,
            url: None,
            rules: Some(non_osx_rules),
            natives: None,
        };
        let lwjgl_osx = Library {
            name: "org.lwjgl.lwjgl:lwjgl:2.9.2-nightly-20140822".into(),
            downloads: None,
            url: None,
            rules: Some(vec![osx_only_rule]),
            natives: None,
        };
        let parent = VersionDetails {
            libraries: vec![lwjgl_main, lwjgl_osx],
            ..vanilla_parent()
        };
        let merged = merge_inherits(fabric_child(), parent);
        let names: Vec<&str> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        assert!(
            names.contains(&"org.lwjgl.lwjgl:lwjgl:2.9.4-nightly-20150209"),
            "Windows/Linux lwjgl entry must survive dedup; got {names:?}"
        );
        assert!(
            names.contains(&"org.lwjgl.lwjgl:lwjgl:2.9.2-nightly-20140822"),
            "OSX lwjgl entry must survive dedup; got {names:?}"
        );
    }

    /// Regression for the slice-12 e2e bug: vanilla 1.20.4 stores LWJGL
    /// natives as separate 4-segment library entries (legacy format),
    /// distinct from the main 3-segment JAR entry. The dedup must keep
    /// them apart — otherwise the main `lwjgl-X.Y.Z.jar` disappears and
    /// `RenderSystem.<clinit>` throws `NoClassDefFoundError` at MC startup.
    #[test]
    fn libraries_classifier_variants_do_not_collide_with_main_jar() {
        let main_jar = Library {
            name: "org.lwjgl:lwjgl-glfw:3.3.2".into(),
            downloads: None,
            url: None,
            rules: None,
            natives: None,
        };
        let natives_win = Library {
            name: "org.lwjgl:lwjgl-glfw:3.3.2:natives-windows".into(),
            downloads: None,
            url: None,
            rules: None,
            natives: None,
        };
        let natives_linux = Library {
            name: "org.lwjgl:lwjgl-glfw:3.3.2:natives-linux".into(),
            downloads: None,
            url: None,
            rules: None,
            natives: None,
        };

        let parent_with_natives = VersionDetails {
            libraries: vec![main_jar, natives_win, natives_linux],
            ..vanilla_parent()
        };
        let merged = merge_inherits(fabric_child(), parent_with_natives);
        let names: Vec<&str> = merged.libraries.iter().map(|l| l.name.as_str()).collect();

        assert!(
            names.contains(&"org.lwjgl:lwjgl-glfw:3.3.2"),
            "main JAR must survive dedup; got {names:?}"
        );
        assert!(
            names.contains(&"org.lwjgl:lwjgl-glfw:3.3.2:natives-windows"),
            "windows natives must survive dedup; got {names:?}"
        );
        assert!(
            names.contains(&"org.lwjgl:lwjgl-glfw:3.3.2:natives-linux"),
            "linux natives must survive dedup; got {names:?}"
        );
    }
}
