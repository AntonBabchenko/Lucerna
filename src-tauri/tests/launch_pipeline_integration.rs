//! Integration tests for the launch module's testable pieces.
//!
//! `launch::spawn::start` needs a `tauri::AppHandle`, which is not
//! constructable in integration tests. We exercise `build_argv`
//! end-to-end against a realistic 1.20.4 fixture from outside the
//! crate to confirm the public surface is well-formed.

use ftlauncher_lib::accounts::Account;
use ftlauncher_lib::launch::args::{build_argv, build_classpath, ArgvInput};
use ftlauncher_lib::versions::version_json::parse;
use std::path::PathBuf;

const FIXTURE: &str = r#"{
  "id": "1.20.4",
  "mainClass": "net.minecraft.client.main.Main",
  "javaVersion": {"component": "java-runtime-gamma", "majorVersion": 17},
  "assetIndex": {"id": "12", "url": "u", "sha1": "s", "size": 1},
  "assets": "12",
  "downloads": {"client": {"url": "u", "sha1": "s", "size": 1}},
  "libraries": [
    {
      "name": "com.mojang:authlib:3.x",
      "downloads": {"artifact": {"path": "com/mojang/authlib/3.x/authlib.jar", "url": "u", "sha1": "s", "size": 1}}
    }
  ],
  "arguments": {
    "jvm": ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"],
    "game": [
      "--username", "${auth_player_name}",
      "--version", "${version_name}",
      "--gameDir", "${game_directory}",
      "--assetsDir", "${assets_root}",
      "--assetIndex", "${assets_index_name}"
    ]
  }
}"#;

#[test]
fn full_argv_for_1_20_4_has_all_required_pieces() {
    let details = parse(FIXTURE).expect("parse");
    let account = Account {
        id: "of-test-1".into(),
        name: "IntegrationTester".into(),
        uuid: "aaaabbbb-cccc-dddd-eeee-ffff00112233".into(),
        expires_at: None,
    };
    let input = ArgvInput {
        details: &details,
        account: &account,
        java_path: PathBuf::from("C:/jres/java-runtime-gamma/bin/javaw.exe"),
        libraries_dir: PathBuf::from("C:/Users/foo/AppData/Roaming/com.ftlauncher.app/libraries"),
        assets_dir: PathBuf::from("C:/Users/foo/AppData/Roaming/com.ftlauncher.app/assets"),
        natives_dir: PathBuf::from(
            "C:/Users/foo/AppData/Roaming/com.ftlauncher.app/instances/default/natives",
        ),
        game_dir: PathBuf::from(
            "C:/Users/foo/AppData/Roaming/com.ftlauncher.app/instances/default/.minecraft",
        ),
        client_jar: Some(PathBuf::from(
            "C:/Users/foo/AppData/Roaming/com.ftlauncher.app/versions/1.20.4/1.20.4.jar",
        )),
        os: "windows",
        arch: "x64",
    };
    let argv = build_argv(&input).expect("build_argv");

    let main_idx = argv
        .iter()
        .position(|a| a == "net.minecraft.client.main.Main")
        .expect("main class present");
    assert!(main_idx > 0);
    assert!(main_idx < argv.len() - 1);

    assert!(
        argv[0].contains("instances/default/natives")
            || argv[0].contains("instances\\default\\natives"),
        "natives path substituted: {}",
        argv[0]
    );

    let cp_idx = argv.iter().position(|a| a == "-cp").expect("-cp present");
    let cp = &argv[cp_idx + 1];
    assert!(cp.contains("1.20.4.jar"), "client jar in classpath: {cp}");
    assert!(cp.contains("authlib.jar"), "library in classpath: {cp}");

    let game_args: Vec<_> = argv.iter().skip(main_idx + 1).collect();
    let username_idx = game_args
        .iter()
        .position(|a| **a == "--username")
        .expect("--username");
    assert_eq!(*game_args[username_idx + 1], "IntegrationTester");
    let asset_idx = game_args
        .iter()
        .position(|a| **a == "--assetIndex")
        .expect("--assetIndex");
    assert_eq!(*game_args[asset_idx + 1], "12");
}

#[test]
fn classpath_with_one_lib_uses_semicolon_on_windows() {
    let details = parse(FIXTURE).expect("parse");
    let cp = build_classpath(
        &details.libraries,
        std::path::Path::new("C:/libs"),
        Some(std::path::Path::new("C:/client.jar")),
        "windows",
        "x64",
    );
    assert!(cp.contains(";"), "Windows separator present: {cp}");
    assert!(cp.contains("authlib.jar"));
    assert!(
        cp.ends_with("C:/client.jar"),
        "client jar appended last: {cp}"
    );
}
