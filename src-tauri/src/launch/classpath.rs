//! Which files go on the launch `-cp`, in which order, and which version jar ends
//! it (spec 2026-10-16 §3). Pure: no I/O. `launch::version_jar` puts a named
//! version jar on disk.

use crate::error::{Error, Result};
use crate::versions::libraries::artifacts_to_install;
use crate::versions::version_json::{Argument, ArgumentValue, Library, VersionDetails};
use std::path::{Path, PathBuf};

const VERSION_NAME: &str = "${version_name}";

/// Whether the profile refers to its version jar BY NAME: some JVM argument
/// contains `${version_name}`. That is the BootstrapLauncher-era ignore list
/// (`-DignoreList=…,${version_name}.jar`, Forge 1.17–1.20.x and NeoForge
/// 20.x–21.x), which keeps the jar out of the module layer only under that
/// exact file name. Game arguments are not consulted: every profile passes
/// `--version ${version_name}` there. Rules are not consulted either: the
/// question is what the profile expects, not which branch this OS takes.
pub fn names_version_jar(details: &VersionDetails) -> bool {
    let Some(args) = details.arguments.as_ref() else {
        return false;
    };
    args.jvm.iter().any(|arg| match arg {
        Argument::Plain(s) => s.contains(VERSION_NAME),
        Argument::Conditional { value, .. } => match value {
            ArgumentValue::Single(s) => s.contains(VERSION_NAME),
            ArgumentValue::Multiple(ss) => ss.iter().any(|s| s.contains(VERSION_NAME)),
        },
    })
}

/// The jar that ends the launch classpath (spec §3.1). The official launcher
/// always puts `versions/<id>/<id>.jar`, a copy of the vanilla client jar,
/// there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionJar {
    /// The vanilla client jar itself, `versions/<mc>/<mc>.jar`, used when no
    /// profile argument names the jar. Not re-hashed at launch: install and
    /// verify own it.
    Vanilla(PathBuf),
    /// `versions/<install id>/<version_name>.jar`, derived from `source` (the
    /// vanilla jar) at launch, because the profile names it. `sha1` is the
    /// vanilla client's, `None` when the merged JSON carries no
    /// `downloads.client`.
    Named {
        dest: PathBuf,
        source: PathBuf,
        sha1: Option<String>,
    },
}

impl VersionJar {
    /// The path that goes on `-cp`.
    pub fn path(&self) -> &Path {
        match self {
            VersionJar::Vanilla(p) => p,
            VersionJar::Named { dest, .. } => dest,
        }
    }
}

/// Decide the version jar for a merged profile installed as `install_id`
/// (the Lucerna id whose directory holds the merged JSON) on Minecraft `mc`.
/// `details.id` is the installer's own id and becomes a file name, so it must
/// be one safe path segment. Anything else is refused, never joined.
pub fn plan_version_jar(
    details: &VersionDetails,
    versions_dir: &Path,
    install_id: &str,
    mc: &str,
) -> Result<VersionJar> {
    let vanilla = crate::versions::install::version_jar_in(versions_dir, mc);
    if !names_version_jar(details) {
        return Ok(VersionJar::Vanilla(vanilla));
    }
    // The stem is validated, not `<id>.jar`: `is_reserved_windows_name` compares
    // whole names, so `CON.jar` would pass a file-name check that `CON` fails.
    crate::pathsafe::validate_segment(&details.id).map_err(|why| {
        Error::io(
            crate::versions::install::version_json_in(versions_dir, install_id)
                .display()
                .to_string(),
            format!("the profile id {:?} cannot name a file: {why}", details.id),
        )
    })?;
    Ok(VersionJar::Named {
        dest: versions_dir
            .join(install_id)
            .join(format!("{}.jar", details.id)),
        source: vanilla,
        sha1: details.downloads.as_ref().map(|d| d.client.sha1.clone()),
    })
}

/// The patched-client entry Lucerna <= 0.27.x injected into Forge 1.17–1.20.x
/// profiles ("ADDENDUM D", removed by spec 2026-10-16 §3.1). Old cached merged
/// JSONs still carry it. It is recognised by the exact shape only the
/// injection wrote: `net.minecraftforge:forge:<v>:client` with `url: ""` and
/// no `downloads`. Forge 1.20.6+'s own `:client` entry has `downloads` and is
/// kept.
pub fn is_injected_patched_client(lib: &Library) -> bool {
    if lib.downloads.is_some() || lib.url.as_deref() != Some("") {
        return false;
    }
    let parts: Vec<&str> = lib.name.split(':').collect();
    parts.len() == 4
        && parts[0] == "net.minecraftforge"
        && parts[1] == "forge"
        && parts[3] == "client"
}

pub(crate) fn classpath_sep(os: &str) -> &'static str {
    if os == "windows" {
        ";"
    } else {
        ":"
    }
}

/// Every platform-applicable library artifact, in the merged profile's order,
/// then `version_jar` last, joined with the OS separator. Library order is
/// unchanged on purpose (spec §2.1: it does not matter).
pub fn build_classpath(
    libs: &[Library],
    libraries_dir: &Path,
    version_jar: &Path,
    os: &str,
    arch: &str,
) -> String {
    let mut parts: Vec<String> = libs
        .iter()
        .filter(|lib| !is_injected_patched_client(lib))
        .flat_map(|lib| artifacts_to_install(lib, os, arch))
        .map(|(rel_path, _, _, _)| libraries_dir.join(rel_path).to_string_lossy().into_owned())
        .collect();
    parts.push(version_jar.to_string_lossy().into_owned());
    parts.join(classpath_sep(os))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::version_json::parse;

    /// JVM args shaped like each era's real merged profile (trimmed). Only the
    /// fields `names_version_jar` / `plan_version_jar` read are kept.
    fn profile(id: &str, main_class: &str, jvm: &str) -> VersionDetails {
        parse(&format!(
            r#"{{"id":"{id}","mainClass":"{main_class}","libraries":[],
                 "downloads":{{"client":{{"url":"u","sha1":"aa11","size":1}}}},
                 "arguments":{{"jvm":{jvm},"game":["--version","${{version_name}}"]}}}}"#
        ))
        .expect("fixture parses")
    }

    const VANILLA_JVM: &str = r#"["-Djava.library.path=${natives_directory}","-cp","${classpath}",
        {"rules":[{"action":"allow","os":{"name":"windows"}}],"value":"-XX:HeapDumpPath=x"}]"#;
    const FORGE_47_JVM: &str = r#"["-DignoreList=bootstraplauncher,securejarhandler,forge-,${version_name}.jar",
        "-DlibraryDirectory=${library_directory}","-p","${library_directory}/a.jar"]"#;
    const NEOFORGE_21_1_JVM: &str = r#"["-DignoreList=client-extra,${version_name}.jar",
        "-DlibraryDirectory=${library_directory}"]"#;
    const FORGE_52_JVM: &str = r#"["-Djava.net.preferIPv6Addresses=system"]"#;
    const NEOFORGE_21_11_JVM: &str = r#"["-DlibraryDirectory=${library_directory}",
        "--add-opens","java.base/java.lang.invoke=ALL-UNNAMED"]"#;

    #[test]
    fn names_version_jar_true_for_bootstraplauncher_profiles() {
        assert!(names_version_jar(&profile(
            "1.20.1-forge-47.4.10",
            "cpw.mods.bootstraplauncher.BootstrapLauncher",
            FORGE_47_JVM
        )));
        assert!(names_version_jar(&profile(
            "neoforge-21.1.235",
            "cpw.mods.bootstraplauncher.BootstrapLauncher",
            NEOFORGE_21_1_JVM
        )));
    }

    #[test]
    fn names_version_jar_false_when_only_a_game_arg_names_it() {
        // Every profile has `--version ${version_name}` among its GAME args.
        assert!(!names_version_jar(&profile(
            "1.20.1",
            "net.minecraft.client.main.Main",
            VANILLA_JVM
        )));
        assert!(!names_version_jar(&profile(
            "1.21.1-forge-52.1.14",
            "net.minecraftforge.bootstrap.ForgeBootstrap",
            FORGE_52_JVM
        )));
        assert!(!names_version_jar(&profile(
            "neoforge-21.11.45",
            "net.neoforged.fml.startup.Client",
            NEOFORGE_21_11_JVM
        )));
    }

    #[test]
    fn names_version_jar_sees_conditional_values() {
        let p = profile(
            "x",
            "m",
            r#"[{"rules":[{"action":"allow","os":{"name":"osx"}}],
                "value":["-a","-DignoreList=${version_name}.jar"]}]"#,
        );
        assert!(
            names_version_jar(&p),
            "rules do not matter for the naming question"
        );
    }

    #[test]
    fn names_version_jar_false_for_legacy_profile_without_arguments() {
        let p = parse(
            r#"{"id":"1.12.2-forge-14.23.5.2860","mainClass":"net.minecraft.launchwrapper.Launch",
                "libraries":[],"minecraftArguments":"--version ${version_name}"}"#,
        )
        .unwrap();
        assert!(!names_version_jar(&p));
    }

    #[test]
    fn plan_names_the_jar_after_the_profile_id_in_the_install_dir() {
        let p = profile(
            "1.20.1-forge-47.4.10",
            "cpw.mods.bootstraplauncher.BootstrapLauncher",
            FORGE_47_JVM,
        );
        let plan = plan_version_jar(&p, Path::new("V"), "forge-47.4.10-1.20.1", "1.20.1").unwrap();
        assert_eq!(
            plan,
            VersionJar::Named {
                dest: Path::new("V")
                    .join("forge-47.4.10-1.20.1")
                    .join("1.20.1-forge-47.4.10.jar"),
                source: Path::new("V").join("1.20.1").join("1.20.1.jar"),
                sha1: Some("aa11".into()),
            }
        );
    }

    #[test]
    fn plan_uses_the_vanilla_jar_when_the_profile_does_not_name_it() {
        let p = profile(
            "1.21.1-forge-52.1.14",
            "net.minecraftforge.bootstrap.ForgeBootstrap",
            FORGE_52_JVM,
        );
        let plan = plan_version_jar(&p, Path::new("V"), "forge-52.1.14-1.21.1", "1.21.1").unwrap();
        assert_eq!(
            plan,
            VersionJar::Vanilla(Path::new("V").join("1.21.1").join("1.21.1.jar"))
        );
    }

    #[test]
    fn plan_refuses_an_unsafe_profile_id() {
        for bad in ["../evil", "a/b", "a\\b", "C:x", ".hidden", "CON"] {
            // Set after parsing: inside the JSON fixture `a\b` would be an escape.
            let mut p = profile(
                "placeholder",
                "cpw.mods.bootstraplauncher.BootstrapLauncher",
                FORGE_47_JVM,
            );
            p.id = bad.to_string();
            assert!(
                plan_version_jar(&p, Path::new("V"), "forge-x-1.20.1", "1.20.1").is_err(),
                "{bad} must be refused"
            );
        }
    }

    fn lib(json: &str) -> Library {
        serde_json::from_str(json).expect("library fixture parses")
    }

    #[test]
    fn the_injected_entry_is_recognised_by_its_exact_shape() {
        // Exactly what Lucerna <= 0.27 serialised (a real merged 47.4.10 JSON).
        assert!(is_injected_patched_client(&lib(
            r#"{"name":"net.minecraftforge:forge:1.20.1-47.4.10:client","downloads":null,"url":"","rules":null,"natives":null}"#
        )));
        // Forge 1.20.6+'s own entry carries downloads: genuine, keep it.
        assert!(!is_injected_patched_client(&lib(
            r#"{"name":"net.minecraftforge:forge:1.21.1-52.1.14:client","downloads":{"artifact":{"path":"p","url":"","sha1":"s","size":1}},"url":null}"#
        )));
        // Transitional entry: no classifier.
        assert!(!is_injected_patched_client(&lib(
            r#"{"name":"net.minecraftforge:forge:1.16.5-36.2.34","url":""}"#
        )));
        // A NeoForge coordinate was never injected.
        assert!(!is_injected_patched_client(&lib(
            r#"{"name":"net.neoforged:neoforge:21.1.235:client","url":""}"#
        )));
    }

    #[test]
    fn build_classpath_ends_with_the_version_jar_and_drops_the_injected_entry() {
        let libs = vec![
            lib(
                r#"{"name":"a:b:1","downloads":{"artifact":{"path":"a/b/1/b-1.jar","url":"u","sha1":"s","size":1}}}"#,
            ),
            lib(r#"{"name":"net.minecraftforge:forge:1.20.1-47.4.10:client","url":""}"#),
            lib(
                r#"{"name":"net.minecraftforge:forge:1.21.1-52.1.14:client","downloads":{"artifact":{"path":"net/minecraftforge/forge/1.21.1-52.1.14/forge-1.21.1-52.1.14-client.jar","url":"","sha1":"s","size":1}}}"#,
            ),
        ];
        let cp = build_classpath(
            &libs,
            Path::new("L"),
            Path::new("V/x.jar"),
            "windows",
            "x64",
        );
        let parts: Vec<&str> = cp.split(';').collect();
        assert_eq!(parts.len(), 3, "{cp}");
        assert!(parts[0].ends_with("b-1.jar"));
        assert!(
            parts[1].ends_with("forge-1.21.1-52.1.14-client.jar"),
            "genuine :client kept"
        );
        assert_eq!(parts[2], Path::new("V/x.jar").to_string_lossy());
        assert!(!cp.contains("47.4.10"), "injected entry dropped: {cp}");
    }

    #[test]
    fn build_classpath_uses_colon_off_windows() {
        let cp = build_classpath(&[], Path::new("L"), Path::new("v.jar"), "linux", "x64");
        assert_eq!(cp, "v.jar");
    }
}
