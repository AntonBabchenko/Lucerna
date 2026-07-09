//! Fabric/Quilt prebuilt server-launcher download assembly.
use lucerna_lib::servers_runtime::create::create_fabric_server;
use lucerna_lib::servers_runtime::create::create_quilt_server;
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

#[tokio::test]
async fn quilt_server_downloads_launcher_jar() {
    let _g = test_lock();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/server/jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"quilt-launcher".to_vec()))
        .mount(&server)
        .await;
    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

    let base = tempdir().unwrap();
    let file = sample("srv-q", ServerCore::Quilt);
    let jar_url = format!("{}/server/jar", server.uri());
    create_quilt_server(base.path(), &file, &jar_url)
        .await
        .unwrap();

    let p = lucerna_lib::paths::server_paths(base.path(), "srv-q");
    assert!(p.runtime.join("server.jar").exists());
    assert!(p.runtime.join("eula.txt").exists());
}
