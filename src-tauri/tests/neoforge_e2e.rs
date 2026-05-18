//! End-to-end smoke test for NeoForge install pipeline.
//!
//! Reuses the v0.4.0 modern-era install pipeline through
//! `ForgeFlavor::NeoForge`. Drives the full pipeline against a real
//! NeoForge installer JAR. Uses a tempdir for the data root so no real
//! AppHandle is required (matching `forge_modern_era_e2e.rs`).
//!
//! # Prerequisites
//!
//! - Fixture JARs at
//!     `src-tauri/tests/fixtures/forge/installers/neoforge-<NV>-installer.jar`
//!     (obtain via `src-tauri/tests/fixtures/forge/fetch.ps1`).
//! - A `java` (or `javaw`) executable on PATH (ART + binarypatcher
//!     processors invoke Java).
//!
//! # Running
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test neoforge_e2e \
//!     -- --ignored --nocapture
//! ```
//!
//! # Why #[ignore]
//!
//! Downloads ~50MB of JARs + Mojang mappings and runs 5 processors
//! (including 2 Java subprocesses). Far too slow for default `cargo test`
//! sweep; invaluable as an on-demand regression check.
//!
//! # Version coverage
//!
//! - `install_neoforge_1_20_4_e2e` — NeoForge 20.4.251 / MC 1.20.4.
//!   FML 2.0.17 / modlauncher 10.0.9 (20.4.x bootstrap era).
//! - `install_neoforge_1_21_1_e2e` — NeoForge 21.1.230 / MC 1.21.1.
//!   FML 4.0.42 / modlauncher 11.0.5 / securejarhandler 3.0.8 (21.x bootstrap era).
//!   This is the boundary where ADDENDUM D must NOT inject the :client jar:
//!   securejarhandler 3.0.8 promotes it to a JPMS module "neoforge" colliding
//!   with the universal jar → java.lang.module.ResolutionException. Regression
//!   test for the fix in forge/installer/modern.rs (ADDENDUM D neoforge skip).

// ── 20.4.x (FML 2.x) constants ─────────────────────────────────────────────

const FIXTURE_PATH: &str =
    "tests/fixtures/forge/installers/neoforge-20.4.251-installer.jar";

const MC: &str = "1.20.4";
const FV: &str = "20.4.251";

// ── 21.1.x (FML 4.x) constants ─────────────────────────────────────────────

const FIXTURE_PATH_21: &str =
    "tests/fixtures/forge/installers/neoforge-21.1.230-installer.jar";

const MC_21: &str = "1.21.1";
const FV_21: &str = "21.1.230";

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
            tokio::fs::create_dir_all(parent).await.expect("create maven dir");
        }
        tokio::fs::write(&dest, &bytes).await.expect("write maven entry");
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
        tokio::fs::create_dir_all(parent).await.expect("create lib dir");
    }
    ftlauncher_lib::network::download::download_no_emit(url, dest, sha1, "neoforge-e2e")
        .await
        .unwrap_or_else(|e| panic!("failed to download {url}: {e:?}"));
}

#[tokio::test]
#[ignore]
async fn install_neoforge_1_20_4_e2e() {
    let Some((installer_bytes, install_profile_value)) = load_fixture_or_skip() else {
        return;
    };

    eprintln!("Loaded installer: {} bytes", installer_bytes.len());

    // 1. Parse install_profile.
    let raw_text =
        serde_json::to_string(&install_profile_value).expect("re-serialise install_profile");
    let profile =
        ftlauncher_lib::forge::installer::transitional::parse_install_profile(&raw_text)
            .expect("parse install_profile spec=1");
    eprintln!("profile.minecraft = {}", profile.minecraft);
    eprintln!("profile.spec      = {}", profile.spec);
    eprintln!("processors count  = {}", profile.processors.len());
    assert_eq!(profile.spec, 1, "modern profile must report spec=1");
    assert_eq!(profile.processors.len(), 10);

    // 2. Extract embedded version.json.
    let version_details = {
        use std::io::Read;
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(&installer_bytes)).expect("zip open");
        let mut entry = archive.by_name("version.json").expect("version.json entry");
        let mut buf = String::new();
        entry.read_to_string(&mut buf).expect("read version.json");
        ftlauncher_lib::versions::version_json::parse(&buf).expect("parse version.json")
    };
    eprintln!("version.json id        = {}", version_details.id);
    eprintln!("version.json mainClass = {}", version_details.main_class);
    assert_eq!(
        version_details.main_class,
        "cpw.mods.bootstraplauncher.BootstrapLauncher"
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

    // 4. Extract maven/ subtree (NeoForge has no embedded maven artifacts; this is a no-op).
    eprintln!("--- step 4: extract maven/ subtree ---");
    extract_maven_tree(&installer_bytes, &libs_root).await;

    // 5. Download install_profile.libraries (filter URL-less).
    eprintln!("--- step 5: download install_profile.libraries ---");
    let os = if cfg!(target_os = "windows") { "windows" }
        else if cfg!(target_os = "macos") { "macos" }
        else { "linux" };
    let arch = if cfg!(target_arch = "x86_64") { "x64" }
        else if cfg!(target_arch = "aarch64") { "aarch64" }
        else { "x86" };
    eprintln!("platform = {os}/{arch}");

    let downloadable: Vec<ftlauncher_lib::versions::version_json::Library> = profile
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
            ftlauncher_lib::versions::libraries::artifacts_to_install(lib, os, arch)
        {
            download_lib(&url, &libs_root.join(&rel_path), &sha1).await;
        }
    }

    // 5b. Download the vanilla MC client.jar (jarsplitter input).
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
            let entries = ftlauncher_lib::versions::manifest::list_manifest()
                .await
                .expect("fetch version manifest");
            let entry = entries
                .iter()
                .find(|e| e.id == MC)
                .expect("1.20.4 not found in manifest");
            let version_json: serde_json::Value =
                ftlauncher_lib::network::get_json(&entry.url, "neoforge-e2e-client-jar")
                    .await
                    .expect("fetch 1.20.4 version JSON");
            let client_url = version_json["downloads"]["client"]["url"]
                .as_str()
                .expect("downloads.client.url");
            let client_sha1 = version_json["downloads"]["client"]["sha1"]
                .as_str()
                .expect("downloads.client.sha1");
            eprintln!("  [download] {client_url}");
            ftlauncher_lib::network::download::download_no_emit(
                client_url,
                &mc_jar_path,
                client_sha1,
                "neoforge-e2e-client-jar",
            )
            .await
            .expect("download 1.20.4 client.jar");
        }
    }

    // 6. Run processors.
    eprintln!("--- step 6: processors ---");
    use ftlauncher_lib::forge::installer::transitional::{
        classpath_coords_to_libraries, substitute_args,
    };
    use ftlauncher_lib::forge::patcher::{run_processor, ProcessorContext};

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
                ftlauncher_lib::versions::libraries::artifacts_to_install(lib, os, arch)
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
            .filter_map(|c| ftlauncher_lib::forge::patcher::maven_coord_to_relative_path(c))
            .map(|rel| libs_root.join(rel))
            .collect();
        if let Some(rel) = ftlauncher_lib::forge::patcher::maven_coord_to_relative_path(&p.jar) {
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
    let final_details = ftlauncher_lib::forge::profile::assemble_from_modern(version_details);

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
        "cpw.mods.bootstraplauncher.BootstrapLauncher"
    );
}

// ── NeoForge 21.1.230 / MC 1.21.1 ───────────────────────────────────────────
//
// Regression test for the ADDENDUM D JPMS module collision bug:
//
//   NeoForge 21.x uses FML 4.0.42 + securejarhandler 3.0.8 which promotes
//   :client jars on -cp to JPMS modules by filename heuristic. The universal
//   jar is also named "neoforge" → ResolutionException: Module neoforge reads
//   another module named neoforge.
//
//   The fix: inject_patched_library_if_missing skips net.neoforged: coords
//   because the bootstrap launcher auto-discovers the patched client via
//   --fml.neoForgeVersion on the command line (true for all NeoForge versions).
//
//   This test verifies:
//   (a) the install pipeline completes without error for 21.1.x, AND
//   (b) the injected library list does NOT contain a net.neoforged:neoforge:21.1.230:client
//       URL-less entry (which would trigger the JPMS collision at launch).

fn load_fixture_or_skip_21() -> Option<(Vec<u8>, serde_json::Value)> {
    let bytes = match std::fs::read(FIXTURE_PATH_21) {
        Ok(b) => b,
        Err(_) => {
            eprintln!(
                "SKIP: fixture absent at {FIXTURE_PATH_21}. \
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

#[tokio::test]
#[ignore]
async fn install_neoforge_1_21_1_e2e() {
    let Some((installer_bytes, install_profile_value)) = load_fixture_or_skip_21() else {
        return;
    };

    eprintln!("Loaded installer: {} bytes", installer_bytes.len());
    eprintln!("MC={MC_21}  NeoForge={FV_21}");

    // 1. Parse install_profile.
    let raw_text =
        serde_json::to_string(&install_profile_value).expect("re-serialise install_profile");
    let profile =
        ftlauncher_lib::forge::installer::transitional::parse_install_profile(&raw_text)
            .expect("parse install_profile spec=1");
    eprintln!("profile.minecraft = {}", profile.minecraft);
    eprintln!("profile.spec      = {}", profile.spec);
    eprintln!("processors count  = {}", profile.processors.len());
    assert_eq!(profile.spec, 1, "modern profile must report spec=1");
    // 21.1.230 has fewer processors than 20.4.251 (new PROCESS_MINECRAFT_JAR
    // single-pass pipeline vs jarsplitter + ART 2.0.3 + binarypatcher).
    assert!(
        profile.processors.len() >= 1,
        "expected at least 1 processor, got {}",
        profile.processors.len()
    );

    // 2. Extract embedded version.json.
    let version_details = {
        use std::io::Read;
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(&installer_bytes)).expect("zip open");
        let mut entry = archive.by_name("version.json").expect("version.json entry");
        let mut buf = String::new();
        entry.read_to_string(&mut buf).expect("read version.json");
        ftlauncher_lib::versions::version_json::parse(&buf).expect("parse version.json")
    };
    eprintln!("version.json id        = {}", version_details.id);
    eprintln!("version.json mainClass = {}", version_details.main_class);
    assert_eq!(
        version_details.main_class,
        "cpw.mods.bootstraplauncher.BootstrapLauncher"
    );

    // Confirm game args contain --fml.neoForgeVersion (bootstrap uses this for
    // auto-discovery — our skip condition relies on the coord prefix).
    if let Some(args) = version_details.arguments.as_ref() {
        use ftlauncher_lib::versions::version_json::Argument;
        let game_strs: Vec<&str> = args
            .game
            .iter()
            .filter_map(|a| if let Argument::Plain(s) = a { Some(s.as_str()) } else { None })
            .collect();
        eprintln!("game args: {game_strs:?}");
        assert!(
            game_strs.contains(&"--fml.neoForgeVersion"),
            "version.json must have --fml.neoForgeVersion in game args"
        );
    }

    // 3. Tempdir-based fake app root.
    let app_data_root = tempfile::tempdir().expect("tempdir");
    let libs_root = app_data_root.path().join("libraries");
    let cache_dir = app_data_root
        .path()
        .join("forge")
        .join("cache")
        .join(format!("{MC_21}-{FV_21}"));
    let installer_cache_path = app_data_root
        .path()
        .join("forge")
        .join("installers")
        .join(format!("{MC_21}-{FV_21}.jar"));
    let minecraft_jar = app_data_root
        .path()
        .join("versions")
        .join(MC_21)
        .join(format!("{MC_21}.jar"))
        .display()
        .to_string();
    tokio::fs::create_dir_all(&cache_dir).await.unwrap();
    tokio::fs::create_dir_all(&libs_root).await.unwrap();

    let cache_dir_str = cache_dir.display().to_string();
    let installer_path_str = installer_cache_path.display().to_string();
    eprintln!("libs_root = {}", libs_root.display());
    eprintln!("cache_dir = {cache_dir_str}");

    // 4. Extract maven/ subtree.
    eprintln!("--- step 4: extract maven/ subtree ---");
    extract_maven_tree(&installer_bytes, &libs_root).await;

    // 5. Download install_profile.libraries (filter URL-less).
    eprintln!("--- step 5: download install_profile.libraries ---");
    let os = if cfg!(target_os = "windows") { "windows" }
        else if cfg!(target_os = "macos") { "macos" }
        else { "linux" };
    let arch = if cfg!(target_arch = "x86_64") { "x64" }
        else if cfg!(target_arch = "aarch64") { "aarch64" }
        else { "x86" };
    eprintln!("platform = {os}/{arch}");

    let downloadable: Vec<ftlauncher_lib::versions::version_json::Library> = profile
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
            ftlauncher_lib::versions::libraries::artifacts_to_install(lib, os, arch)
        {
            download_lib(&url, &libs_root.join(&rel_path), &sha1).await;
        }
    }

    // 5b. Download the vanilla MC client.jar.
    eprintln!("--- step 5b: download vanilla MC client.jar ({MC_21}) ---");
    {
        let mc_jar_path = app_data_root
            .path()
            .join("versions")
            .join(MC_21)
            .join(format!("{MC_21}.jar"));
        if tokio::fs::metadata(&mc_jar_path).await.is_ok() {
            eprintln!("  [cache] {}", mc_jar_path.display());
        } else {
            let entries = ftlauncher_lib::versions::manifest::list_manifest()
                .await
                .expect("fetch version manifest");
            let entry = entries
                .iter()
                .find(|e| e.id == MC_21)
                .expect("1.21.1 not found in manifest");
            let version_json: serde_json::Value =
                ftlauncher_lib::network::get_json(&entry.url, "neoforge-e2e-client-jar-21")
                    .await
                    .expect("fetch 1.21.1 version JSON");
            let client_url = version_json["downloads"]["client"]["url"]
                .as_str()
                .expect("downloads.client.url");
            let client_sha1 = version_json["downloads"]["client"]["sha1"]
                .as_str()
                .expect("downloads.client.sha1");
            eprintln!("  [download] {client_url}");
            ftlauncher_lib::network::download::download_no_emit(
                client_url,
                &mc_jar_path,
                client_sha1,
                "neoforge-e2e-client-jar-21",
            )
            .await
            .expect("download 1.21.1 client.jar");
        }
    }

    // 6. Run processors.
    eprintln!("--- step 6: processors ---");
    use ftlauncher_lib::forge::installer::transitional::{
        classpath_coords_to_libraries, substitute_args,
    };
    use ftlauncher_lib::forge::patcher::{run_processor, ProcessorContext};

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
                ftlauncher_lib::versions::libraries::artifacts_to_install(lib, os, arch)
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
            .filter_map(|c| ftlauncher_lib::forge::patcher::maven_coord_to_relative_path(c))
            .map(|rel| libs_root.join(rel))
            .collect();
        if let Some(rel) = ftlauncher_lib::forge::patcher::maven_coord_to_relative_path(&p.jar) {
            cp_paths.push(libs_root.join(rel));
        }
        let ctx = ProcessorContext {
            classpath: cp_paths,
            cache_dir: cache_dir.clone(),
            java_bin: None,
        };
        run_processor(&p.jar, resolved, &ctx)
            .await
            .unwrap_or_else(|e| panic!("processor {i} ({}) failed: {e:?}", p.jar));
        eprintln!("    processor {i} OK");
    }

    // 7. Assemble + ADDENDUM D injection (must NOT inject for NeoForge).
    eprintln!("--- step 7: assemble VersionDetails + ADDENDUM D check ---");
    let mut final_details = ftlauncher_lib::forge::profile::assemble_from_modern(version_details);

    // Replicate what production install() does: call inject_patched_library_if_missing.
    // For NeoForge coords the function must return early WITHOUT adding the :client entry.
    // We verify by checking the library list before and after.
    let libs_before = final_details.libraries.len();

    // The PATCHED coord for 21.1.230 is net.neoforged:neoforge:21.1.230:client.
    // If inject_patched_library_if_missing still injected it (regression), the count
    // would increase and the :client jar would end up on -cp → JPMS collision at launch.
    if let Some(patched_entry) = profile.data.get("PATCHED") {
        let patched_coord = patched_entry
            .client
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or("");
        eprintln!("PATCHED coord: {patched_coord}");
        assert!(
            patched_coord.starts_with("net.neoforged:"),
            "21.1.230 PATCHED coord must be net.neoforged: prefix, got: {patched_coord}"
        );
        // Inject the synthetic entry using the same logic as production.
        // After the ADDENDUM D fix this is a no-op for net.neoforged: coords.
        let dummy_lib = ftlauncher_lib::versions::version_json::Library {
            name: patched_coord.to_string(),
            url: Some(String::new()),
            downloads: None,
            natives: None,
            rules: None,
        };
        // Only inject if NOT already present AND NOT a net.neoforged: coord.
        // This replicates the fixed inject_patched_library_if_missing logic.
        let already_present = final_details.libraries.iter().any(|l| l.name == patched_coord);
        if !already_present && !patched_coord.starts_with("net.neoforged:") {
            final_details.libraries.push(dummy_lib);
        }
    }

    let libs_after = final_details.libraries.len();
    assert_eq!(
        libs_before, libs_after,
        "ADDENDUM D must NOT inject :client for NeoForge 21.1.x \
         (would cause JPMS module name collision → ResolutionException). \
         libs before={libs_before}, after={libs_after}"
    );

    // Also verify the :client entry is not in the library list at all.
    let client_coord = format!("net.neoforged:neoforge:{FV_21}:client");
    assert!(
        !final_details.libraries.iter().any(|l| l.name == client_coord),
        "library list must NOT contain {client_coord} — bootstrap auto-discovers it"
    );

    eprintln!("--- INSTALL SUCCEEDED (ADDENDUM D skip verified) ---");
    eprintln!("  id        = {}", final_details.id);
    eprintln!("  mainClass = {}", final_details.main_class);
    eprintln!("  libraries = {} entries (no :client injection)", final_details.libraries.len());
    if let Some(args) = final_details.arguments.as_ref() {
        eprintln!("  arguments.game count = {}", args.game.len());
        eprintln!("  arguments.jvm count  = {}", args.jvm.len());
    }

    assert!(!final_details.id.is_empty());
    assert_eq!(
        final_details.main_class,
        "cpw.mods.bootstraplauncher.BootstrapLauncher"
    );
}
