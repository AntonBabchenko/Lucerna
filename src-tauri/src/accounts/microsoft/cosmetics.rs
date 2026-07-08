//! Microsoft Minecraft-Services cosmetic mutations: set/clear the active
//! cape and upload/reset the skin. All calls go through the `network::`
//! chokepoint with the account's MC access token; the base URL is overridable
//! for tests via `LUCERNA_MC_SERVICES_BASE_OVERRIDE`.

use crate::accounts::microsoft::mc_services::SkinVariant;
use crate::error::{Error, Result};
use crate::network::allowlist::check_url_allowed;
use crate::network::http;

const SERVICES_BASE_DEFAULT: &str = "https://api.minecraftservices.com";

fn services_base() -> String {
    crate::test_seam::resolve("LUCERNA_MC_SERVICES_BASE_OVERRIDE")
        .unwrap_or_else(|| SERVICES_BASE_DEFAULT.to_string())
}

/// PUT the active cape by id.
pub async fn set_active_cape(token: &str, cape_id: &str) -> Result<()> {
    let url = format!("{}/minecraft/profile/capes/active", services_base());
    check_url_allowed(&url, "microsoft_auth")?;
    let resp = http()
        .put(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "capeId": cape_id }))
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;
    ensure_success(resp, "set_active_cape").await
}

/// DELETE the active cape (hide it).
pub async fn clear_active_cape(token: &str) -> Result<()> {
    let url = format!("{}/minecraft/profile/capes/active", services_base());
    check_url_allowed(&url, "microsoft_auth")?;
    let resp = http()
        .delete(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;
    ensure_success(resp, "clear_active_cape").await
}

/// POST a new skin (multipart: `variant` + `file`).
pub async fn upload_skin(token: &str, png: &[u8], variant: SkinVariant) -> Result<()> {
    let (content_type, body) = build_skin_multipart(variant, png);
    let url = format!("{}/minecraft/profile/skins", services_base());
    check_url_allowed(&url, "microsoft_auth")?;
    let resp = http()
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;
    ensure_success(resp, "upload_skin").await
}

/// DELETE the active skin (reset to default).
pub async fn reset_skin(token: &str) -> Result<()> {
    let url = format!("{}/minecraft/profile/skins/active", services_base());
    check_url_allowed(&url, "microsoft_auth")?;
    let resp = http()
        .delete(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| Error::network(&url, e))?;
    ensure_success(resp, "reset_skin").await
}

/// Build a `multipart/form-data` body with the model variant and the PNG.
/// Pure — a fixed boundary keeps it deterministic and unit-testable. The
/// boundary literal cannot occur in a valid PNG stream, so collision is a
/// non-issue.
pub fn build_skin_multipart(variant: SkinVariant, png: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----LucernaSkinBoundary7MA4YWxkTrZu0gW";
    let variant_str = match variant {
        SkinVariant::Classic => "classic",
        SkinVariant::Slim => "slim",
    };
    let head = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{variant_str}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\n\
         Content-Type: image/png\r\n\r\n"
    );
    let tail = format!("\r\n--{boundary}--\r\n");
    let mut body = Vec::with_capacity(head.len() + png.len() + tail.len());
    body.extend_from_slice(head.as_bytes());
    body.extend_from_slice(png);
    body.extend_from_slice(tail.as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn ensure_success(resp: reqwest::Response, stage: &str) -> Result<()> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let text = resp.text().await.unwrap_or_default();
    Err(Error::AuthFailed {
        stage: stage.into(),
        details: format!("HTTP {status}: {text}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn multipart_has_variant_and_png_part() {
        let (ct, body) = build_skin_multipart(SkinVariant::Slim, &[0x89, 0x50, 0x4e, 0x47]);
        assert!(ct.starts_with("multipart/form-data; boundary="));
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("name=\"variant\""));
        assert!(s.contains("slim"));
        assert!(s.contains("name=\"file\"; filename=\"skin.png\""));
        assert!(s.contains("Content-Type: image/png"));
        assert!(body.windows(4).any(|w| w == [0x89, 0x50, 0x4e, 0x47]));
    }

    #[tokio::test]
    async fn set_active_cape_puts_cape_id() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/minecraft/profile/capes/active"))
            .and(header("Authorization", "Bearer tok"))
            .and(body_json(serde_json::json!({ "capeId": "c1" })))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;
        let _s = crate::test_seam::scope(&[
            ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
            ("LUCERNA_MC_SERVICES_BASE_OVERRIDE", &server.uri()),
        ]);
        set_active_cape("tok", "c1").await.unwrap();
    }

    #[tokio::test]
    async fn clear_active_cape_deletes() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/minecraft/profile/capes/active"))
            .and(header("Authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let _s = crate::test_seam::scope(&[
            ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
            ("LUCERNA_MC_SERVICES_BASE_OVERRIDE", &server.uri()),
        ]);
        clear_active_cape("tok").await.unwrap();
    }

    #[tokio::test]
    async fn reset_skin_deletes_active() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/minecraft/profile/skins/active"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let _s = crate::test_seam::scope(&[
            ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
            ("LUCERNA_MC_SERVICES_BASE_OVERRIDE", &server.uri()),
        ]);
        reset_skin("tok").await.unwrap();
    }

    #[tokio::test]
    async fn upload_skin_posts_multipart() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/minecraft/profile/skins"))
            .and(header("Authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;
        let _s = crate::test_seam::scope(&[
            ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
            ("LUCERNA_MC_SERVICES_BASE_OVERRIDE", &server.uri()),
        ]);
        upload_skin("tok", &[0x89, 0x50, 0x4e, 0x47], SkinVariant::Classic)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn non_success_maps_to_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/minecraft/profile/capes/active"))
            .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
            .mount(&server)
            .await;
        let _s = crate::test_seam::scope(&[
            ("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost"),
            ("LUCERNA_MC_SERVICES_BASE_OVERRIDE", &server.uri()),
        ]);
        let err = clear_active_cape("tok").await.unwrap_err();
        assert!(
            matches!(err, crate::error::Error::AuthFailed { .. }),
            "got {err:?}"
        );
    }
}
