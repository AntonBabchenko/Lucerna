//! Fabric prebuilt server-launcher download; Quilt server assembly (Quilt has
//! no prebuilt server jar — see `servers_runtime::quilt`).
use lucerna_lib::servers_runtime::create::create_fabric_server;
use lucerna_lib::servers_runtime::create::create_quilt_server;
use lucerna_lib::servers_runtime::quilt::{
    launch_jar, parse_server_profile, QuiltServerProfile, LAUNCH_JAR, SERVER_LAUNCHER_MAIN,
};
use lucerna_lib::servers_runtime::schema::{ServerCore, ServerFile};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn sample(id: &str, loader: ServerCore) -> ServerFile {
    ServerFile {
        id: id.into(),
        name: "S".into(),
        mc_version: "1.20.4".into(),
        loader,
        loader_version: Some("0.16.5".into()),
        max_heap_mb: 2048,
        extra_jvm_args: String::new(),
        created_unix_ms: 1.0,
        eula_accepted: true,
        created_from_instance: None,
        handled_log_sig: None,
        java_component: None,
        upload: None,
    }
}

#[tokio::test]
async fn fabric_server_downloads_launcher_jar() {
    let _g = test_lock();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/server/jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"fabric-launcher".to_vec()))
        .mount(&server)
        .await;
    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

    let base = tempdir().unwrap();
    let file = sample("srv-f", ServerCore::Fabric);
    let jar_url = format!("{}/server/jar", server.uri());
    create_fabric_server(base.path(), &file, &jar_url)
        .await
        .unwrap();

    let p = lucerna_lib::paths::server_paths(base.path(), "srv-f");
    assert!(p.runtime.join("server.jar").exists());
    assert!(p.runtime.join("eula.txt").exists());
}

const LOADER_PATH: &str = "/org/quiltmc/quilt-loader/0.26.0/quilt-loader-0.26.0.jar";
const INTERMEDIARY_PATH: &str = "/net/fabricmc/intermediary/1.20.4/intermediary-1.20.4.jar";
const VANILLA_PATH: &str = "/v1/objects/server.jar";

fn quilt_profile(maven: &str) -> QuiltServerProfile {
    let json = serde_json::json!({
        "launcherMainClass": SERVER_LAUNCHER_MAIN,
        "libraries": [
            { "name": "org.quiltmc:quilt-loader:0.26.0", "url": format!("{maven}/") },
            { "name": "net.fabricmc:intermediary:1.20.4", "url": format!("{maven}/") }
        ]
    });
    parse_server_profile(json, "1.20.4").unwrap()
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    hex::encode(Sha1::digest(bytes))
}

async fn mount(server: &MockServer, at: &str, status: u16, body: &[u8]) {
    Mock::given(method("GET"))
        .and(path(at.to_string()))
        .respond_with(ResponseTemplate::new(status).set_body_bytes(body.to_vec()))
        .mount(server)
        .await;
}

/// The launch jar's manifest with continuation lines joined back.
fn read_manifest(jar: &std::path::Path) -> String {
    let file = std::fs::File::open(jar).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name("META-INF/MANIFEST.MF").unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
    text.replace("\r\n ", "")
}

#[tokio::test]
async fn quilt_server_is_assembled_in_quilt_layout() {
    let _g = test_lock();
    let server = MockServer::start().await;
    mount(&server, LOADER_PATH, 200, b"loader").await;
    mount(&server, INTERMEDIARY_PATH, 200, b"intermediary").await;
    mount(&server, VANILLA_PATH, 200, b"vanilla").await;
    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

    let base = tempdir().unwrap();
    let file = sample("srv-q", ServerCore::Quilt);
    let vanilla_url = format!("{}{VANILLA_PATH}", server.uri());
    create_quilt_server(
        base.path(),
        &file,
        &quilt_profile(&server.uri()),
        &vanilla_url,
        &sha1_hex(b"vanilla"),
    )
    .await
    .unwrap();

    let rt = lucerna_lib::paths::server_paths(base.path(), "srv-q").runtime;
    assert_eq!(
        std::fs::read(rt.join("libraries").join(&LOADER_PATH[1..])).unwrap(),
        b"loader"
    );
    assert_eq!(
        std::fs::read(rt.join("libraries").join(&INTERMEDIARY_PATH[1..])).unwrap(),
        b"intermediary"
    );
    assert_eq!(std::fs::read(rt.join("server.jar")).unwrap(), b"vanilla");
    let manifest = read_manifest(&rt.join(LAUNCH_JAR));
    assert!(
        manifest.contains(&format!("Main-Class: {SERVER_LAUNCHER_MAIN}\r\n")),
        "{manifest}"
    );
    assert!(
        manifest.contains(&format!(
            "Class-Path: libraries{LOADER_PATH} libraries{INTERMEDIARY_PATH}\r\n"
        )),
        "{manifest}"
    );
    assert!(rt.join("eula.txt").exists());
    assert_eq!(launch_jar(&rt).unwrap(), LAUNCH_JAR);
}

#[tokio::test]
async fn quilt_server_with_a_bad_vanilla_jar_gets_no_launcher() {
    let _g = test_lock();
    let server = MockServer::start().await;
    mount(&server, LOADER_PATH, 200, b"loader").await;
    mount(&server, INTERMEDIARY_PATH, 200, b"intermediary").await;
    mount(&server, VANILLA_PATH, 200, b"tampered").await;
    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

    let base = tempdir().unwrap();
    let file = sample("srv-q", ServerCore::Quilt);
    let vanilla_url = format!("{}{VANILLA_PATH}", server.uri());
    let r = create_quilt_server(
        base.path(),
        &file,
        &quilt_profile(&server.uri()),
        &vanilla_url,
        &sha1_hex(b"vanilla"),
    )
    .await;

    assert!(r.is_err(), "a SHA-1 mismatch must fail the assembly");
    let rt = lucerna_lib::paths::server_paths(base.path(), "srv-q").runtime;
    assert!(
        !rt.join(LAUNCH_JAR).exists(),
        "no launch jar for a half-built server"
    );
    assert!(launch_jar(&rt).is_err());
}

#[tokio::test]
async fn quilt_server_with_a_missing_library_gets_no_launcher() {
    let _g = test_lock();
    let server = MockServer::start().await;
    mount(&server, LOADER_PATH, 200, b"loader").await;
    mount(&server, INTERMEDIARY_PATH, 404, b"").await;
    mount(&server, VANILLA_PATH, 200, b"vanilla").await;
    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

    let base = tempdir().unwrap();
    let file = sample("srv-q", ServerCore::Quilt);
    let vanilla_url = format!("{}{VANILLA_PATH}", server.uri());
    let r = create_quilt_server(
        base.path(),
        &file,
        &quilt_profile(&server.uri()),
        &vanilla_url,
        &sha1_hex(b"vanilla"),
    )
    .await;

    assert!(r.is_err(), "a missing library must fail the assembly");
    let rt = lucerna_lib::paths::server_paths(base.path(), "srv-q").runtime;
    assert!(
        !rt.join(LAUNCH_JAR).exists(),
        "no launch jar for a half-built server"
    );
}
