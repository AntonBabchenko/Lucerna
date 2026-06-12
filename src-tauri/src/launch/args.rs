//! Build the final argv passed to `java(.exe)` from the parsed
//! per-version JSON + account + resolved paths.
//!
//! Handles both schemas:
//! - 1.13+ : `details.arguments.{jvm,game}` arrays with plain or
//!   rule-conditional entries.
//! - pre-1.13 : `details.minecraft_arguments` single space-separated
//!   game-args string; JVM args we synthesise ourselves.

use crate::accounts::{Account, AccountKind};
use crate::error::Result;
use crate::launch::quick_play::QuickPlay;
use crate::versions::libraries::artifacts_to_install;
use crate::versions::version_json::{
    Argument, ArgumentValue, Library, Rule, RuleAction, VersionDetails,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const LAUNCHER_NAME: &str = "Lucerna";

pub struct ArgvInput<'a> {
    pub details: &'a VersionDetails,
    pub account: &'a Account,
    pub java_path: PathBuf,
    pub libraries_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub natives_dir: PathBuf,
    pub game_dir: PathBuf,
    /// Vanilla MC client jar to append to the classpath. `None` for
    /// modern Forge / NeoForge installs that ship a patched MC in their
    /// libraries — adding vanilla there duplicates the MC bytecode and
    /// triggers a JPMS ResolutionException on the modern Java 9+ module
    /// path bootstrap. Legacy-era Forge (launchwrapper) and
    /// Vanilla / Fabric / Quilt do get the vanilla jar.
    pub client_jar: Option<PathBuf>,
    pub os: &'static str,
    pub arch: &'static str,
    /// Optional direct-launch target. When set, the matching quick-play
    /// feature args are enabled and `${quickPlayName}` / `${quickPlayMultiplayer}`
    /// are substituted. `None` = launch to the main menu (default).
    pub quick_play: Option<&'a QuickPlay>,
}

/// Build the full argv. Returns `[..jvm_args, main_class, ..game_args]`.
pub fn build_argv(input: &ArgvInput) -> Result<Vec<String>> {
    let classpath = build_classpath(
        &input.details.libraries,
        &input.libraries_dir,
        input.client_jar.as_deref(),
        input.os,
        input.arch,
    );
    let subs = substitution_map(input, &classpath)?;

    let features = enabled_features(input.quick_play);

    let (jvm, game) = match (&input.details.arguments, &input.details.minecraft_arguments) {
        (Some(args), _) => {
            let jvm = walk_arguments(&args.jvm, &subs, input.os, input.arch, &features);
            let game = walk_arguments(&args.game, &subs, input.os, input.arch, &features);
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
) -> crate::error::Result<HashMap<&'static str, String>> {
    let mut m = HashMap::new();
    m.insert("auth_player_name", input.account.name.clone());
    m.insert("auth_uuid", input.account.uuid.replace('-', ""));

    // Auth tokens by account kind. Offline = "0" / "legacy" placeholder
    // (MC accepts these in offline mode). Microsoft = real MC access
    // token from the keychain + user_type "msa".
    match input.account.kind {
        AccountKind::Offline => {
            m.insert("auth_access_token", "0".into());
            m.insert("auth_session", "0".into());
            m.insert("user_type", "legacy".into());
        }
        AccountKind::Microsoft => {
            let mc_token = crate::accounts::keychain::retrieve(
                &crate::accounts::keychain::mc_access_key(&input.account.id),
            )?
            .ok_or_else(|| crate::error::Error::AuthFailed {
                stage: "launch".into(),
                details: "MC access token missing from keychain — sign in again".into(),
            })?;
            m.insert("auth_access_token", mc_token.clone());
            m.insert("auth_session", mc_token);
            m.insert("user_type", "msa".into());
        }
    }
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
        input.details.assets.clone().ok_or_else(|| {
            crate::error::Error::io(
                "<version_json>",
                "merged JSON missing assets — upstream schema change?",
            )
        })?,
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
    match input.quick_play {
        Some(QuickPlay::Singleplayer { world }) => {
            m.insert("quickPlayName", world.clone());
        }
        Some(QuickPlay::Multiplayer { address }) => {
            m.insert("quickPlayMultiplayer", address.clone());
        }
        None => {}
    }
    Ok(m)
}

fn classpath_sep(os: &str) -> &'static str {
    if os == "windows" {
        ";"
    } else {
        ":"
    }
}

/// Join all platform-applicable library artifact paths with the OS's
/// classpath separator. Optionally appends `client_jar` — pass `None`
/// for modern Forge / NeoForge installs whose libraries already include
/// a patched MC (adding vanilla there duplicates the bytecode and on
/// the modern Java module-path bootstrap triggers a JPMS
/// ResolutionException for `net.minecraft.*` packages).
pub fn build_classpath(
    libs: &[Library],
    libraries_dir: &Path,
    client_jar: Option<&Path>,
    os: &str,
    arch: &str,
) -> String {
    let sep = classpath_sep(os);
    let mut parts: Vec<String> = libs
        .iter()
        .flat_map(|lib| artifacts_to_install(lib, os, arch))
        .map(|(rel_path, _, _, _)| libraries_dir.join(rel_path).to_string_lossy().into_owned())
        .collect();
    if let Some(cj) = client_jar {
        parts.push(cj.to_string_lossy().into_owned());
    }
    parts.join(sep)
}

/// The quick-play feature keys to enable for a given target. We enable ONLY
/// these — every other feature (`is_demo_user`, …) stays unmatched, so the
/// existing "features rules are dropped" behavior holds for everything else.
fn enabled_features(quick_play: Option<&QuickPlay>) -> Vec<&'static str> {
    match quick_play {
        Some(QuickPlay::Singleplayer { .. }) => vec!["is_quick_play_singleplayer"],
        Some(QuickPlay::Multiplayer { .. }) => vec!["is_quick_play_multiplayer"],
        None => vec![],
    }
}

/// Whether `details` carries any quick-play feature-gated game arg. The
/// honest "does this version support Quick Play" signal — robust across
/// `1.20.4`, `26.1.2`, snapshots, and all loaders (which inherit the vanilla
/// game args). Used by the `instance_quick_play_support` command.
pub fn details_has_quick_play(details: &VersionDetails) -> bool {
    let Some(args) = details.arguments.as_ref() else {
        return false;
    };
    args.game.iter().any(|arg| match arg {
        Argument::Conditional { rules, .. } => rules.iter().any(|r| {
            r.features.as_ref().is_some_and(|f| {
                f.contains_key("is_quick_play_singleplayer")
                    || f.contains_key("is_quick_play_multiplayer")
            })
        }),
        Argument::Plain(_) => false,
    })
}

fn walk_arguments(
    args: &[Argument],
    subs: &HashMap<&'static str, String>,
    os: &str,
    arch: &str,
    features: &[&str],
) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            Argument::Plain(s) => out.push(substitute(s, subs)),
            Argument::Conditional { rules, value } => {
                if rules_match(rules, os, arch, features) {
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

/// Evaluate the conditional `rules` array. Mojang's semantics: walk in
/// order, each matching rule's action becomes the state. Default state is
/// false. `features` rules match only when every `(key, true)` they request
/// is in `enabled_features` — we pass only quick-play keys, so all other
/// feature rules stay unmatched.
pub fn rules_match(rules: &[Rule], os: &str, arch: &str, enabled_features: &[&str]) -> bool {
    let mut allowed = false;
    for rule in rules {
        if rule_matches_one(rule, os, arch, enabled_features) {
            allowed = matches!(rule.action, RuleAction::Allow);
        }
    }
    allowed
}

fn rule_matches_one(rule: &Rule, os: &str, arch: &str, enabled_features: &[&str]) -> bool {
    // A features rule is decided solely on features: it matches iff every
    // `(key, true)` it requests is enabled. Mojang's quick-play / demo rules
    // carry only a `features` map (no `os`), so returning here is correct;
    // we never enable `is_demo_user`, so `--demo` stays dropped.
    if let Some(feats) = rule.features.as_ref() {
        return feats
            .iter()
            .all(|(key, &want)| want && enabled_features.contains(&key.as_str()));
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

/// Resolve `${name}` placeholders in `s` from `subs` with a single
/// left-to-right pass. Each placeholder is resolved exactly once: a value
/// substituted in is appended verbatim and never re-scanned, so a value that
/// happens to contain `${other}` (e.g. a player named `${classpath}`) is not
/// re-expanded. Unknown placeholders are left literal. Deterministic
/// regardless of `subs` iteration order.
fn substitute(s: &str, subs: &HashMap<&'static str, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match subs.get(name) {
                    // Resolve once — do NOT re-scan the substituted value.
                    Some(value) => out.push_str(value),
                    // Unknown placeholder: leave it literal.
                    None => {
                        out.push_str("${");
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            // No closing brace: emit the rest verbatim and stop.
            None => {
                out.push_str("${");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Floor for the JVM max-heap (MB). Below this, the JVM may refuse to start;
/// `0` or an absurd `u32::MAX` from a stale/hostile instance file would
/// otherwise produce a cryptic launch failure.
const MIN_HEAP_MB: u32 = 512;

/// Upper bound (bytes) on the user's `extra_jvm_args` blob passed to the JVM.
const MAX_JVM_ARGS_LEN: usize = 4096;

/// Clamp a requested max-heap (MB) into a launchable range: at least
/// [`MIN_HEAP_MB`], and never above physical RAM when it is known. Applied at
/// the spawn site so it also covers values persisted before this guard
/// existed. `requested.max(MIN).min(ram)` (not `clamp`) so a tiny-RAM system
/// where `ram < MIN` does not panic on inverted bounds.
pub(crate) fn clamp_heap_mb(requested: u32, total_ram_mb: Option<u64>) -> u32 {
    let floored = requested.max(MIN_HEAP_MB);
    match total_ram_mb {
        Some(ram) => floored.min(ram.min(u32::MAX as u64) as u32),
        None => floored,
    }
}

/// Split the user's `extra_jvm_args` into argv tokens, dropping any token that
/// contains a control character (never legitimate in a JVM flag) and capping
/// the total length so an unbounded blob cannot be passed to the process.
///
/// Limitation: this splits on whitespace and does NOT honour shell-style
/// quoting, so a path containing spaces cannot be expressed as a single arg.
/// That is a documented constraint of the free-text field, not fixed here.
pub(crate) fn sanitize_jvm_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut dropped = 0usize;
    for tok in raw.split_whitespace() {
        // Control chars (NUL, ESC, …) are never valid in a JVM flag — a token
        // carrying one is a paste error or injection attempt; drop it.
        if tok.chars().any(|c| c.is_control()) {
            dropped += 1;
            continue;
        }
        // Check before pushing so `out` never exceeds the cap. Skip (not
        // break) an over-cap token so smaller later tokens can still fit.
        if total + tok.len() + 1 > MAX_JVM_ARGS_LEN {
            dropped += 1;
            continue;
        }
        total += tok.len() + 1;
        out.push(tok.to_string());
    }
    // Dropping is silent in the argv. Emit a launcher diagnostic to stderr so
    // the reason is recoverable when running from a console / dev build —
    // matching the eprintln! diagnostics used elsewhere in the launch path.
    // (This is a developer/console signal, not the in-app log viewer.)
    if dropped > 0 {
        eprintln!(
            "launch: dropped {dropped} extra_jvm_args token(s) (control chars or {MAX_JVM_ARGS_LEN}-byte cap)"
        );
    }
    out
}

/// pre-1.13 path: synthesise the minimal JVM args + split the
/// `minecraftArguments` string into game args.
fn legacy_argv(mc_args: &str, subs: &HashMap<&'static str, String>) -> (Vec<String>, Vec<String>) {
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
            kind: AccountKind::Offline,
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
          "--userType", "${user_type}",
          "--assetsDir", "${assets_root}",
          "--assetIndex", "${assets_index_name}",
          {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"},
          {"rules": [{"action": "allow", "features": {"is_quick_play_singleplayer": true}}], "value": ["--quickPlaySingleplayer", "${quickPlayName}"]},
          {"rules": [{"action": "allow", "features": {"is_quick_play_multiplayer": true}}], "value": ["--quickPlayMultiplayer", "${quickPlayMultiplayer}"]}
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
            client_jar: Some(PathBuf::from("C:/versions/1.20.4/1.20.4.jar")),
            os: "windows",
            arch: "x64",
            quick_play: None,
        }
    }

    fn input_qp<'a>(
        details: &'a VersionDetails,
        account: &'a Account,
        quick_play: Option<&'a QuickPlay>,
    ) -> ArgvInput<'a> {
        ArgvInput {
            quick_play,
            ..input(details, account)
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
    fn quick_play_singleplayer_emits_world_arg() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        let acct = account();
        let qp = QuickPlay::Singleplayer {
            world: "My World".into(),
        };
        let argv = build_argv(&input_qp(&details, &acct, Some(&qp))).expect("build");
        let idx = argv
            .iter()
            .position(|a| a == "--quickPlaySingleplayer")
            .expect("--quickPlaySingleplayer present");
        assert_eq!(argv[idx + 1], "My World");
        assert!(!argv.iter().any(|a| a == "--quickPlayMultiplayer"));
        assert!(!argv.iter().any(|a| a == "--demo"));
    }

    #[test]
    fn quick_play_multiplayer_emits_address_arg() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        let acct = account();
        let qp = QuickPlay::Multiplayer {
            address: "mc.example.net:25566".into(),
        };
        let argv = build_argv(&input_qp(&details, &acct, Some(&qp))).expect("build");
        let idx = argv
            .iter()
            .position(|a| a == "--quickPlayMultiplayer")
            .expect("--quickPlayMultiplayer present");
        assert_eq!(argv[idx + 1], "mc.example.net:25566");
        assert!(!argv.iter().any(|a| a == "--quickPlaySingleplayer"));
    }

    #[test]
    fn no_quick_play_target_emits_no_quick_play_args() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        let acct = account();
        let argv = build_argv(&input_qp(&details, &acct, None)).expect("build");
        assert!(!argv.iter().any(|a| a == "--quickPlaySingleplayer"));
        assert!(!argv.iter().any(|a| a == "--quickPlayMultiplayer"));
        assert!(!argv.iter().any(|a| a == "--demo"));
    }

    #[test]
    fn details_has_quick_play_true_for_120_fixture() {
        let details = parse(FIXTURE_1_20_4).expect("parse");
        assert!(details_has_quick_play(&details));
    }

    #[test]
    fn details_has_quick_play_false_for_legacy_fixture() {
        let details = parse(FIXTURE_1_7_10).expect("parse");
        assert!(!details_has_quick_play(&details));
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
        assert!(!rules_match(&[], "windows", "x64", &[]));
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
        assert!(!rules_match(&[rule], "windows", "x64", &[]));
    }

    #[test]
    fn build_classpath_windows_separator() {
        let libs = vec![];
        let cp = build_classpath(
            &libs,
            Path::new("C:/libs"),
            Some(Path::new("C:/client.jar")),
            "windows",
            "x64",
        );
        assert_eq!(cp, "C:/client.jar");
    }

    #[test]
    fn build_classpath_skips_client_jar_when_none() {
        // Forge / NeoForge installs pass None to avoid duplicating the
        // patched MC bytecode on the JPMS module path.
        let libs = vec![];
        let cp = build_classpath(&libs, Path::new("C:/libs"), None, "windows", "x64");
        assert_eq!(cp, "");
    }

    #[test]
    fn build_argv_errors_when_assets_missing() {
        let mut details = parse(FIXTURE_1_20_4).expect("parse");
        details.assets = None;
        let acct = account();
        let r = build_argv(&input(&details, &acct));
        assert!(r.is_err(), "expected Err when details.assets = None");
    }

    #[test]
    fn build_argv_microsoft_account_reads_mc_access_token_from_keychain() {
        let ms_account = Account {
            id: "ms-test-account-1".into(),
            kind: AccountKind::Microsoft,
            name: "Notch".into(),
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            expires_at: Some(2_000_000_000.0),
        };
        // Pre-populate the keychain (in-memory backend in tests) with the
        // MC access token that the substitution map should pick up.
        crate::accounts::keychain::store(
            &crate::accounts::keychain::mc_access_key(&ms_account.id),
            "expected-mc-token-xyz",
        )
        .unwrap();

        let details = parse(FIXTURE_1_20_4).expect("parse");
        let argv = build_argv(&input(&details, &ms_account)).expect("build");

        // Argv must contain --accessToken expected-mc-token-xyz
        let access_idx = argv
            .iter()
            .position(|a| a == "--accessToken")
            .expect("--accessToken present");
        assert_eq!(argv[access_idx + 1], "expected-mc-token-xyz");
        // user_type must be msa for Microsoft accounts
        let user_type_idx = argv
            .iter()
            .position(|a| a == "--userType")
            .expect("--userType present");
        assert_eq!(argv[user_type_idx + 1], "msa");

        // Cleanup
        crate::accounts::keychain::delete(&crate::accounts::keychain::mc_access_key(
            &ms_account.id,
        ))
        .unwrap();
    }

    #[test]
    fn build_argv_microsoft_account_without_keychain_entry_errors() {
        let ms_account = Account {
            id: "ms-no-keychain".into(),
            kind: AccountKind::Microsoft,
            name: "Notch".into(),
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            expires_at: Some(2_000_000_000.0),
        };
        // No keychain entry — substitution must error, not silently use "0".

        let details = parse(FIXTURE_1_20_4).expect("parse");
        let result = build_argv(&input(&details, &ms_account));
        assert!(
            matches!(
                result,
                Err(crate::error::Error::AuthFailed { ref stage, .. }) if stage == "launch"
            ),
            "expected AuthFailed{{stage=launch}}, got {:?}",
            result
        );
    }

    // ---- substitute: single-pass, no re-expansion ---------------------------

    fn subs(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn substitute_resolves_known_and_leaves_unknown_literal() {
        let m = subs(&[("auth_player_name", "Steve")]);
        assert_eq!(substitute("hi ${auth_player_name}", &m), "hi Steve");
        // Unknown placeholder stays verbatim.
        assert_eq!(substitute("x ${nope} y", &m), "x ${nope} y");
    }

    #[test]
    fn substitute_does_not_reexpand_injected_placeholder() {
        // A player literally named "${classpath}" must NOT cause the classpath
        // to be expanded into the resolved value.
        let m = subs(&[
            ("auth_player_name", "${classpath}"),
            ("classpath", "/secret/cp"),
        ]);
        assert_eq!(substitute("${auth_player_name}", &m), "${classpath}");
    }

    #[test]
    fn substitute_is_deterministic_and_handles_unclosed() {
        let m = subs(&[("a", "1"), ("b", "2")]);
        assert_eq!(substitute("${a}-${b}-${a}", &m), "1-2-1");
        // Unclosed `${` is emitted verbatim.
        assert_eq!(substitute("pre ${unclosed", &m), "pre ${unclosed");
        assert_eq!(substitute("none", &m), "none");
    }

    // ---- clamp_heap_mb ------------------------------------------------------

    #[test]
    fn clamp_heap_mb_floors_and_caps() {
        // Zero / below floor → floor.
        assert_eq!(clamp_heap_mb(0, Some(8192)), 512);
        assert_eq!(clamp_heap_mb(256, Some(8192)), 512);
        // In range → unchanged.
        assert_eq!(clamp_heap_mb(4096, Some(8192)), 4096);
        // Above RAM → RAM.
        assert_eq!(clamp_heap_mb(u32::MAX, Some(8192)), 8192);
        // Unknown RAM → floor only, no upper cap.
        assert_eq!(clamp_heap_mb(0, None), 512);
        assert_eq!(clamp_heap_mb(16384, None), 16384);
        // Tiny system where RAM < floor → RAM (no panic on inverted bounds).
        assert_eq!(clamp_heap_mb(2048, Some(256)), 256);
    }

    // ---- sanitize_jvm_args --------------------------------------------------

    #[test]
    fn sanitize_jvm_args_keeps_plain_and_drops_control_chars() {
        assert_eq!(
            sanitize_jvm_args("-XX:+UseG1GC -Xss512k"),
            vec!["-XX:+UseG1GC".to_string(), "-Xss512k".to_string()]
        );
        // A token with an embedded control char (e.g. NUL) is dropped.
        assert_eq!(
            sanitize_jvm_args("-Dgood=1 -Dbad=\u{0}x -Dok=2"),
            vec!["-Dgood=1".to_string(), "-Dok=2".to_string()]
        );
        assert!(sanitize_jvm_args("").is_empty());
        assert!(sanitize_jvm_args("   \t  ").is_empty());
    }

    #[test]
    fn sanitize_jvm_args_caps_total_length() {
        // Many tokens whose cumulative length exceeds the 4096-byte cap are
        // truncated rather than passed through unbounded.
        let blob = (0..1000)
            .map(|i| format!("-Dk{i}=v"))
            .collect::<Vec<_>>()
            .join(" ");
        let out = sanitize_jvm_args(&blob);
        assert!(
            out.len() < 1000,
            "expected truncation, got {} tokens",
            out.len()
        );
        // The cap check runs before each push, so the emitted total never
        // exceeds MAX_JVM_ARGS_LEN.
        let total: usize = out.iter().map(|t| t.len() + 1).sum();
        assert!(
            total <= MAX_JVM_ARGS_LEN,
            "emitted {total} bytes, cap {MAX_JVM_ARGS_LEN}"
        );
    }
}
