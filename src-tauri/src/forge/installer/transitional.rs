//! MC 1.13.x — 1.16.x installer era. install_profile v2 + embedded
//! /version.json + 4 processors (installertools, jarsplitter,
//! SpecialSource, binarypatcher).

use crate::error::{Error, Result};
use crate::versions::version_json::VersionDetails;
use serde::Deserialize;

/// Extract every entry under `maven/` from the installer JAR into
/// the libraries directory, preserving subpaths. Forge installers
/// bundle the universal jar (and sometimes the main forge jar) inside
/// themselves under `maven/`; those entries appear in
/// `install_profile.libraries[]` WITHOUT a download URL because they
/// aren't on any maven server — they're shipped in the installer.
/// Mirrors Phase 1's `legacy::extract_universal_jar_bytes` behavior
/// generalised to the whole maven subtree.
async fn extract_installer_maven_tree(
    installer_bytes: &[u8],
    libs_root: &std::path::Path,
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(installer_bytes))
        .map_err(|e| Error::ForgeInstallerCorrupted {
            mc: "<unknown>".into(),
            fv: "<unknown>".into(),
            details: format!("maven-tree zip open: {e}"),
        })?;
    let mut to_write: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        use std::io::Read;
        let mut entry = archive.by_index(i).map_err(|e| {
            Error::ForgeInstallerCorrupted {
                mc: "<unknown>".into(),
                fv: "<unknown>".into(),
                details: format!("maven-tree zip index {i}: {e}"),
            }
        })?;
        let name = entry.name().to_string();
        if !name.starts_with("maven/") || name.ends_with('/') {
            continue;
        }
        let Some(rel) = name.strip_prefix("maven/") else { continue };
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| {
            Error::ForgeInstallerCorrupted {
                mc: "<unknown>".into(),
                fv: "<unknown>".into(),
                details: format!("read maven entry {name}: {e}"),
            }
        })?;
        to_write.push((libs_root.join(rel), buf));
        // entry (ZipFile, not Send) drops at end of this iteration
        // — before any .await below.
    }
    for (dest, bytes) in to_write {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                Error::io(parent.display().to_string(), e)
            })?;
        }
        tokio::fs::write(&dest, &bytes).await.map_err(|e| {
            Error::io(dest.display().to_string(), e)
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProfile {
    #[serde(default)]
    pub spec: i64,
    pub profile: String,
    pub version: String,
    pub minecraft: String,
    pub path: Option<String>,
    #[serde(default)]
    pub data: std::collections::HashMap<String, DataEntry>,
    #[serde(default)]
    pub libraries: Vec<crate::versions::version_json::Library>,
    #[serde(default)]
    pub processors: Vec<ProcessorEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataEntry {
    pub client: String,
    pub server: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessorEntry {
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub outputs: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub sides: Option<Vec<String>>,
}

pub fn parse_install_profile(text: &str) -> Result<InstallProfile> {
    serde_json::from_str(text).map_err(|e| Error::ForgeInstallerCorrupted {
        mc: "<unknown>".into(),
        fv: "<unknown>".into(),
        details: format!("install_profile v2 parse: {e}"),
    })
}

pub async fn install(
    install_profile_value: &serde_json::Value,
    installer_bytes: &[u8],
    mc: &str,
    fv: &str,
    app: &tauri::AppHandle,
) -> Result<VersionDetails> {
    let raw_text = serde_json::to_string(install_profile_value).map_err(|e| Error::ForgeInstallerCorrupted {
        mc: mc.into(), fv: fv.into(),
        details: format!("re-serialise install_profile: {e}"),
    })?;
    let profile = parse_install_profile(&raw_text).map_err(|e| match e {
        Error::ForgeInstallerCorrupted { details, .. } => Error::ForgeInstallerCorrupted {
            mc: mc.into(), fv: fv.into(), details,
        },
        other => other,
    })?;

    // 2. Extract /version.json from installer.
    let version_details = {
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(installer_bytes)).map_err(|e| {
            Error::ForgeInstallerCorrupted { mc: mc.into(), fv: fv.into(), details: format!("zip: {e}") }
        })?;
        let mut entry = archive.by_name("version.json").map_err(|_| Error::ForgeInstallerCorrupted {
            mc: mc.into(), fv: fv.into(), details: "missing /version.json".into(),
        })?;
        let mut buf = String::new();
        entry.read_to_string(&mut buf).map_err(|e| Error::ForgeInstallerCorrupted {
            mc: mc.into(), fv: fv.into(), details: format!("read version.json: {e}"),
        })?;
        crate::versions::version_json::parse(&buf).map_err(|e| Error::ForgeInstallerCorrupted {
            mc: mc.into(), fv: fv.into(), details: format!("parse version.json: {e}"),
        })?
    };

    // 3-5. Compute paths.
    let app_data = crate::paths::app_dir(app).map_err(|e| Error::io("<app_data>", e))?;
    let cache_dir = app_data.join("forge").join("cache").join(format!("{mc}-{fv}"));
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| Error::io(cache_dir.display().to_string(), e))?;
    let cache_dir_str = cache_dir.display().to_string();
    let libs_root = crate::paths::libraries_dir(app).map_err(|e| Error::io("<libraries>", e))?;
    let installer_cache_path = app_data.join("forge").join("installers").join(format!("{mc}-{fv}.jar"));
    let installer_path_str = installer_cache_path.display().to_string();
    let minecraft_jar = app_data.join("versions").join(mc).join(format!("{mc}.jar"))
        .display().to_string();

    // 6. Sequence processors.
    use crate::forge::patcher::{run_processor, ProcessorContext};
    use crate::versions::libraries::ensure_libraries;
    let os = crate::versions::install::current_os();
    let arch = crate::versions::install::current_arch();

    // Ensure the vanilla MC client.jar is present BEFORE processors run.
    // Processors 1-3 (jarsplitter, SpecialSource, binarypatcher) consume it
    // via the {MINECRAFT_JAR} placeholder. In the normal UI flow the caller
    // (install_version) already downloads vanilla before entering the Forge
    // pipeline, but this function can be called in isolation (e.g. from e2e
    // tests) and should be self-sufficient — it's idempotent when cached.
    let vanilla_jre_component;
    {
        let vanilla_details = crate::versions::install::ensure_version_json(mc, app).await?;
        vanilla_jre_component = vanilla_details
            .java_version
            .as_ref()
            .map(|jv| jv.component.clone())
            .unwrap_or_else(|| crate::jre::DEFAULT_LEGACY_COMPONENT.to_string());
        let client_download = vanilla_details
            .downloads
            .as_ref()
            .ok_or_else(|| crate::error::Error::ForgeInstallerCorrupted {
                mc: mc.into(),
                fv: fv.into(),
                details: "vanilla version JSON has no downloads field".into(),
            })?;
        crate::versions::client::ensure_client(mc, &client_download.client, app).await?;
    }

    // Ensure JRE is installed BEFORE running processors. The SpecialSource
    // processor (Java subprocess) requires it. Installing upfront is idempotent
    // and cheap when already cached. Use a no-op progress callback — this
    // function doesn't have the event-emission infrastructure that
    // install_version has; the JRE is typically already installed by the time
    // the Forge install step runs.
    crate::jre::ensure_jre(&vanilla_jre_component, app, |_, _, _| {}).await?;
    let java_bin = crate::jre::java_executable_path(&vanilla_jre_component, app)?;

    // Extract maven/ subtree from installer (forge universal jar etc.) into
    // libs_root before fetching networked libraries. These entries appear in
    // libraries[] without a URL because they ship inside the installer JAR.
    extract_installer_maven_tree(installer_bytes, &libs_root).await?;

    // Filter to only entries that have a download URL — the URL-less ones
    // (forge universal etc.) are already on disk from extract_installer_maven_tree.
    let downloadable: Vec<crate::versions::version_json::Library> = profile
        .libraries
        .iter()
        .filter(|l| {
            let from_top = l.url.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
            let from_artifact = l
                .downloads
                .as_ref()
                .and_then(|d| d.artifact.as_ref())
                .map(|a| !a.url.is_empty())
                .unwrap_or(false);
            from_top || from_artifact
        })
        .cloned()
        .collect();
    ensure_libraries(&downloadable, os, arch, app).await?;

    for p in &profile.processors {
        // Skip if `sides` is specified and doesn't include "client".
        if let Some(sides) = &p.sides {
            if !sides.iter().any(|s| s == "client") { continue; }
        }
        let cp_libs = classpath_coords_to_libraries(&p.classpath);
        ensure_libraries(&cp_libs, os, arch, app).await?;
        let resolved = substitute_args(
            &p.args, &profile.data, "client",
            &libs_root, installer_bytes, &installer_path_str, &cache_dir_str, &minecraft_jar,
        ).await?;
        let mut cp_paths: Vec<std::path::PathBuf> = p.classpath.iter()
            .filter_map(|c| crate::forge::patcher::maven_coord_to_relative_path(c))
            .map(|rel| libs_root.join(rel))
            .collect();
        // Include processor's OWN jar so Java can find its main class.
        // The classpath declared in install_profile lists dependencies only;
        // the processor jar itself is separate (it IS the tool, not a dep).
        if let Some(rel) = crate::forge::patcher::maven_coord_to_relative_path(&p.jar) {
            cp_paths.push(libs_root.join(rel));
        }
        let ctx = ProcessorContext {
            classpath: cp_paths,
            cache_dir: cache_dir.clone(),
            java_bin: Some(java_bin.clone()),
        };
        run_processor(&p.jar, resolved, &ctx).await?;
    }

    Ok(crate::forge::profile::assemble_from_transitional(version_details))
}

use std::path::Path;

/// Resolve a maven-coord-shaped string `[group:artifact:version[:classifier][@ext]]`
/// to an absolute libraries-root path. Returns the resolved path as a String, or
/// `Error::ForgeInstallerCorrupted` if the coord is malformed.
///
/// Shape: `[g:a:v]`, `[g:a:v:classifier]`, `[g:a:v@ext]`, `[g:a:v:classifier@ext]`.
/// Default ext when omitted: `jar`.
fn resolve_bracket_coord(raw: &str, libs_root: &Path) -> Result<String> {
    let Some(coord) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return Err(Error::ForgeInstallerCorrupted {
            mc: "<unknown>".into(),
            fv: "<unknown>".into(),
            details: format!("not a bracket coord: {raw}"),
        });
    };
    let (coord_no_ext, ext) = match coord.rsplit_once('@') {
        Some((c, e)) => (c, e),
        None => (coord, "jar"),
    };
    let parts: Vec<&str> = coord_no_ext.split(':').collect();
    if parts.len() < 3 {
        return Err(Error::ForgeInstallerCorrupted {
            mc: "<unknown>".into(),
            fv: "<unknown>".into(),
            details: format!("malformed coord: {raw}"),
        });
    }
    let group_path = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Ok(libs_root
        .join(format!(
            "{group_path}/{artifact}/{version}/{artifact}-{version}{classifier}.{ext}"
        ))
        .display()
        .to_string())
}

pub fn resolve_reserved(
    name: &str,
    _libs_root: &Path,
    installer_path: &str,
    cache_dir: &str,
    minecraft_jar: &str,
) -> Option<String> {
    match name {
        "SIDE" => Some("client".into()),
        "ROOT" => Some(cache_dir.into()),
        "INSTALLER" => Some(installer_path.into()),
        "MINECRAFT_JAR" => Some(minecraft_jar.into()),
        _ => None,
    }
}

pub async fn resolve_data(
    placeholder: &str,
    data: &std::collections::HashMap<String, DataEntry>,
    side: &str,
    libs_root: &Path,
    installer_bytes: &[u8],
    cache_dir: &str,
) -> Result<String> {
    let entry = data.get(placeholder).ok_or_else(|| Error::ForgeInstallerCorrupted {
        mc: "<unknown>".into(), fv: "<unknown>".into(),
        details: format!("data missing placeholder `{placeholder}`"),
    })?;
    let raw = match side {
        "client" => entry.client.as_str(),
        "server" => entry.server.as_str(),
        other => return Err(Error::ForgeInstallerCorrupted {
            mc: "<unknown>".into(), fv: "<unknown>".into(),
            details: format!("unknown side `{other}`"),
        }),
    };

    if raw.starts_with('[') && raw.ends_with(']') {
        return resolve_bracket_coord(raw, libs_root);
    }

    if let Some(hex) = raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        // Quoted hex literal — used by `outputs` blocks as expected sha1.
        // Substitution returns the literal hex bytes.
        return Ok(hex.to_string());
    }

    if let Some(rel) = raw.strip_prefix('/') {
        // In-jar path. Extract from installer into cache_dir/<basename>.
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(installer_bytes)).map_err(|e| {
            Error::ForgeInstallerCorrupted {
                mc: "<unknown>".into(), fv: "<unknown>".into(),
                details: format!("zip open: {e}"),
            }
        })?;
        let mut entry = archive.by_name(rel).map_err(|_| Error::ForgeInstallerCorrupted {
            mc: "<unknown>".into(), fv: "<unknown>".into(),
            details: format!("installer missing entry: {rel}"),
        })?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| Error::ForgeInstallerCorrupted {
            mc: "<unknown>".into(), fv: "<unknown>".into(),
            details: format!("read {rel}: {e}"),
        })?;
        let basename = std::path::Path::new(rel).file_name().unwrap().to_string_lossy().to_string();
        let dest = std::path::PathBuf::from(cache_dir).join(&basename);
        if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent).ok(); }
        std::fs::write(&dest, &buf).map_err(|e| Error::io(dest.display().to_string(), e))?;
        return Ok(dest.display().to_string());
    }

    // Literal string fallback.
    Ok(raw.to_string())
}

pub async fn substitute_args(
    args: &[String],
    data: &std::collections::HashMap<String, DataEntry>,
    side: &str,
    libs_root: &Path,
    installer_bytes: &[u8],
    installer_path: &str,
    cache_dir: &str,
    minecraft_jar: &str,
) -> Result<Vec<String>> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        let mut s = a.clone();
        while let Some(open) = s.find('{') {
            let close = match s[open..].find('}') {
                Some(c) => open + c,
                None => break,
            };
            let name = s[open + 1..close].to_string();
            let value = if let Some(v) = resolve_reserved(&name, libs_root, installer_path, cache_dir, minecraft_jar) {
                v
            } else {
                resolve_data(&name, data, side, libs_root, installer_bytes, cache_dir).await?
            };
            s = format!("{}{}{}", &s[..open], value, &s[close + 1..]);
        }
        // After {PLACEHOLDER} substitution, if the arg is shaped as a bracketed
        // maven coord (e.g. MCP_DATA's --input), resolve it to a libraries path.
        // This mirrors resolve_data's bracket branch but applies to top-level args
        // that aren't behind a {PLACEHOLDER} indirection.
        if s.starts_with('[') && s.ends_with(']') {
            s = resolve_bracket_coord(&s, libs_root)?;
        }
        out.push(s);
    }
    Ok(out)
}

pub fn classpath_coords_to_libraries(coords: &[String]) -> Vec<crate::versions::version_json::Library> {
    coords.iter().map(|c| crate::versions::version_json::Library {
        name: c.clone(),
        // For Forge-specific processors (jar coords like net.minecraftforge:installertools)
        // hint the Forge maven. For NeoForge processors (net.neoforged.* group),
        // hint maven.neoforged.net. The libraries.minecraft.net fallback in
        // ensure_libraries is used for everything else (e.g. org.ow2.asm:asm).
        url: if c.starts_with("net.minecraftforge:") || c.starts_with("de.oceanlabs.mcp:") {
            Some("https://maven.minecraftforge.net/".into())
        } else if c.starts_with("net.neoforged.installertools:") || c.starts_with("net.neoforged:") {
            Some("https://maven.neoforged.net/releases/".into())
        } else {
            None
        },
        downloads: None,
        rules: None,
        natives: None,
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "spec": 0,
      "profile": "1.16.5-forge-36.2.42",
      "version": "1.16.5-forge-36.2.42",
      "minecraft": "1.16.5",
      "path": "net.minecraftforge:forge:1.16.5-36.2.42",
      "data": {
        "MAPPINGS": { "client": "[de.oceanlabs.mcp:mcp_config:1.16.5-20210115.111550@zip]", "server": "[de.oceanlabs.mcp:mcp_config:1.16.5-20210115.111550@zip]" },
        "MC_SLIM_SHA": { "client": "'abc123'", "server": "'def456'" },
        "BINPATCH": { "client": "/data/client.lzma", "server": "/data/server.lzma" }
      },
      "processors": [
        { "jar": "net.minecraftforge:installertools:1.3.0", "args": ["--task", "MCP_DATA"] }
      ]
    }"#;

    #[test]
    fn parses_v2_with_three_value_shapes() {
        let p = parse_install_profile(FIXTURE).expect("parse");
        assert_eq!(p.spec, 0);
        assert_eq!(p.minecraft, "1.16.5");
        assert_eq!(p.data.len(), 3);
        assert!(p.data["MAPPINGS"].client.starts_with('['));
        assert!(p.data["MC_SLIM_SHA"].client.starts_with('\''));
        assert!(p.data["BINPATCH"].client.starts_with('/'));
        assert_eq!(p.processors.len(), 1);
    }

    #[test]
    fn missing_required_top_level_errors() {
        let bad = r#"{ "spec": 0 }"#;
        assert!(parse_install_profile(bad).is_err());
    }

    #[tokio::test]
    async fn resolve_data_maven_coord_with_at_ext() {
        let mut data = std::collections::HashMap::new();
        data.insert("MAPPINGS".into(), DataEntry {
            client: "[de.oceanlabs.mcp:mcp_config:1.16.5-20210115@zip]".into(),
            server: "[de.oceanlabs.mcp:mcp_config:1.16.5-20210115@zip]".into(),
        });
        let libs = std::path::PathBuf::from("/x/libraries");
        let result = resolve_data("MAPPINGS", &data, "client", &libs, b"", "/cache").await.unwrap();
        // Result should be `/x/libraries/de/oceanlabs/mcp/mcp_config/1.16.5-20210115/mcp_config-1.16.5-20210115.zip`
        assert!(result.contains("mcp_config-1.16.5-20210115.zip"), "got: {result}");
        assert!(result.ends_with(".zip"));
    }

    #[tokio::test]
    async fn resolve_data_quoted_hex_literal() {
        let mut data = std::collections::HashMap::new();
        data.insert("MC_SLIM_SHA".into(), DataEntry {
            client: "'fccafccf8ad4ce6d9f008e786b48ff53172bf9de'".into(),
            server: "'sha2'".into(),
        });
        let libs = std::path::PathBuf::from("/x/libraries");
        let result = resolve_data("MC_SLIM_SHA", &data, "client", &libs, b"", "/cache").await.unwrap();
        assert_eq!(result, "fccafccf8ad4ce6d9f008e786b48ff53172bf9de");
    }

    #[tokio::test]
    async fn resolve_reserved_root_and_installer_and_minecraft_jar() {
        let libs = std::path::PathBuf::from("/x/libraries");
        assert_eq!(resolve_reserved("ROOT", &libs, "/inst.jar", "/cache", "/mc.jar"), Some("/cache".into()));
        assert_eq!(resolve_reserved("INSTALLER", &libs, "/inst.jar", "/cache", "/mc.jar"), Some("/inst.jar".into()));
        assert_eq!(resolve_reserved("MINECRAFT_JAR", &libs, "/inst.jar", "/cache", "/mc.jar"), Some("/mc.jar".into()));
        assert_eq!(resolve_reserved("UNKNOWN", &libs, "/i", "/c", "/m"), None);
    }

    #[tokio::test]
    async fn substitute_args_resolves_top_level_bracket_coord() {
        use std::collections::HashMap;
        let data: HashMap<String, DataEntry> = HashMap::new();
        let libs = std::path::PathBuf::from("/x/libraries");
        let raw = vec!["[de.oceanlabs.mcp:mcp_config:1.16.5-20210115.111550@zip]".into()];
        let out = substitute_args(&raw, &data, "client", &libs, b"", "/inst.jar", "/cache", "/mc.jar").await.unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].ends_with("mcp_config-1.16.5-20210115.111550.zip"), "got: {}", out[0]);
        assert!(!out[0].contains('['), "bracket should be resolved away: {}", out[0]);
    }

    #[test]
    fn classpath_forge_coord_gets_forge_maven_url() {
        let coords = vec![
            "net.minecraftforge:installertools:1.3.0".into(),
            "com.google.guava:guava:21.0".into(),
        ];
        let libs = classpath_coords_to_libraries(&coords);
        assert_eq!(libs[0].url.as_deref(), Some("https://maven.minecraftforge.net/"));
        assert!(libs[1].url.is_none()); // falls back to libraries.minecraft.net
    }
}

/// Public re-export so `installer::modern` can call the same helper.
/// Phase 2 kept this private as an internal detail of `install`; modern
/// era shares the exact same maven-tree extraction logic.
pub async fn extract_installer_maven_tree_pub(
    installer_bytes: &[u8],
    libs_root: &std::path::Path,
) -> Result<()> {
    extract_installer_maven_tree(installer_bytes, libs_root).await
}
