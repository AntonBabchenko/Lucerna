//! Account profile skin: fetch the player's public skin head by UUID
//! through the `network::` chokepoint, cache the PNG on disk, and expose
//! it to the UI. Skins are public (no token required); the avatar is
//! cosmetic, so every failure degrades silently to `None`.

use crate::error::Result;
use base64::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

/// Skin payload returned to the UI. The full skin PNG is base64-encoded;
/// the head is cropped client-side onto a canvas.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct AccountSkin {
    pub uuid: String,
    pub texture_url: String,
    pub skin_png_base64: String,
}

/// One entry in the sessionserver profile `properties` array.
#[derive(Debug, Deserialize)]
struct ProfileProperty {
    name: String,
    value: String,
}

/// Minimal shape of the sessionserver profile response we care about.
#[derive(Debug, Deserialize)]
struct ProfileWithProperties {
    #[serde(default)]
    properties: Vec<ProfileProperty>,
}

/// Decoded `textures` property payload (base64-decoded from the property value).
#[derive(Debug, Deserialize)]
struct TexturesPayload {
    textures: TextureMap,
}

#[derive(Debug, Deserialize)]
struct TextureMap {
    #[serde(rename = "SKIN")]
    skin: Option<TextureEntry>,
}

#[derive(Debug, Deserialize)]
struct TextureEntry {
    url: String,
}

/// Extract the SKIN texture URL from a profile's `properties`. Returns
/// `None` when there is no `textures` property, the base64 is invalid,
/// the inner JSON is malformed, or there is no SKIN entry (default skin).
fn skin_url_from_properties(props: &[ProfileProperty]) -> Option<String> {
    let textures_b64 = props.iter().find(|p| p.name == "textures")?.value.as_str();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(textures_b64)
        .ok()?;
    let payload: TexturesPayload = serde_json::from_slice(&decoded).ok()?;
    payload.textures.skin.map(|s| s.url)
}

const SESSIONSERVER_DEFAULT: &str = "https://sessionserver.mojang.com";

/// Sessionserver base URL, overridable for tests via
/// `LUCERNA_SESSIONSERVER_URL_OVERRIDE` (mirrors the MS-auth URL-override
/// pattern in `accounts/microsoft/mc_services.rs`).
fn sessionserver_base() -> String {
    std::env::var("LUCERNA_SESSIONSERVER_URL_OVERRIDE")
        .unwrap_or_else(|_| SESSIONSERVER_DEFAULT.to_string())
}

fn profile_url(uuid_no_dashes: &str) -> String {
    format!(
        "{}/session/minecraft/profile/{}",
        sessionserver_base(),
        uuid_no_dashes
    )
}

/// Fetch the player's skin by UUID. Returns `Ok(None)` when there is no
/// usable skin: the profile is absent (204 / non-2xx), carries no SKIN
/// texture, or the texture itself is missing (non-2xx / empty body).
/// `Err` only on a transport-level failure (no HTTP status received) — the
/// orchestrator degrades that to `None` too.
pub async fn fetch_skin(uuid: &str) -> Result<Option<AccountSkin>> {
    let uuid_no_dashes = uuid.replace('-', "");
    let url = profile_url(&uuid_no_dashes);
    let resp = crate::network::request::get(&url, &[], "account_skin").await?;
    // 204 (no such profile / offline UUID), empty body, or any non-2xx → no skin.
    if resp.status == 204 || resp.body.is_empty() || !(200..300).contains(&resp.status) {
        return Ok(None);
    }
    let profile: ProfileWithProperties = match serde_json::from_slice(&resp.body) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(texture_url) = skin_url_from_properties(&profile.properties) else {
        return Ok(None);
    };
    let tex_resp = crate::network::request::get(&texture_url, &[], "account_skin").await?;
    if !(200..300).contains(&tex_resp.status) || tex_resp.body.is_empty() {
        return Ok(None);
    }
    let skin_png_base64 = base64::engine::general_purpose::STANDARD.encode(&tex_resp.body);
    Ok(Some(AccountSkin {
        uuid: uuid.to_string(),
        texture_url,
        skin_png_base64,
    }))
}

/// Lazy-path cache freshness window. Sign-in / refresh prefetch with
/// `force=true` bypasses this; the lazy display path honors it.
const SKIN_CACHE_TTL_SECS: f64 = 6.0 * 3600.0;

#[derive(Debug, Serialize, Deserialize)]
struct SkinMeta {
    texture_url: String,
    fetched_at: f64,
}

/// Read a cached skin if present and fresh (`now - fetched_at <= ttl`).
/// Pure over the filesystem — takes the dir + clock so it is unit-testable.
fn read_cached_skin(skins_dir: &Path, uuid: &str, ttl_secs: f64, now: f64) -> Option<AccountSkin> {
    let meta_raw = std::fs::read_to_string(skins_dir.join(format!("{uuid}.json"))).ok()?;
    let meta: SkinMeta = serde_json::from_str(&meta_raw).ok()?;
    if now - meta.fetched_at > ttl_secs {
        return None;
    }
    let png = std::fs::read(skins_dir.join(format!("{uuid}.png"))).ok()?;
    Some(AccountSkin {
        uuid: uuid.to_string(),
        texture_url: meta.texture_url,
        skin_png_base64: base64::engine::general_purpose::STANDARD.encode(&png),
    })
}

/// Persist a freshly-fetched skin (raw PNG + meta sidecar).
fn write_cached_skin(skins_dir: &Path, skin: &AccountSkin, now: f64) -> Result<()> {
    std::fs::create_dir_all(skins_dir)
        .map_err(|e| crate::error::Error::io(skins_dir.display().to_string(), e))?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(&skin.skin_png_base64)
        .map_err(|e| crate::error::Error::io("<skin png>", format!("decode: {e}")))?;
    std::fs::write(skins_dir.join(format!("{}.png", skin.uuid)), png)
        .map_err(|e| crate::error::Error::io("<skin png>", e))?;
    let meta = SkinMeta {
        texture_url: skin.texture_url.clone(),
        fetched_at: now,
    };
    let meta_json = serde_json::to_vec(&meta)
        .map_err(|e| crate::error::Error::io("<skin meta>", format!("serialise: {e}")))?;
    std::fs::write(skins_dir.join(format!("{}.json", skin.uuid)), meta_json)
        .map_err(|e| crate::error::Error::io("<skin meta>", e))?;
    Ok(())
}

/// Cache-first skin resolution. Reads a fresh cache entry unless `force`;
/// otherwise fetches, caches, and returns. Any error (network, parse, IO)
/// degrades to `None` — the avatar is cosmetic. Logs the degradation.
pub async fn get_account_skin(
    app: &tauri::AppHandle,
    uuid: &str,
    force: bool,
) -> Option<AccountSkin> {
    let dir = match crate::paths::skins_dir(app) {
        Ok(d) => d,
        Err(e) => {
            crate::diag!("account_skin: cannot resolve skins dir: {e}");
            return None;
        }
    };
    let now = crate::accounts::now_secs();
    if !force {
        if let Some(cached) = read_cached_skin(&dir, uuid, SKIN_CACHE_TTL_SECS, now) {
            return Some(cached);
        }
    }
    match fetch_skin(uuid).await {
        Ok(Some(skin)) => {
            if let Err(e) = write_cached_skin(&dir, &skin, now) {
                crate::diag!("account_skin: cache write failed for {uuid}: {e}");
            }
            Some(skin)
        }
        Ok(None) => None,
        Err(e) => {
            crate::diag!("account_skin: fetch failed for {uuid}: {e}");
            None
        }
    }
}

#[cfg(test)]
fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::test_env_lock()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn encode_textures(json: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    #[test]
    fn extracts_skin_url_from_textures_property() {
        let textures = r#"{"timestamp":1,"profileId":"abc","profileName":"P","textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/deadbeef"}}}"#;
        let props = vec![ProfileProperty {
            name: "textures".into(),
            value: encode_textures(textures),
        }];
        assert_eq!(
            skin_url_from_properties(&props).as_deref(),
            Some("https://textures.minecraft.net/texture/deadbeef")
        );
    }

    #[test]
    fn returns_none_when_no_skin_entry() {
        let textures =
            r#"{"textures":{"CAPE":{"url":"https://textures.minecraft.net/texture/cape"}}}"#;
        let props = vec![ProfileProperty {
            name: "textures".into(),
            value: encode_textures(textures),
        }];
        assert_eq!(skin_url_from_properties(&props), None);
    }

    #[test]
    fn returns_none_when_no_textures_property() {
        let props = vec![ProfileProperty {
            name: "something-else".into(),
            value: "x".into(),
        }];
        assert_eq!(skin_url_from_properties(&props), None);
    }

    #[test]
    fn returns_none_on_invalid_base64() {
        let props = vec![ProfileProperty {
            name: "textures".into(),
            value: "!!!not-base64!!!".into(),
        }];
        assert_eq!(skin_url_from_properties(&props), None);
    }

    #[tokio::test]
    async fn fetch_skin_happy_path_returns_png_base64() {
        let _g = test_env_lock();
        let server = MockServer::start().await;
        // sessionserver returns a profile whose textures point at the SAME
        // mock server's /texture/<hash> path (so the host check passes via
        // LUCERNA_EXTRA_ALLOWED_HOSTS and no real network is touched).
        let texture_url = format!("{}/texture/deadbeef", server.uri());
        let textures_json = format!(r#"{{"textures":{{"SKIN":{{"url":"{texture_url}"}}}}}}"#);
        let textures_b64 =
            base64::engine::general_purpose::STANDARD.encode(textures_json.as_bytes());
        let profile_body = format!(
            r#"{{"id":"abc","name":"P","properties":[{{"name":"textures","value":"{textures_b64}"}}]}}"#
        );
        Mock::given(method("GET"))
            .and(path_regex(r"^/session/minecraft/profile/.*$"))
            .respond_with(ResponseTemplate::new(200).set_body_string(profile_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/texture/deadbeef"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8, 2, 3, 4]))
            .mount(&server)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("LUCERNA_SESSIONSERVER_URL_OVERRIDE", server.uri());

        let skin = fetch_skin("7e8d9c0a-1234-5678-9abc-def012345678")
            .await
            .unwrap()
            .expect("expected a skin");

        std::env::remove_var("LUCERNA_SESSIONSERVER_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(skin.texture_url, texture_url);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(skin.skin_png_base64)
                .unwrap(),
            vec![1u8, 2, 3, 4]
        );
    }

    #[tokio::test]
    async fn fetch_skin_204_returns_none() {
        let _g = test_env_lock();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/session/minecraft/profile/.*$"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("LUCERNA_SESSIONSERVER_URL_OVERRIDE", server.uri());

        let skin = fetch_skin("00000000-0000-0000-0000-000000000000")
            .await
            .unwrap();

        std::env::remove_var("LUCERNA_SESSIONSERVER_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(skin, None);
    }

    #[test]
    fn cache_roundtrip_reads_back_fresh_entry() {
        let dir = tempdir().unwrap();
        let skin = AccountSkin {
            uuid: "u-1".into(),
            texture_url: "https://textures.minecraft.net/texture/x".into(),
            skin_png_base64: base64::engine::general_purpose::STANDARD.encode([9u8, 8, 7]),
        };
        write_cached_skin(dir.path(), &skin, 1000.0).unwrap();
        let read = read_cached_skin(dir.path(), "u-1", 6.0 * 3600.0, 1001.0);
        assert_eq!(read, Some(skin));
    }

    #[test]
    fn cache_miss_when_expired() {
        let dir = tempdir().unwrap();
        let skin = AccountSkin {
            uuid: "u-2".into(),
            texture_url: "https://textures.minecraft.net/texture/y".into(),
            skin_png_base64: base64::engine::general_purpose::STANDARD.encode([1u8]),
        };
        write_cached_skin(dir.path(), &skin, 0.0).unwrap();
        // now is well beyond ttl → expired → None.
        assert_eq!(read_cached_skin(dir.path(), "u-2", 100.0, 10_000.0), None);
    }

    #[test]
    fn cache_miss_when_absent() {
        let dir = tempdir().unwrap();
        assert_eq!(
            read_cached_skin(dir.path(), "nope", 6.0 * 3600.0, 1.0),
            None
        );
    }

    #[tokio::test]
    async fn fetch_skin_texture_404_returns_none() {
        let _g = test_env_lock();
        let server = MockServer::start().await;
        let texture_url = format!("{}/texture/missing", server.uri());
        let textures_json = format!(r#"{{"textures":{{"SKIN":{{"url":"{texture_url}"}}}}}}"#);
        let textures_b64 =
            base64::engine::general_purpose::STANDARD.encode(textures_json.as_bytes());
        let profile_body = format!(
            r#"{{"id":"abc","name":"P","properties":[{{"name":"textures","value":"{textures_b64}"}}]}}"#
        );
        Mock::given(method("GET"))
            .and(path_regex(r"^/session/minecraft/profile/.*$"))
            .respond_with(ResponseTemplate::new(200).set_body_string(profile_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/texture/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("LUCERNA_SESSIONSERVER_URL_OVERRIDE", server.uri());

        let skin = fetch_skin("7e8d9c0a-1234-5678-9abc-def012345678")
            .await
            .unwrap();

        std::env::remove_var("LUCERNA_SESSIONSERVER_URL_OVERRIDE");
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
        assert_eq!(skin, None);
    }
}
