//! Shared GET+JSON helper for the Paper/Purpur core-build clients: one request
//! through the `network::` chokepoint, mapping every non-2xx (incl. 404) and
//! decode failure to a typed `ServerJarUnavailable` carrying the loader/MC
//! context. Both clients call it with their own loader label.

use crate::error::{Error, Result};

/// Identifies the launcher software with a contact URL, as PaperMC/PurpurMC ask.
const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";

/// GET `url` and decode the JSON body into `T`. `loader` (e.g. `"paper"`) and
/// `mc` are only used to enrich the `ServerJarUnavailable` error; a 404 becomes
/// a "version not available" error rather than a raw HTTP status.
pub(crate) async fn get_core_json<T: serde::de::DeserializeOwned>(
    url: &str,
    loader: &'static str,
    mc: &str,
) -> Result<T> {
    let resp = crate::network::request::get(url, &[("user-agent", UA)], "servers")
        .await
        .map_err(|e| Error::ServerJarUnavailable {
            loader: loader.into(),
            mc_version: mc.to_string(),
            reason: e.to_string(),
        })?;
    if resp.status == 404 {
        return Err(Error::ServerJarUnavailable {
            loader: loader.into(),
            mc_version: mc.to_string(),
            reason: "version not available".into(),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(Error::ServerJarUnavailable {
            loader: loader.into(),
            mc_version: mc.to_string(),
            reason: format!("HTTP {}", resp.status),
        });
    }
    serde_json::from_slice(&resp.body).map_err(|e| Error::ServerJarUnavailable {
        loader: loader.into(),
        mc_version: mc.to_string(),
        reason: format!("decode: {e}"),
    })
}
