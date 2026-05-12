//! Parser for Mojang's per-version JSON
//! (`piston-meta.mojang.com/v1/packages/.../<id>.json`).
//!
//! Schema differs across MC eras:
//! - **1.13+**: `arguments.jvm` + `arguments.game` are arrays of either
//!   plain strings or rule-conditional objects.
//! - **pre-1.13**: `minecraftArguments` is a single space-separated
//!   string; JVM args are implicit (the launcher synthesises them).
//!
//! We parse both into a unified `VersionDetails` shape. The caller
//! (launch::args, slice 6) walks the unified shape; this parser
//! doesn't know about argument substitution.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionDetails {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    /// Java component name — `"java-runtime-gamma"`, etc. Used by
    /// slice 5 (JRE). Absent for very old MC versions.
    #[serde(rename = "javaVersion", default)]
    pub java_version: Option<JavaVersion>,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,
    /// `assets` field — usually equals `assetIndex.id` but historically
    /// differed in `legacy` / `pre-1.6`. Drives the assets directory layout.
    pub assets: String,
    pub libraries: Vec<Library>,
    pub downloads: Downloads,
    /// 1.13+ — present alongside `minecraftArguments` for transition
    /// versions; we prefer this when present.
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// pre-1.13 — single space-separated string. Fallback when
    /// `arguments` is None.
    #[serde(rename = "minecraftArguments", default)]
    pub minecraft_arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub url: String,
    pub sha1: String,
    #[serde(rename = "totalSize", default)]
    pub total_size: Option<u64>,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Downloads {
    pub client: DownloadEntry,
    /// Optional — `client_mappings`, `server`, `server_mappings` exist
    /// in modern manifests but we don't need them.
    #[serde(flatten, default)]
    pub other: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DownloadEntry {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    /// Native classifier mapping (legacy — Forge etc. still use this).
    /// `{ "linux": "natives-linux", "windows": "natives-windows", ... }`
    #[serde(default)]
    pub natives: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    /// Per-OS native artifacts (1.13+ style).
    #[serde(default)]
    pub classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Artifact {
    /// Maven-style relative path, e.g.
    /// `com/mojang/authlib/3.0/authlib-3.0.jar`.
    pub path: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<OsRule>,
    /// 1.13+ also allows `features` here — we ignore them (always
    /// false unless we explicitly set them, which we don't).
    #[serde(default)]
    pub features: Option<std::collections::HashMap<String, bool>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    /// `"windows"`, `"linux"`, `"osx"`, `"mac"`.
    #[serde(default)]
    pub name: Option<String>,
    /// Regex on `System.getProperty("os.version")` — we ignore this
    /// for v0.1.0 (Mojang rarely uses it).
    #[serde(default)]
    pub version: Option<String>,
    /// `"x86"`, `"x64"`, `"arm"`, etc.
    #[serde(default)]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Arguments {
    #[serde(default)]
    pub jvm: Vec<Argument>,
    #[serde(default)]
    pub game: Vec<Argument>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Argument {
    /// Plain string argument.
    Plain(String),
    /// Rule-gated argument.
    Conditional {
        rules: Vec<Rule>,
        /// Single string OR array of strings.
        value: ArgumentValue,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Single(String),
    Multiple(Vec<String>),
}

pub fn parse(json: &str) -> Result<VersionDetails, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed 1.20.4 fixture — real ~50 KB → ~1 KB here. We keep only
    /// fields the parser must accept; the rest is irrelevant for parse
    /// correctness.
    const FIXTURE_1_20_4: &str = r#"{
      "id": "1.20.4",
      "mainClass": "net.minecraft.client.main.Main",
      "javaVersion": {"component": "java-runtime-gamma", "majorVersion": 17},
      "assetIndex": {
        "id": "12",
        "url": "https://piston-meta.example/12.json",
        "sha1": "aaa",
        "totalSize": 100,
        "size": 50
      },
      "assets": "12",
      "downloads": {
        "client": {
          "url": "https://piston-data.example/client.jar",
          "sha1": "ccc",
          "size": 26000000
        }
      },
      "libraries": [
        {
          "name": "com.mojang:authlib:3.x",
          "downloads": {
            "artifact": {
              "path": "com/mojang/authlib/3.x/authlib-3.x.jar",
              "url": "https://libraries.example/authlib.jar",
              "sha1": "bbb",
              "size": 1234
            }
          }
        },
        {
          "name": "org.lwjgl:lwjgl:3.3.1:natives-windows",
          "rules": [{"action": "allow", "os": {"name": "windows"}}],
          "downloads": {
            "artifact": {
              "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar",
              "url": "https://libraries.example/lwjgl-natives.jar",
              "sha1": "ddd",
              "size": 555
            }
          }
        }
      ],
      "arguments": {
        "jvm": [
          "-Djava.library.path=${natives_directory}",
          {
            "rules": [{"action": "allow", "os": {"name": "osx"}}],
            "value": ["-XstartOnFirstThread"]
          }
        ],
        "game": [
          "--username", "${auth_player_name}",
          "--version", "${version_name}"
        ]
      }
    }"#;

    const FIXTURE_1_7_10: &str = r#"{
      "id": "1.7.10",
      "mainClass": "net.minecraft.client.main.Main",
      "assetIndex": {
        "id": "1.7.10",
        "url": "https://piston-meta.example/1.7.10.json",
        "sha1": "eee",
        "size": 60
      },
      "assets": "1.7.10",
      "downloads": {
        "client": {
          "url": "https://piston-data.example/1.7.10-client.jar",
          "sha1": "fff",
          "size": 5000000
        }
      },
      "libraries": [
        {"name": "net.sf.jopt-simple:jopt-simple:4.5"}
      ],
      "minecraftArguments": "--username ${auth_player_name} --version ${version_name}"
    }"#;

    #[test]
    fn parses_1_20_4_modern_format() {
        let v = parse(FIXTURE_1_20_4).expect("parse 1.20.4");
        assert_eq!(v.id, "1.20.4");
        assert_eq!(v.main_class, "net.minecraft.client.main.Main");
        assert_eq!(v.java_version.as_ref().unwrap().component, "java-runtime-gamma");
        assert_eq!(v.asset_index.id, "12");
        assert_eq!(v.libraries.len(), 2);
        assert!(v.arguments.is_some());
        assert!(v.minecraft_arguments.is_none());
    }

    #[test]
    fn parses_1_7_10_legacy_format() {
        let v = parse(FIXTURE_1_7_10).expect("parse 1.7.10");
        assert_eq!(v.id, "1.7.10");
        assert!(v.java_version.is_none(), "pre-1.7.10 has no javaVersion field");
        assert!(v.arguments.is_none());
        let m = v.minecraft_arguments.as_ref().expect("minecraftArguments string");
        assert!(m.contains("${auth_player_name}"));
        assert_eq!(v.libraries.len(), 1);
        assert!(v.libraries[0].downloads.is_none(), "legacy libs have no downloads block");
    }

    #[test]
    fn argument_conditional_with_string_array_value() {
        let v = parse(FIXTURE_1_20_4).expect("parse");
        let args = v.arguments.expect("arguments");
        // Second JVM arg is conditional with array value
        match &args.jvm[1] {
            Argument::Conditional { rules, value } => {
                assert_eq!(rules.len(), 1);
                assert_eq!(rules[0].action, RuleAction::Allow);
                match value {
                    ArgumentValue::Multiple(v) => assert_eq!(v, &vec!["-XstartOnFirstThread".to_string()]),
                    ArgumentValue::Single(_) => panic!("expected array"),
                }
            }
            Argument::Plain(_) => panic!("expected conditional"),
        }
    }

    #[test]
    fn library_rules_allow_windows_only() {
        let v = parse(FIXTURE_1_20_4).expect("parse");
        let natives_lib = &v.libraries[1];
        let rules = natives_lib.rules.as_ref().expect("rules");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].action, RuleAction::Allow);
        assert_eq!(rules[0].os.as_ref().unwrap().name.as_deref(), Some("windows"));
    }
}
