//! Real-network smoke tests for "own server" assembly (Plan 1).
//!
//! Ignored by default — they hit LIVE Mojang/Fabric APIs and download real
//! server jars, so they must never run in CI. Run manually:
//!
//!   cargo test --test servers_real_smoke -- --ignored --nocapture
//!
//! Each test assembles a server into a fresh temp dir and prints the path so
//! you can inspect the result. The hosts used (piston-meta/piston-data.mojang.com,
//! meta.fabricmc.net, maven.fabricmc.net) are already on the network allowlist.

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::servers_runtime::schema::ServerFile;
use lucerna_lib::servers_runtime::{create, jar};
use tempfile::tempdir;

fn file(id: &str, loader: LoaderKind, loader_version: Option<&str>) -> ServerFile {
    ServerFile {
        id: id.into(),
        name: "Smoke".into(),
        mc_version: "1.20.4".into(),
        loader,
        loader_version: loader_version.map(|s| s.into()),
        max_heap_mb: 2048,
        extra_jvm_args: String::new(),
        created_unix_ms: 1.0,
        eula_accepted: true,
        created_from_instance: None,
        handled_log_sig: None,
    }
}

#[tokio::test]
#[ignore = "real network: resolves + downloads a real vanilla server jar (~50MB)"]
async fn real_vanilla_assembly() {
    let base = tempdir().unwrap();
    let f = file("srv-vanilla", LoaderKind::Vanilla, None);

    let (url, sha1) = create::resolve_vanilla_jar(&f.mc_version)
        .await
        .expect("resolve vanilla server jar from manifest");
    eprintln!("[vanilla] server jar url = {url}");

    create::create_vanilla_server(base.path(), &f, &url, &sha1)
        .await
        .expect("assemble vanilla server");

    let p = lucerna_lib::paths::server_paths(base.path(), "srv-vanilla");
    let size = std::fs::metadata(p.runtime.join("server.jar"))
        .unwrap()
        .len();
    eprintln!(
        "[vanilla] assembled at {} ({size} bytes)",
        p.runtime.display()
    );

    assert!(
        size > 1_000_000,
        "real vanilla server jar should be multi-MB"
    );
    assert!(
        std::fs::read_to_string(p.runtime.join("eula.txt"))
            .unwrap()
            .contains("eula=true"),
        "eula.txt written and accepted"
    );
}

#[tokio::test]
#[ignore = "real network: resolves + downloads a real fabric server launcher jar"]
async fn real_fabric_assembly() {
    let base = tempdir().unwrap();
    let f = file("srv-fabric", LoaderKind::Fabric, Some("0.16.5"));

    let installer = create::latest_fabric_installer(&f.mc_version)
        .await
        .expect("fetch latest fabric installer version");
    eprintln!("[fabric] installer version = {installer}");

    let url = jar::fabric_server_jar_url(
        &f.mc_version,
        f.loader_version.as_ref().unwrap(),
        &installer,
    );
    eprintln!("[fabric] server jar url = {url}");

    create::create_fabric_server(base.path(), &f, &url)
        .await
        .expect("assemble fabric server");

    let p = lucerna_lib::paths::server_paths(base.path(), "srv-fabric");
    let size = std::fs::metadata(p.runtime.join("server.jar"))
        .unwrap()
        .len();
    eprintln!(
        "[fabric] assembled at {} ({size} bytes)",
        p.runtime.display()
    );

    assert!(size > 1000, "fabric server launcher jar present");
    assert!(
        std::fs::read_to_string(p.runtime.join("eula.txt"))
            .unwrap()
            .contains("eula=true"),
        "eula.txt written and accepted"
    );
}
