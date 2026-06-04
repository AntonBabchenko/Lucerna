//! Reusable helpers that download a modpack version archive to the OS temp
//! dir and return the local path. Extracted from `modpack_fetch_to_temp` so
//! the `ModpackSource` adapters can delegate to them without duplicating the
//! HTTP + temp-write logic. These helpers are the canonical archive-staging
//! logic used by the Modrinth, CurseForge, and FTB adapters.

use tauri::Manager;

use crate::error::Error;

/// Download a Modrinth `.mrpack` version archive to a temp file and return
/// its path. Calls `{base}/v2/project/{project_id}/version/{version_id}`,
/// locates the primary `.mrpack` entry, downloads the bytes, and writes them
/// to `<temp>/lucerna/modpack/<uuid>.mrpack`.
pub(crate) async fn download_modrinth_mrpack(
    app: &tauri::AppHandle,
    base: &str,
    project_id: &str,
    version_id: &str,
) -> Result<String, Error> {
    let url = format!("{base}/v2/project/{project_id}/version/{version_id}");
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
        filename: String,
        primary: bool,
    }

    let v: V = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
        platform: "modrinth".into(),
        details: e.to_string(),
    })?;
    let f = v
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| v.files.iter().find(|f| f.filename.ends_with(".mrpack")))
        .ok_or(Error::ModpackManifestInvalid {
            format: "modrinth".into(),
            details: "no primary .mrpack file on version".into(),
        })?;
    let bytes = crate::network::get_bytes(&f.url, "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: f.url.clone(),
            details: e.to_string(),
        })?;

    write_to_temp(app, &bytes, "mrpack").await
}

/// Download a CurseForge modpack `.zip` to a temp file and return its path.
/// Calls `cf_api::resolve_file_download` to get the CDN URL, then downloads
/// the bytes and writes them to `<temp>/lucerna/modpack/<uuid>.zip`.
pub(crate) async fn download_curseforge_zip(
    app: &tauri::AppHandle,
    base: &str,
    key: Option<&str>,
    project_id: &str,
    version_id: &str,
) -> Result<String, Error> {
    let dl = crate::mods::modpack::cf_api::resolve_file_download(base, key, project_id, version_id)
        .await?;
    let bytes = crate::network::get_bytes(&dl, "modpacks")
        .await
        .map_err(|e| Error::ModsNetwork {
            url: dl.clone(),
            details: e.to_string(),
        })?;
    write_to_temp(app, &bytes, "zip").await
}

/// Write `bytes` to `<temp>/lucerna/modpack/<uuid>.<ext>` and return the
/// path string. Shared by both download helpers above and FTB staging.
pub(crate) async fn write_to_temp(
    app: &tauri::AppHandle,
    bytes: &[u8],
    ext: &str,
) -> Result<String, Error> {
    let temp_dir = app
        .path()
        .temp_dir()
        .map_err(|e| Error::Io {
            path: "<temp>".into(),
            details: e.to_string(),
        })?
        .join("lucerna")
        .join("modpack");
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| Error::Io {
            path: temp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    let dest = temp_dir.join(format!("{}.{ext}", uuid::Uuid::new_v4()));
    tokio::fs::write(&dest, bytes)
        .await
        .map_err(|e| Error::Io {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?;
    Ok(dest.to_string_lossy().to_string())
}
