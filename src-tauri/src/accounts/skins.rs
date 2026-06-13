//! Account profile skin: fetch the player's public skin head by UUID
//! through the `network::` chokepoint, cache the PNG on disk, and expose
//! it to the UI. Skins are public (no token required); the avatar is
//! cosmetic, so every failure degrades silently to `None`.

use crate::error::Result;
use base64::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
#[allow(unused_imports)]
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

/// Fetch the player's skin by UUID. Returns `Ok(None)` when the profile
/// has no skin (default skin, offline UUID → 204, or any non-2xx). `Err`
/// only on a transport failure reaching the texture host; the orchestrator
/// degrades that to `None` too.
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
    let png = crate::network::get_bytes(&texture_url, "account_skin").await?;
    let skin_png_base64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(Some(AccountSkin {
        uuid: uuid.to_string(),
        texture_url,
        skin_png_base64,
    }))
}

#[cfg(test)]
fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::test_env_lock()
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
