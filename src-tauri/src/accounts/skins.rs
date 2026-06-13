//! Account profile skin: fetch the player's public skin head by UUID
//! through the `network::` chokepoint, cache the PNG on disk, and expose
//! it to the UI. Skins are public (no token required); the avatar is
//! cosmetic, so every failure degrades silently to `None`.

// These imports are used by fetch_skin and disk-cache functions added in later tasks.
#[allow(unused_imports)]
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
// Used by fetch_skin in a later task.
#[allow(dead_code)]
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
// used by fetch_skin in a later task
#[allow(dead_code)]
fn skin_url_from_properties(props: &[ProfileProperty]) -> Option<String> {
    let textures_b64 = props.iter().find(|p| p.name == "textures")?.value.as_str();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(textures_b64)
        .ok()?;
    let payload: TexturesPayload = serde_json::from_slice(&decoded).ok()?;
    payload.textures.skin.map(|s| s.url)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
