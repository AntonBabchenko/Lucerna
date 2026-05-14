//! Build the final argv passed to `java(.exe)` from the parsed
//! per-version JSON + account + resolved paths.
//!
//! Handles both schemas:
//! - 1.13+ : `details.arguments.{jvm,game}` arrays with plain or
//!   rule-conditional entries.
//! - pre-1.13 : `details.minecraft_arguments` single space-separated
//!   game-args string; JVM args we synthesise ourselves.

use crate::accounts::Account;
use crate::error::Result;
use crate::versions::libraries::artifacts_to_install;
use crate::versions::version_json::{
    Argument, ArgumentValue, Library, Rule, RuleAction, VersionDetails,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const LAUNCHER_NAME: &str = "FTlauncher";

pub struct ArgvInput<'a> {
    pub details: &'a VersionDetails,
    pub account: &'a Account,
    pub java_path: PathBuf,
    pub libraries_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub natives_dir: PathBuf,
    pub game_dir: PathBuf,
    pub client_jar: PathBuf,
    pub os: &'static str,
    pub arch: &'static str,
}

/// Build the full argv. Returns `[..jvm_args, main_class, ..game_args]`.
pub fn build_argv(input: &ArgvInput) -> Result<Vec<String>> {
    let classpath = build_classpath(
        &input.details.libraries,
        &input.libraries_dir,
        &input.client_jar,
        input.os,
        input.arch,
    );
    let subs = substitution_map(input, &classpath);

    let (jvm, game) = match (&input.details.arguments, &input.details.minecraft_arguments) {
        (Some(args), _) => {
            let jvm = walk_arguments(&args.jvm, &subs, input.os, input.arch);
            let game = walk_arguments(&args.game, &subs, input.os, input.arch);
            (jvm, game)
        }
        (None, Some(mc_args)) => legacy_argv(mc_args, &subs),
        (None, None) => (synth_jvm(&subs), vec![]),
    };

    let mut out = jvm;
    out.push(input.details.main_class.clone());
    out.extend(game);
    Ok(out)
}

fn substitution_map<'a>(
    input: &'a ArgvInput<'a>,
    classpath: &str,
) -> HashMap<&'static str, String> {
    let mut m = HashMap::new();
    m.insert("auth_player_name", input.account.name.clone());
    m.insert("auth_uuid", input.account.uuid.replace('-', ""));

    // Offline auth: token is "0", user_type is "legacy". Microsoft auth was
    // implemented and deferred — see git tag `v0.2.0-msauth-attempt`.
    m.insert("auth_access_token", "0".into());
    m.insert("auth_session", "0".into());
    m.insert("user_type", "legacy".into());
    m.insert("user_properties", "{}".into());
    m.insert("version_name", input.details.id.clone());
    m.insert("version_type", "release".into());
    m.insert(
        "game_directory",
        input.game_dir.to_string_lossy().into_owned(),
    );
    m.insert(
        "assets_root",
        input.assets_dir.to_string_lossy().into_owned(),
    );
    m.insert(
        "game_assets",
        input.assets_dir.to_string_lossy().into_owned(),
    );
    // After ensure_version_json, assets and asset_index are always Some
    // (vanilla parent provides them; loader profiles inherit via merge_inherits).
    m.insert(
        "assets_index_name",
        input
            .details
            .assets
            .clone()
            .expect("merged JSON should have assets — vanilla parent must provide it"),
    );
    m.insert("classpath", classpath.to_string());
    m.insert(
        "natives_directory",
        input.natives_dir.to_string_lossy().into_owned(),
    );
    m.insert("launcher_name", LAUNCHER_NAME.into());
    m.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    m.insert("classpath_separator", classpath_sep(input.os).into());
    m.insert(
        "library_directory",
        input.libraries_dir.to_string_lossy().into_owned(),
    );
    m
}

fn classpath_sep(os: &str) -> &'static str {
    if os == "windows" {
        ";"
    } else {
        ":"
    }
}

/// Join all platform-applicable library artifact paths plus the client
/// jar with the OS's classpath separator.
pub fn build_classpath(
    libs: &[Library],
    libraries_dir: &Path,
    client_jar: &Path,
    os: &str,
    arch: &str,
) -> String {
    let sep = classpath_sep(os);
    let mut parts: Vec<String> = libs
        .iter()
        .flat_map(|lib| artifacts_to_install(lib, os, arch))
        .map(|(rel_path, _, _, _)| {
            libraries_dir.join(rel_path).to_string_lossy().into_owned()
        })
        .collect();
    parts.push(client_jar.to_string_lossy().into_owned());
    parts.join(sep)
}

fn walk_arguments(
    args: &[Argument],
    subs: &HashMap<&'static str, String>,
    os: &str,
    arch: &str,
) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            Argument::Plain(s) => out.push(substitute(s, subs)),
            Argument::Conditional { rules, value } => {
                if rules_match(rules, os, arch) {
                    match value {
                        ArgumentValue::Single(s) => out.push(substitute(s, subs)),
                        ArgumentValue::Multiple(ss) => {
                            out.extend(ss.iter().map(|s| substitute(s, subs)));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Evaluate the conditional `rules` array. Mojang's semantics: walk
/// in order, each matching rule's action becomes the state. Default
/// state is false. `features` rules NEVER match (we don't set them).
pub fn rules_match(rules: &[Rule], os: &str, arch: &str) -> bool {
    let mut allowed = false;
    for rule in rules {
        if rule_matches_one(rule, os, arch) {
            allowed = matches!(rule.action, RuleAction::Allow);
        }
    }
    allowed
}

fn rule_matches_one(rule: &Rule, os: &str, arch: &str) -> bool {
    // Features rules → never match (we never set is_demo_user etc.).
    if rule.features.is_some() {
        return false;
    }
    let Some(os_rule) = rule.os.as_ref() else {
        return true;
    };
    if let Some(name) = os_rule.name.as_deref() {
        let have = match os {
            "macos" => &["osx", "mac", "macos"][..],
            _ => &[os][..],
        };
        if !have.contains(&name) {
            return false;
        }
    }
    if let Some(want_arch) = os_rule.arch.as_deref() {
        if want_arch != arch {
            return false;
        }
    }
    true
}

fn substitute(s: &str, subs: &HashMap<&'static str, String>) -> String {
    let mut out = s.to_string();
    for (key, value) in subs {
        let needle = format!("${{{key}}}");
        if out.contains(&needle) {
            out = out.replace(&needle, value);
        }
    }
    out
}

/// pre-1.13 path: synthesise the minimal JVM args + split the
/// `minecraftArguments` string into game args.
fn legacy_argv(
    mc_args: &str,
    subs: &HashMap<&'static str, String>,
) -> (Vec<String>, Vec<String>) {
    let jvm = synth_jvm(subs);
    let game = mc_args
        .split_whitespace()
        .map(|w| substitute(w, subs))
        .collect();
    (jvm, game)
}

fn synth_jvm(subs: &HashMap<&'static str, String>) -> Vec<String> {
    vec![
        format!(
            "-Djava.library.path={}",
            subs.get("natives_directory").cloned().unwrap_or_default()
        ),
        "-cp".into(),
        subs.get("classpath").cloned().unwrap_or_default(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::version_json::parse;

    fn account() -> Account {
        Account {
            id: "of-test".into(),
            name: "TestPlayer".into(),
            uuid: "12345678-1234-1234-1234-123456789abc".into(),
            expires_at: None,
        }
    }

    const FIXTURE_1_20_4: &str = r#"{
      "id": "1.20.4",
      "mainClass": "net.minecraft.client.main.Main",
      "javaVersion": {"component": "java-runtime-gamma", "majorVersion": 17},
      "assetIndex": {"id": "12", "url": "https://example/", "sha1": "x", "size": 1},
      "assets": "12",
      "downloads": {"client": {"url": "https://example/", "sha1": "y", "size": 1}},
      "libraries": [
        {
          "name": "com.mojang:authlib:3.x",
          "downloads": {"artifact": {"path": "com/mojang/authlib/3.x/authlib.jar", "url": "u", "sha1": "s", "size": 1}}
        }
      ],
      "arguments": {
        "jvm": [
          "-Djava.library.path=${natives_directory}",
          "-cp",
          "${classpath}",
          {"rules": [{"action": "allow", "os": {"name": "osx"}}], "value": "-XstartOnFirstThread"},
          {"rules": [{"action": "allow", "os": {"name": "windows"}}], "value": ["-Dos.name=Windows", "-Dos.version=10"]}
        ],
        "game": [
          "--username", "${auth_player_name}",
          "--version", "${version_name}",
          "--uuid", "${auth_uuid}",
          "--accessToken", "${auth_access_token}",
          "--assetsDir", "${assets_root}",
          "--assetIndex", "${assets_index_name}",
          {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"}
        ]
      }
    }"#;

    const FIXTURE_1_7_10: &str = r#"{
      "id": "1.7.10",
      "mainClass": "net.minecraft.client.main.Main",
      "assetIndex": {"id": "1.7.10", "url": "u", "sha1": "x", "size": 1},
      "assets": "1.7.10",
      "downloads": {"client": {"url": "u", "sha1": "y", "size": 1}},
      "libraries": [],
      "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --uuid ${auth_uuid} --accessToken ${auth_access_token}"
    }"#;

    fn input<'a>(details: &'a VersionDetails, account: &'a Account) -> ArgvInput<'a> {
        ArgvInput {
            details,
            account,
            java_path: PathBuf::from("C:/jres/java.exe"),
            libraries_dir: PathBuf::from("C:/libs"),
            assets_dir: PathBuf::from("C:/assets"),
            natives_dir: PathBuf::from("C:/instance/natives"),
            game_dir: PathBuf::from("C:/instance/.minecraft"),
            client_jar: PathBuf::from("C:/versions/1.20.4/1.20.4.jar"),
            os: "windows",
            arch: "x64",
        }
    }

    #[test]
    fn build_argv_modern_substitutes_player_uuid_classpath() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        let acct = account();
        let argv = build_argv(&input(&details, &acct)).expect("build");
        let main_idx = argv
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .expect("main class");
        assert!(main_idx > 0);
        assert!(main_idx < argv.len() - 1);

        let username_idx = argv
            .iter()
            .position(|a| a == "--username")
            .expect("--username");
        assert_eq!(argv[username_idx + 1], "TestPlayer");

        let uuid_idx = argv.iter().position(|a| a == "--uuid").expect("--uuid");
        assert_eq!(argv[uuid_idx + 1], "12345678123412341234123456789abc");

        let token_idx = argv
            .iter()
            .position(|a| a == "--accessToken")
            .expect("--accessToken");
        assert_eq!(argv[token_idx + 1], "0");

        let cp_idx = argv.iter().position(|a| a == "-cp").expect("-cp");
        let cp = &argv[cp_idx + 1];
        assert!(cp.contains("1.20.4.jar"), "client jar in classpath: {cp}");
        assert!(cp.contains("authlib.jar"), "library in classpath: {cp}");
        assert!(cp.contains(";"), "Windows classpath separator: {cp}");
    }

    #[test]
    fn build_argv_conditional_windows_jvm_args_present() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        let acct = account();
        let argv = build_argv(&input(&details, &acct)).expect("build");
        assert!(
            argv.iter().any(|a| a == "-Dos.name=Windows"),
            "windows conditional JVM arg present: {argv:?}"
        );
        assert!(
            argv.iter().any(|a| a == "-Dos.version=10"),
            "windows conditional JVM arg present: {argv:?}"
        );
    }

    #[test]
    fn build_argv_osx_conditional_skipped_on_windows() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        let acct = account();
        let argv = build_argv(&input(&details, &acct)).expect("build");
        assert!(
            !argv.iter().any(|a| a == "-XstartOnFirstThread"),
            "osx-only conditional must NOT appear on windows: {argv:?}"
        );
    }

    #[test]
    fn build_argv_features_rule_drops_arg() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        let acct = account();
        let argv = build_argv(&input(&details, &acct)).expect("build");
        assert!(
            !argv.iter().any(|a| a == "--demo"),
            "demo flag must not appear: {argv:?}"
        );
    }

    #[test]
    fn build_argv_legacy_synthesises_jvm_and_splits_game_args() {
        let details = parse(FIXTURE_1_7_10).expect("parse");
        let acct = account();
        let argv = build_argv(&input(&details, &acct)).expect("build");
        assert!(argv.iter().any(|a| a.starts_with("-Djava.library.path=")));
        assert!(argv.iter().any(|a| a == "-cp"));
        assert!(argv.iter().any(|a| a == "net.minecraft.client.main.Main"));
        assert!(argv.iter().any(|a| a == "TestPlayer"));
        assert!(argv.iter().any(|a| a == "1.7.10"));
    }

    #[test]
    fn rules_match_no_rules_returns_false() {
        assert!(!rules_match(&[], "windows", "x64"));
    }

    #[test]
    fn rules_match_features_rule_never_matches() {
        let rule = Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some(std::collections::HashMap::from([(
                "is_demo_user".to_string(),
                true,
            )])),
        };
        assert!(!rules_match(&[rule], "windows", "x64"));
    }

    #[test]
    fn build_classpath_windows_separator() {
        let libs = vec![];
        let cp = build_classpath(
            &libs,
            Path::new("C:/libs"),
            Path::new("C:/client.jar"),
            "windows",
            "x64",
        );
        assert_eq!(cp, "C:/client.jar");
    }
}
