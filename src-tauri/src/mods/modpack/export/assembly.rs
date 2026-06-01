//! Zip assembly + overrides.

use std::path::Path;

use sha1::Sha1;
use sha2::{Digest as Sha2Digest, Sha512};

use crate::error::Error;

/// Lowercase hex sha1 + sha512 and byte size of the file at `path`.
/// Reads the whole file once. Used to build `.mrpack` file hashes from the
/// local jar (the source of truth — we do not trust the registry's sha1).
pub fn hash_file(path: &Path) -> Result<(String, String, u64), Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    // sha1 0.11 and sha2 0.10 pull in different versions of the `digest`
    // crate; use fully-qualified trait calls to avoid the ambiguity.
    let sha1 = {
        use sha1::Digest as Sha1Digest;
        hex::encode(Sha1::digest(&bytes))
    };
    let sha512 = hex::encode(Sha512::digest(&bytes));
    Ok((sha1, sha512, bytes.len() as u64))
}

use crate::mods::platform::ModSource;

/// Resolve the canonical download URL for a referenced mod. Modrinth: the
/// primary file URL from the version endpoint. CurseForge: the forgecdn
/// download URL (also an allowed mrpack download host). Returns
/// `Ok(None)` when the platform refuses distribution (CF disabled) — the
/// caller falls back to bundling the local jar.
pub async fn resolve_download_url(
    source: ModSource,
    project_id: &str,
    version_id: &str,
) -> Result<Option<String>, Error> {
    match source {
        ModSource::Modrinth => {
            let url =
                format!("https://api.modrinth.com/v2/project/{project_id}/version/{version_id}");
            let resp = crate::network::request::get(
                &url,
                &[("user-agent", "AntonBabchenko/Lucerna")],
                "modpacks",
            )
            .await
            .map_err(|e| Error::ModsNetwork {
                url: url.clone(),
                details: e.to_string(),
            })?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            #[derive(serde::Deserialize)]
            struct V {
                files: Vec<F>,
            }
            #[derive(serde::Deserialize)]
            struct F {
                url: String,
                primary: bool,
            }
            let v: V = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
            Ok(v.files
                .iter()
                .find(|f| f.primary)
                .or_else(|| v.files.first())
                .map(|f| f.url.clone()))
        }
        ModSource::Curseforge => {
            let key = crate::mods::curseforge::keyring::get().ok().flatten();
            match crate::mods::modpack::cf_api::resolve_file_download(
                "https://api.curseforge.com",
                key.as_deref(),
                project_id,
                version_id,
            )
            .await
            {
                Ok(url) => Ok(Some(url)),
                // Distribution disabled → caller bundles the local jar instead.
                Err(Error::ModpackCfDistributionDisabled { .. }) => Ok(None),
                Err(e) => Err(e),
            }
        }
    }
}

#[cfg(test)]
mod hash_tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn hashes_known_bytes() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"abc").unwrap();
        let (sha1, sha512, size) = hash_file(f.path()).unwrap();
        // Known digests of "abc".
        assert_eq!(sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha512,
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(size, 3);
    }
}
