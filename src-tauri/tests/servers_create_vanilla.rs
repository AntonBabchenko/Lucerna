//! Vanilla server assembly against a mocked server jar.
//!
//! Tests `create_vanilla_server` end-to-end: writes `server.json`,
//! downloads the jar to `runtime/server.jar`, and writes `eula.txt`.

use lucerna_lib::instances::schema::LoaderKind;
use lucerna_lib::servers_runtime::create::create_vanilla_server;
use lucerna_lib::servers_runtime::schema::ServerFile;
use sha1::{Digest, Sha1};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Serialize tests that mutate `LUCERNA_EXTRA_ALLOWED_HOSTS`.
fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

#[tokio::test]
async fn vanilla_server_assembled_into_runtime() {
    let _g = test_lock();
    let server = MockServer::start().await;
    let jar_body = b"fake-server-jar";
    let sha1 = sha1_hex(jar_body);

    Mock::given(method("GET"))
        .and(path("/server.jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(jar_body.to_vec()))
        .mount(&server)
        .await;
    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

    let base = tempdir().unwrap();
    let file = ServerFile {
        id: "srv-1".into(),
        name: "Vanilla".into(),
        mc_version: "1.20.4".into(),
        loader: LoaderKind::Vanilla,
        loader_version: None,
        max_heap_mb: 2048,
        extra_jvm_args: String::new(),
        created_unix_ms: 1.0,
        eula_accepted: true,
        created_from_instance: None,
        handled_log_sig: None,
        upload: None,
    };
    let jar_url = format!("{}/server.jar", server.uri());

    create_vanilla_server(base.path(), &file, &jar_url, &sha1)
        .await
        .unwrap();

    let p = lucerna_lib::paths::server_paths(base.path(), "srv-1");
    assert!(p.json.exists(), "server.json written");
    assert!(
        p.runtime.join("server.jar").exists(),
        "jar downloaded into runtime/"
    );
    assert!(
        p.runtime.join("eula.txt").exists(),
        "eula.txt written into runtime/"
    );
    assert!(
        std::fs::read_to_string(p.runtime.join("eula.txt"))
            .unwrap()
            .contains("eula=true"),
        "eula must be accepted"
    );
}

#[tokio::test]
async fn vanilla_server_eula_gate_rejects_unaccepted() {
    let _g = test_lock();
    let base = tempdir().unwrap();
    let file = ServerFile {
        id: "srv-2".into(),
        name: "No EULA".into(),
        mc_version: "1.20.4".into(),
        loader: LoaderKind::Vanilla,
        loader_version: None,
        max_heap_mb: 2048,
        extra_jvm_args: String::new(),
        created_unix_ms: 1.0,
        eula_accepted: false,
        created_from_instance: None,
        handled_log_sig: None,
        upload: None,
    };

    let err = create_vanilla_server(base.path(), &file, "https://example.com/x.jar", "")
        .await
        .unwrap_err();
    assert!(
        matches!(err, lucerna_lib::error::Error::ServerEulaNotAccepted),
        "expected ServerEulaNotAccepted, got: {err:?}"
    );
}
