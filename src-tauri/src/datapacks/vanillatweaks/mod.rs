//! Vanilla Tweaks datapack builder.
//!
//! VT is a builder, not a catalogue: you tick packs and the site builds a zip
//! on demand. There are no project ids and no file hashes — but every pack
//! does carry a version string, which is what makes update checking a
//! comparison rather than a blind rebuild. See
//! `docs/superpowers/specs/2026-08-05-vanilla-tweaks-design.md`.

pub mod client;
pub mod family;
pub mod platform;
pub mod unpack;

pub use client::{VtCatalogue, VtCategory, VtClient, VtPack};
pub use family::family_for;
pub use platform::VanillaTweaksPlatform;
pub use unpack::split_bundle;

/// Build `selection` and return `(filename, bytes)` per pack. The filenames
/// come from the bundle, never from a prediction: VT names its inner zips
/// `<pack> v<version>.zip`, so the name changes with every release and the
/// bundle is the only authority on it.
pub async fn build_selection_with(
    client: &VtClient,
    family: &str,
    selection: &[(String, Vec<String>)],
) -> crate::error::Result<Vec<(String, Vec<u8>)>> {
    let link = client.build_link(family, selection).await?;
    let bytes = client.download_bundle(&link).await?;
    split_bundle(&bytes)
}

/// Production entry point for a whole selection.
pub async fn build_selection(
    family: &str,
    selection: &[(String, Vec<String>)],
) -> crate::error::Result<Vec<(String, Vec<u8>)>> {
    build_selection_with(&VtClient::new(), family, selection).await
}

/// One pack, for the update path. `project_id` is `<category>/<name>`.
pub async fn build_one_with(
    client: &VtClient,
    family: &str,
    project_id: &str,
) -> crate::error::Result<(String, Vec<u8>)> {
    let (category, name) = project_id.split_once('/').ok_or_else(|| {
        crate::error::Error::VanillaTweaksBuildFailed {
            message: format!("'{project_id}' is not a <category>/<name> pack id"),
        }
    })?;
    let mut packs = build_selection_with(
        client,
        family,
        &[(category.to_string(), vec![name.to_string()])],
    )
    .await?;
    if packs.is_empty() {
        // An empty bundle is not "nothing to do": we asked for one pack and
        // got none, which means the update did not happen.
        return Err(crate::error::Error::VanillaTweaksBuildFailed {
            message: format!("the bundle built for '{project_id}' contained no pack"),
        });
    }
    Ok(packs.remove(0))
}

/// Production entry point for one pack.
pub async fn build_one(family: &str, project_id: &str) -> crate::error::Result<(String, Vec<u8>)> {
    build_one_with(&VtClient::new(), family, project_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn bundle(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, body) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(body).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    async fn serving(bundle_bytes: Vec<u8>) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/assets/server/zipdatapacks.php"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"status":"success","link":"/dl/b.zip","message":""}"#),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/dl/b.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bundle_bytes))
            .mount(&s)
            .await;
        s
    }

    #[tokio::test]
    async fn build_selection_returns_every_pack_under_its_bundle_name() {
        let s = serving(bundle(&[
            ("graves v2.8.5.zip", b"one"),
            ("armor statues v2.8.21.zip", b"two"),
        ]))
        .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        let out = build_selection_with(
            &VtClient::with_base(s.uri()),
            "1.21",
            &[("survival".into(), vec!["graves".into()])],
        )
        .await
        .unwrap();

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "graves v2.8.5.zip");
        assert_eq!(out[0].1, b"one");
    }

    #[tokio::test]
    async fn a_bundle_over_the_cap_is_refused_before_it_is_split() {
        // The limit is injected rather than using MAX_VT_BUNDLE_BYTES: the
        // real cap is 2 GiB, and a test must not allocate it to prove the
        // comparison works.
        let s = serving(bundle(&[("small v1.0.zip", b"body")])).await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        let c = VtClient::with_base(s.uri());
        let err = c
            .download_bundle_with_limit(&format!("{}/dl/b.zip", s.uri()), 4)
            .await
            .unwrap_err();

        assert!(
            matches!(err, crate::error::Error::VanillaTweaksBundleTooLarge { .. }),
            "expected VanillaTweaksBundleTooLarge, got {err:?}"
        );
    }

    #[test]
    fn the_bundle_cap_is_larger_than_the_per_pack_cap() {
        // The whole point of the second constant: a multi-pack build cannot be
        // bounded by the single-pack limit.
        assert!(crate::datapacks::MAX_VT_BUNDLE_BYTES > crate::datapacks::MAX_DATAPACK_BYTES);
    }

    #[tokio::test]
    async fn build_one_answers_the_single_pack_it_asked_for() {
        let s = serving(bundle(&[("graves v2.8.5.zip", b"one")])).await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        let (name, bytes) =
            build_one_with(&VtClient::with_base(s.uri()), "1.21", "survival/graves")
                .await
                .unwrap();
        assert_eq!(name, "graves v2.8.5.zip");
        assert_eq!(bytes, b"one");
    }

    #[tokio::test]
    async fn build_one_refuses_an_id_without_a_category() {
        let err = build_one_with(&VtClient::new(), "1.21", "graves")
            .await
            .unwrap_err();
        assert!(
            matches!(err, crate::error::Error::VanillaTweaksBuildFailed { .. }),
            "expected VanillaTweaksBuildFailed, got {err:?}"
        );
    }

    #[tokio::test]
    async fn build_one_refuses_an_empty_bundle_rather_than_reporting_success() {
        let s = serving(bundle(&[])).await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        let err = build_one_with(&VtClient::with_base(s.uri()), "1.21", "survival/graves")
            .await
            .unwrap_err();
        assert!(
            matches!(err, crate::error::Error::VanillaTweaksBuildFailed { .. }),
            "expected VanillaTweaksBuildFailed, got {err:?}"
        );
    }
}
