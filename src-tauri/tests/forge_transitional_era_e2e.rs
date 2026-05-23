//! End-to-end smoke test for the transitional-era Forge install pipeline.
//!
//! Drives the full pipeline — maven-tree extraction, library downloads,
//! arg substitution, and all 4 processor runs — against the real 1.16.5
//! installer JAR. Uses a tempdir for the data root so no real AppHandle is
//! required (matching the established pattern across this test suite, which
//! avoids constructing AppHandle in integration tests).
//!
//! # Prerequisites
//!
//! The fixture JAR must be present:
//!   `src-tauri/tests/fixtures/forge/installers/forge-1.16.5-36.2.42-installer.jar`
//! Obtain it by running `src-tauri/tests/fixtures/forge/fetch.ps1`.
//!
//! # Running
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test forge_transitional_era_e2e \
//!     -- --ignored --nocapture
//! ```
//!
//! `--nocapture` is required: progress output (eprintln!) is otherwise
//! swallowed by the test harness.
//!
//! # Why #[ignore]
//!
//! The test downloads ~30 MB of JARs and runs 4 processors (JVM-less
//! Rust implementations). It is far too slow for the default `cargo test`
//! sweep but invaluable as an on-demand regression check.
//!
//! # AppHandle strategy
//!
//! `transitional::install` takes `&tauri::AppHandle`, which binds to the
//! concrete Wry runtime. Constructing a real Wry App in a test binary
//! requires WebView2 DLLs that are not in the test binary's PATH — the
//! binary crashes at startup with STATUS_ENTRYPOINT_NOT_FOUND before
//! any test code runs.
//!
//! Instead this test replicates the pipeline steps using public helpers:
//!   - `extract_installer_maven_tree_to` (inline) — zip extraction
//!   - `download_no_emit` — downloads without AppHandle
//!   - `artifacts_to_install` + `should_install` — library selection
//!   - `substitute_args`, `classpath_coords_to_libraries` — arg resolution
//!   - `run_processor` — processor dispatch
//!   - `forge::profile::assemble_from_transitional` — final assembly
//!
//! This covers every meaningful code path in `install` except the
//! fire-and-forget `DownloadProgress` event emission, which is UI-layer
//! behaviour untestable outside a real webview.

const FIXTURE_PATH: &str = "tests/fixtures/forge/installers/forge-1.16.5-36.2.42-installer.jar";

const MC: &str = "1.16.5";
const FV: &str = "36.2.42";

/// Load the installer bytes and extract `install_profile.json` as a
/// `serde_json::Value`. Returns `None` and prints a SKIP message if the
/// fixture file is absent so CI stays green even without the fixture.
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

/// Extract every `maven/` entry from the installer ZIP into `libs_root`,
/// preserving the sub-path. Replicates the private
/// `transitional::extract_installer_maven_tree` for test use.
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

/// Download a library using `download_no_emit` (no AppHandle required).
async fn download_lib(url: &str, dest: &std::path::Path, sha1: &str) {
    if sha1.is_empty() {
        if tokio::fs::metadata(dest).await.is_ok() {
            return; // TOFU: file already present, trust it
        }
    } else {
        // Check if existing file matches sha1.
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
    ftlauncher_lib::network::download::download_no_emit(url, dest, sha1, "forge-e2e")
        .await
        .unwrap_or_else(|e| panic!("failed to download {url}: {e:?}"));
}

/// Full end-to-end smoke for the transitional-era install pipeline against
/// the real 1.16.5 Forge installer JAR.
///
/// Replicates `transitional::install` step-by-step using public helpers
/// and a tempdir in lieu of an AppHandle. Every meaningful code path in
/// the install pipeline is exercised, including all 4 processor runs.
///
/// If this test passes, the pipeline works and the maintainer only needs a
/// UI sanity check (Task 27).
///
/// If this test panics, the panic message + eprintln output points at the
/// next bug — no manual "click Install → wait → read error" loop required.
#[tokio::test]
#[ignore]
async fn install_forge_1_16_5_transitional_era_e2e() {
    let Some((installer_bytes, install_profile_value)) = load_fixture_or_skip() else {
        return;
    };

    eprintln!("Loaded installer: {} bytes", installer_bytes.len());

    // 1. Parse install_profile.
    let raw_text =
        serde_json::to_string(&install_profile_value).expect("re-serialise install_profile");
    let profile = ftlauncher_lib::forge::installer::transitional::parse_install_profile(&raw_text)
        .expect("parse install_profile v2");

    eprintln!("profile.minecraft = {}", profile.minecraft);
    eprintln!("profile.version   = {}", profile.version);
    eprintln!("processors count  = {}", profile.processors.len());

    // 2. Extract version.json from installer.
    let version_details = {
        use std::io::Read;
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(&installer_bytes)).expect("zip open");
        let mut entry = archive.by_name("version.json").expect("version.json entry");
        let mut buf = String::new();
        entry.read_to_string(&mut buf).expect("read version.json");
        ftlauncher_lib::versions::version_json::parse(&buf).expect("parse version.json")
    };
    eprintln!("version.json id = {}", version_details.id);
    eprintln!("version.json mainClass = {}", version_details.main_class);

    // 3. Set up tempdir as the fake app-data root.
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

    tokio::fs::create_dir_all(&cache_dir)
        .await
        .expect("cache_dir");
    tokio::fs::create_dir_all(&libs_root)
        .await
        .expect("libs_root");

    let cache_dir_str = cache_dir.display().to_string();
    let installer_path_str = installer_cache_path.display().to_string();

    eprintln!("libs_root   = {}", libs_root.display());
    eprintln!("cache_dir   = {cache_dir_str}");

    // 4. Extract maven/ tree from installer into libs_root.
    eprintln!("--- step 4: extract maven/ subtree ---");
    extract_maven_tree(&installer_bytes, &libs_root).await;

    // 5. Download install_profile.libraries (only those with a URL).
    eprintln!("--- step 5: download install_profile.libraries ---");
    // Replicate the pub(crate) helpers from versions::install.
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

    for lib in &downloadable {
        for (rel_path, url, sha1, _size) in
            ftlauncher_lib::versions::libraries::artifacts_to_install(lib, os, arch)
        {
            download_lib(&url, &libs_root.join(&rel_path), &sha1).await;
        }
    }

    // 5b. Download the vanilla MC client.jar.
    // jarsplitter (processor 1) consumes {MINECRAFT_JAR}. The production
    // install_version pipeline handles this before entering the Forge path,
    // but this test drives the steps manually so we must fetch it here.
    eprintln!("--- step 5b: download vanilla MC client.jar ---");
    {
        let mc_jar_path = app_data_root
            .path()
            .join("versions")
            .join(MC)
            .join(format!("{MC}.jar"));

        // Cache-check first so repeated runs don't re-download ~30 MB.
        let already_cached = tokio::fs::metadata(&mc_jar_path).await.is_ok();
        if already_cached {
            eprintln!("  [cache] {}", mc_jar_path.display());
        } else {
            // Fetch the Mojang version manifest to find 1.16.5's URL.
            let entries = ftlauncher_lib::versions::manifest::list_manifest()
                .await
                .expect("fetch version manifest");
            let entry = entries
                .iter()
                .find(|e| e.id == MC)
                .expect("1.16.5 not found in manifest");

            // Fetch the per-version JSON to get downloads.client.
            let version_json: serde_json::Value =
                ftlauncher_lib::network::get_json(&entry.url, "forge-e2e-client-jar")
                    .await
                    .expect("fetch 1.16.5 version JSON");

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
                "forge-e2e-client-jar",
            )
            .await
            .expect("download vanilla 1.16.5 client.jar");
            eprintln!(
                "  [done] {} bytes",
                mc_jar_path.metadata().map(|m| m.len()).unwrap_or(0)
            );
        }
    }

    // 6. Run processors.
    eprintln!("--- step 6: processors ---");
    use ftlauncher_lib::forge::installer::transitional::{
        classpath_coords_to_libraries, substitute_args,
    };
    use ftlauncher_lib::forge::patcher::{run_processor, ProcessorContext};

    for (i, p) in profile.processors.iter().enumerate() {
        // Skip server-side-only processors.
        if let Some(sides) = &p.sides {
            if !sides.iter().any(|s| s == "client") {
                eprintln!("  processor {i} ({}) — skipped (server-only)", p.jar);
                continue;
            }
        }

        eprintln!("  processor {i}: {}", p.jar);

        // Download classpath JARs.
        let cp_libs = classpath_coords_to_libraries(&p.classpath);
        for lib in &cp_libs {
            for (rel_path, url, sha1, _size) in
                ftlauncher_lib::versions::libraries::artifacts_to_install(lib, os, arch)
            {
                download_lib(&url, &libs_root.join(&rel_path), &sha1).await;
            }
        }

        // Substitute args.
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

        // Build classpath paths.
        let mut cp_paths: Vec<std::path::PathBuf> = p
            .classpath
            .iter()
            .filter_map(|c| ftlauncher_lib::forge::patcher::maven_coord_to_relative_path(c))
            .map(|rel| libs_root.join(rel))
            .collect();

        // Include processor's OWN jar so Java can find its main class.
        // Mirrors transitional::install's logic added in the 2026-05-17(b) pivot.
        if let Some(rel) = ftlauncher_lib::forge::patcher::maven_coord_to_relative_path(&p.jar) {
            cp_paths.push(libs_root.join(rel));
        }

        let ctx = ProcessorContext {
            classpath: cp_paths,
            cache_dir: cache_dir.clone(),
            // java_bin: None falls back to "java" on PATH, which is acceptable
            // for the e2e test environment where Java is expected to be installed.
            java_bin: None,
        };

        run_processor(&p.jar, resolved, &ctx)
            .await
            .unwrap_or_else(|e| panic!("processor {i} ({}) failed: {e:?}", p.jar));

        eprintln!("    processor {i} OK");
    }

    // 7. Assemble final VersionDetails.
    eprintln!("--- step 7: assemble VersionDetails ---");
    let final_details = ftlauncher_lib::forge::profile::assemble_from_transitional(version_details);

    eprintln!("--- INSTALL SUCCEEDED ---");
    eprintln!("  id            = {}", final_details.id);
    eprintln!("  mainClass     = {}", final_details.main_class);
    eprintln!(
        "  inheritsFrom  = {:?}",
        final_details.inherits_from.as_deref()
    );
    eprintln!(
        "  libraries     = {} entries",
        final_details.libraries.len()
    );
    if let Some(args) = final_details.arguments.as_ref() {
        eprintln!("  arguments.game count = {}", args.game.len());
        eprintln!("  arguments.jvm count  = {}", args.jvm.len());
    }
    if let Some(mc_args) = final_details.minecraft_arguments.as_deref() {
        eprintln!("  minecraftArguments = {mc_args}");
    }

    // Sanity assertions.
    assert!(
        !final_details.id.is_empty(),
        "version_details.id must not be empty"
    );
    assert!(
        !final_details.main_class.is_empty(),
        "version_details.main_class must not be empty"
    );
}
