//! Integration test: drive a Modrinth `.mrpack` parse through the
//! `mods::modpack::modrinth` parser and assert every file URL host
//! falls within the documented allowlist (`network::allowlist.rs`).
//!
//! This is the v0.5.0 sub-4 counterpart to `mod_browser_self_audit.rs`.
//! Drift between the hosts a parsed mrpack publishes in its
//! `downloads[]` array and the allowlist is a build failure.
//!
//! The CurseForge parser path is covered indirectly. Its only outbound
//! call is the Eternal API bulk-resolve hitting `api.curseforge.com`,
//! which is already in the production allowlist (from sub-3
//! mod-browser slice); the call shape is exercised by the unit tests
//! in `mods/modpack/curseforge.rs`. A wiremock'd end-to-end pass would
//! require setting `FTLAUNCHER_EXTRA_ALLOWED_HOSTS=127.0.0.1` and is
//! deferred — the static assertion in `mod_browser_self_audit.rs`
//! already covers the `api.curseforge.com` host.

use ftlauncher_lib::mods::modpack;
use ftlauncher_lib::network::allowlist::is_host_allowed;

#[tokio::test(flavor = "multi_thread")]
async fn modpack_inspect_hits_only_allowlisted_hosts() {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    // Build a minimal but valid .mrpack in-memory: a zip whose single
    // entry is `modrinth.index.json` describing one mod file hosted on
    // `cdn.modrinth.com` (the only host the parser accepts; see
    // `mods/modpack/modrinth.rs::ALLOWED_HOST`).
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
        let index = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "T",
            "versionId": "1",
            "dependencies": { "minecraft": "1.20.1", "fabric-loader": "0.15.0" },
            "files": [{
                "path": "mods/x.jar",
                "hashes": { "sha1": "abc" },
                "downloads": ["https://cdn.modrinth.com/data/x/versions/y/x.jar"],
                "fileSize": 1
            }]
        }"#;
        w.start_file("modrinth.index.json", SimpleFileOptions::default())
            .expect("zip start_file");
        w.write_all(index.as_bytes()).expect("zip write");
        w.finish().expect("zip finish");
    }

    let summary = modpack::modrinth::parse(&buf).expect("modrinth parser must accept the fixture");
    assert!(
        !summary.files.is_empty(),
        "fixture has one file; parser dropped it silently",
    );
    for f in &summary.files {
        let parsed = url::Url::parse(&f.url)
            .unwrap_or_else(|e| panic!("modpack file URL must parse: {} ({})", f.url, e));
        let host = parsed
            .host_str()
            .expect("modpack file URL must have a host")
            .to_string();
        assert!(
            is_host_allowed(&host),
            "modpack file URL {} hits non-allowlisted host {}",
            f.url,
            host,
        );
    }
}
