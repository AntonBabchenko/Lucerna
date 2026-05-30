//! End-to-end smoke test for the modern-era Forge install pipeline.
//!
//! Drives the full pipeline — maven-tree extraction, library downloads,
//! arg substitution, and all 5 client-side processor runs — against the
//! real 1.20.4-49.0.49 installer JAR. Uses a tempdir for the data root
//! so no real AppHandle is required (matching the established pattern
//! in `forge_transitional_era_e2e.rs`).
//!
//! # Prerequisites
//!
//! - The fixture JAR at
//!     `src-tauri/tests/fixtures/forge/installers/forge-1.20.4-49.0.49-installer.jar`
//!     (obtain via `src-tauri/tests/fixtures/forge/fetch.ps1`).
//! - A `java` (or `javaw`) executable on PATH (FART + binarypatcher
//!     processors invoke Java; `modern::install` uses `ensure_jre`, but
//!     in this manual harness we fall back to system Java).
//!
//! # Running
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test forge_modern_era_e2e \
//!     -- --ignored --nocapture
//! ```
//!
//! # Why #[ignore]
//!
//! Downloads ~50MB of JARs + Mojang mappings and runs 5 processors
//! (including 2 Java subprocesses). Far too slow for default `cargo test`
//! sweep; invaluable as an on-demand regression check.

const FIXTURE_PATH: &str = "tests/fixtures/forge/installers/forge-1.20.4-49.0.49-installer.jar";

const MC: &str = "1.20.4";
const FV: &str = "49.0.49";

fn load_fixture_or_skip() -> Option<(Vec<u8>, serde_json::Value)> {
    let bytes = match std::fs::read(FIXTURE_PATH) {
        Ok(b) => b,
        Err(_) => {
            eprintln!(
                "SKIP: fixture absent at {FIXTURE_PATH}. \
                 Run src-tauri/tests/fixtures/forge/fetch.ps1 to download it."
            );
            return None;
        }
    };
    let cursor = std::io::Cursor::new(bytes.clone());
    let mut archive = zip::ZipArchive::new(cursor).expect("zip open");
    let mut entry = archive
        .by_name("install_profile.json")
        .expect("install_profile.json entry");
    use std::io::Read;
    let mut buf = String::new();
    entry
        .read_to_string(&mut buf)
        .expect("read install_profile.json");
    let profile: serde_json::Value =
        serde_json::from_str(&buf).expect("parse install_profile.json");
    Some((bytes, profile))
}

async fn extract_maven_tree(installer_bytes: &[u8], libs_root: &std::path::Path) {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(installer_bytes))
        .expect("zip open for maven-tree");
    let mut to_write: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("zip index");
        let name = entry.name().to_string();
        if !name.starts_with("maven/") || name.ends_with('/') {
            continue;
        }
        let rel = name.strip_prefix("maven/").unwrap();
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).expect("read maven entry");
        to_write.push((libs_root.join(rel), buf));
    }
    for (dest, bytes) in to_write {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .expect("create maven dir");
        }
        tokio::fs::write(&dest, &bytes)
            .await
            .expect("write maven entry");
    }
}

async fn download_lib(url: &str, dest: &std::path::Path, sha1: &str) {
    if sha1.is_empty() && tokio::fs::metadata(dest).await.is_ok() {
        return; // TOFU
    } else if !sha1.is_empty() {
        if let Ok(bytes) = tokio::fs::read(dest).await {
            use sha1::{Digest, Sha1};
            let got = hex::encode(Sha1::digest(&bytes));
            if got == sha1 {
                eprintln!("  [cache] {}", dest.display());
                return;
            }
        }
    }
    eprintln!("  [download] {url}");
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .expect("create lib dir");
    }
    lucerna_lib::network::download::download_no_emit(url, dest, sha1, "forge-modern-e2e")
        .await
        .unwrap_or_else(|e| panic!("failed to download {url}: {e:?}"));
}

#[tokio::test]
#[ignore]
async fn install_forge_1_20_4_modern_era_e2e() {
    let Some((installer_bytes, install_profile_value)) = load_fixture_or_skip() else {
        return;
    };

    eprintln!("Loaded installer: {} bytes", installer_bytes.len());

    // 1. Parse install_profile.
    let raw_text =
        serde_json::to_string(&install_profile_value).expect("re-serialise install_profile");
    let profile = lucerna_lib::forge::installer::transitional::parse_install_profile(&raw_text)
        .expect("parse install_profile spec=1");
    eprintln!("profile.minecraft = {}", profile.minecraft);
    eprintln!("profile.spec      = {}", profile.spec);
    eprintln!("processors count  = {}", profile.processors.len());
    assert_eq!(profile.spec, 1, "modern profile must report spec=1");
    assert_eq!(profile.processors.len(), 9);

    // 2. Extract embedded version.json.
    let version_details = {
        use std::io::Read;
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(&installer_bytes)).expect("zip open");
        let mut entry = archive.by_name("version.json").expect("version.json entry");
        let mut buf = String::new();
        entry.read_to_string(&mut buf).expect("read version.json");
        lucerna_lib::versions::version_json::parse(&buf).expect("parse version.json")
    };
    eprintln!("version.json id        = {}", version_details.id);
    eprintln!("version.json mainClass = {}", version_details.main_class);
    assert_eq!(
        version_details.main_class,
        "net.minecraftforge.bootstrap.ForgeBootstrap"
    );

    // 3. Tempdir-based fake app root.
    let app_data_root = tempfile::tempdir().expect("tempdir");
    let libs_root = app_data_root.path().join("libraries");
    let cache_dir = app_data_root
        .path()
        .join("forge")
        .join("cache")
        .join(format!("{MC}-{FV}"));
    let installer_cache_path = app_data_root
        .path()
        .join("forge")
        .join("installers")
        .join(format!("{MC}-{FV}.jar"));
    let minecraft_jar = app_data_root
        .path()
        .join("versions")
        .join(MC)
        .join(format!("{MC}.jar"))
        .display()
        .to_string();
    tokio::fs::create_dir_all(&cache_dir).await.unwrap();
    tokio::fs::create_dir_all(&libs_root).await.unwrap();

    let cache_dir_str = cache_dir.display().to_string();
    let installer_path_str = installer_cache_path.display().to_string();
    eprintln!("libs_root = {}", libs_root.display());
    eprintln!("cache_dir = {cache_dir_str}");

    // 4. Extract maven/ subtree (just the shim jar for modern).
    eprintln!("--- step 4: extract maven/ subtree ---");
    extract_maven_tree(&installer_bytes, &libs_root).await;

    // 5. Download install_profile.libraries (filter URL-less).
    eprintln!("--- step 5: download install_profile.libraries ---");
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86"
    };
    eprintln!("platform = {os}/{arch}");

    let downloadable: Vec<lucerna_lib::versions::version_json::Library> = profile
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
    eprintln!("  libs to download: {}", downloadable.len());
    for lib in &downloadable {
        for (rel_path, url, sha1, _size) in
            lucerna_lib::versions::libraries::artifacts_to_install(lib, os, arch)
        {
            download_lib(&url, &libs_root.join(&rel_path), &sha1).await;
        }
    }

    // 5b. Download the vanilla MC client.jar (FART input).
    eprintln!("--- step 5b: download vanilla MC client.jar ---");
    {
        let mc_jar_path = app_data_root
            .path()
            .join("versions")
            .join(MC)
            .join(format!("{MC}.jar"));
        if tokio::fs::metadata(&mc_jar_path).await.is_ok() {
            eprintln!("  [cache] {}", mc_jar_path.display());
        } else {
            let entries = lucerna_lib::versions::manifest::list_manifest()
                .await
                .expect("fetch version manifest");
            let entry = entries
                .iter()
                .find(|e| e.id == MC)
                .expect("1.20.4 not found in manifest");
            let version_json: serde_json::Value =
                lucerna_lib::network::get_json(&entry.url, "forge-modern-e2e-client-jar")
                    .await
                    .expect("fetch 1.20.4 version JSON");
            let client_url = version_json["downloads"]["client"]["url"]
                .as_str()
                .expect("downloads.client.url");
            let client_sha1 = version_json["downloads"]["client"]["sha1"]
                .as_str()
                .expect("downloads.client.sha1");
            eprintln!("  [download] {client_url}");
            lucerna_lib::network::download::download_no_emit(
                client_url,
                &mc_jar_path,
                client_sha1,
                "forge-modern-e2e-client-jar",
            )
            .await
            .expect("download 1.20.4 client.jar");
        }
    }

    // 6. Run processors.
    eprintln!("--- step 6: processors ---");
    use lucerna_lib::forge::installer::transitional::{
        classpath_coords_to_libraries, substitute_args,
    };
    use lucerna_lib::forge::patcher::{run_processor, ProcessorContext};

    for (i, p) in profile.processors.iter().enumerate() {
        if let Some(sides) = &p.sides {
            if !sides.iter().any(|s| s == "client") {
                eprintln!("  processor {i} ({}) — skipped (server-only)", p.jar);
                continue;
            }
        }
        eprintln!("  processor {i}: {}", p.jar);

        let cp_libs = classpath_coords_to_libraries(&p.classpath);
        for lib in &cp_libs {
            for (rel_path, url, sha1, _size) in
                lucerna_lib::versions::libraries::artifacts_to_install(lib, os, arch)
            {
                download_lib(&url, &libs_root.join(&rel_path), &sha1).await;
            }
        }

        let resolved = substitute_args(
            &p.args,
            &profile.data,
            "client",
            &libs_root,
            &installer_bytes,
            &installer_path_str,
            &cache_dir_str,
            &minecraft_jar,
        )
        .await
        .unwrap_or_else(|e| panic!("substitute_args for processor {i} failed: {e:?}"));
        eprintln!("    resolved args: {resolved:?}");

        let mut cp_paths: Vec<std::path::PathBuf> = p
            .classpath
            .iter()
            .filter_map(|c| lucerna_lib::forge::patcher::maven_coord_to_relative_path(c))
            .map(|rel| libs_root.join(rel))
            .collect();
        if let Some(rel) = lucerna_lib::forge::patcher::maven_coord_to_relative_path(&p.jar) {
            cp_paths.push(libs_root.join(rel));
        }
        let ctx = ProcessorContext {
            classpath: cp_paths,
            cache_dir: cache_dir.clone(),
            // Falls back to "java" on PATH — e2e harness expects system JRE.
            java_bin: None,
        };
        run_processor(&p.jar, resolved, &ctx)
            .await
            .unwrap_or_else(|e| panic!("processor {i} ({}) failed: {e:?}", p.jar));
        eprintln!("    processor {i} OK");
    }

    // 7. Assemble.
    eprintln!("--- step 7: assemble VersionDetails ---");
    let final_details = lucerna_lib::forge::profile::assemble_from_modern(version_details);

    eprintln!("--- INSTALL SUCCEEDED ---");
    eprintln!("  id        = {}", final_details.id);
    eprintln!("  mainClass = {}", final_details.main_class);
    eprintln!("  libraries = {} entries", final_details.libraries.len());
    if let Some(args) = final_details.arguments.as_ref() {
        eprintln!("  arguments.game count = {}", args.game.len());
        eprintln!("  arguments.jvm count  = {}", args.jvm.len());
    }

    assert!(!final_details.id.is_empty());
    assert_eq!(
        final_details.main_class,
        "net.minecraftforge.bootstrap.ForgeBootstrap"
    );
}
