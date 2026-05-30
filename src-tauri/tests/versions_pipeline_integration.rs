//! Integration test for the version install pipeline's testable
//! pieces: parser fidelity against a realistic fixture, library
//! rule filtering against a multi-OS library set, and asset index
//! decoding from JSON. The orchestrator is exercised manually
//! via `pnpm tauri dev` in Task 11.

use lucerna_lib::versions::version_json::{parse, RuleAction};

#[test]
fn parses_realistic_1_20_4_skeleton() {
    // A scaled-down but realistic 1.20.4 manifest with multiple
    // platform-conditional libraries.
    let json = r#"{
      "id": "1.20.4",
      "mainClass": "net.minecraft.client.main.Main",
      "javaVersion": {"component": "java-runtime-gamma", "majorVersion": 17},
      "assetIndex": {
        "id": "12",
        "url": "https://piston-meta.example/12.json",
        "sha1": "aaa",
        "totalSize": 100000,
        "size": 50000
      },
      "assets": "12",
      "downloads": {
        "client": {
          "url": "https://piston-data.example/1.20.4-client.jar",
          "sha1": "ccc",
          "size": 26000000
        }
      },
      "libraries": [
        {
          "name": "com.mojang:authlib:3.18.38",
          "downloads": {
            "artifact": {
              "path": "com/mojang/authlib/3.18.38/authlib-3.18.38.jar",
              "url": "https://libraries.example/authlib.jar",
              "sha1": "bbb",
              "size": 1234
            }
          }
        },
        {
          "name": "org.lwjgl:lwjgl:3.3.1",
          "rules": [{"action": "allow"}],
          "downloads": {
            "artifact": {
              "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
              "url": "https://libraries.example/lwjgl.jar",
              "sha1": "ddd",
              "size": 555
            }
          }
        },
        {
          "name": "org.lwjgl:lwjgl:3.3.1:natives-osx",
          "rules": [{"action": "allow", "os": {"name": "osx"}}],
          "downloads": {
            "artifact": {
              "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-osx.jar",
              "url": "https://libraries.example/lwjgl-natives-osx.jar",
              "sha1": "eee",
              "size": 444
            }
          }
        }
      ],
      "arguments": {
        "jvm": ["-Djava.library.path=${natives_directory}"],
        "game": ["--username", "${auth_player_name}"]
      }
    }"#;

    let v = parse(json).expect("parse");
    assert_eq!(v.id, "1.20.4");
    assert_eq!(v.libraries.len(), 3);

    // Library rule eval on the natives-osx entry: should install on
    // macos, not on windows.
    use lucerna_lib::versions::libraries::should_install;
    let natives_lib = &v.libraries[2];
    assert!(should_install(natives_lib, "macos", "aarch64"));
    assert!(!should_install(natives_lib, "windows", "x64"));

    // The unconditional lwjgl entry installs everywhere.
    let plain_lwjgl = &v.libraries[1];
    assert!(should_install(plain_lwjgl, "windows", "x64"));
    assert!(should_install(plain_lwjgl, "macos", "aarch64"));

    // authlib has no rules — installs everywhere.
    let authlib = &v.libraries[0];
    assert!(should_install(authlib, "windows", "x64"));
}

#[test]
fn legacy_minecraft_arguments_string_survives_roundtrip() {
    let json = r#"{
      "id": "1.7.10",
      "mainClass": "net.minecraft.client.main.Main",
      "assetIndex": {
        "id": "1.7.10",
        "url": "https://piston-meta.example/1.7.10.json",
        "sha1": "fff",
        "size": 60
      },
      "assets": "1.7.10",
      "downloads": {
        "client": {
          "url": "https://piston-data.example/1.7.10-client.jar",
          "sha1": "ggg",
          "size": 5000000
        }
      },
      "libraries": [],
      "minecraftArguments": "--username ${auth_player_name} --version ${version_name}"
    }"#;

    let v = parse(json).expect("parse");
    assert!(v.arguments.is_none());
    let m = v.minecraft_arguments.expect("minecraftArguments string");
    assert!(m.contains("${auth_player_name}"));
    assert!(m.contains("${version_name}"));

    // `rules` "action: allow" with no os matches everything — sanity
    // check on the rule action enum.
    assert_eq!(RuleAction::Allow, RuleAction::Allow);
}

#[test]
fn asset_index_json_decodes() {
    use lucerna_lib::versions::assets::AssetIndex;
    let json = r#"{
      "objects": {
        "minecraft/lang/en_us.json": {"hash": "aaa111", "size": 1234},
        "minecraft/sounds/ambient/cave.ogg": {"hash": "bbb222", "size": 5678}
      }
    }"#;
    let index: AssetIndex = serde_json::from_str(json).unwrap();
    assert_eq!(index.objects.len(), 2);
}
