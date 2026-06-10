use super::*;

// =========================================================================
// CurseForge key management + shared cache management (v0.5.0 sub-feature 3)
// =========================================================================

/// Report whether a CurseForge API key is currently stored in the OS
/// keyring. `Invalid` is reserved for future "key was rejected" surfacing —
/// today this command only distinguishes Missing vs Set.
#[tauri::command]
#[specta::specta]
pub async fn mods_get_curseforge_key_status() -> crate::error::Result<KeyStatus> {
    Ok(match cf_keyring::get()? {
        Some(_) => KeyStatus::Set,
        None => KeyStatus::Missing,
    })
}

/// Validate a candidate CurseForge API key by pinging `/v1/games/432`
/// (the Minecraft game id) with `x-api-key`. On a non-success HTTP
/// response we return `ModsPlatformAuth { kind: Invalid }` and do NOT
/// persist anything. Only a successful ping causes the key to be
/// written to the OS keyring.
///
/// After a successful key set, this command also iterates every
/// instance and resets `enrich_attempted = false` on each instance's
/// `source = None` mods, so any mods that were Modrinth-only-attempted
/// under a keyless install are retried (now with CF) on the next
/// Installed-tab open. Reset failures are logged and swallowed — a
/// single instance's registry write failure must not fail the key set.
#[tauri::command]
#[specta::specta]
pub async fn mods_set_curseforge_key(
    app: tauri::AppHandle,
    key: String,
) -> crate::error::Result<()> {
    let url = "https://api.curseforge.com/v1/games/432";
    let resp = crate::network::request::get(url, &[("x-api-key", key.as_str())], "mods")
        .await
        .map_err(|e| crate::error::Error::ModsNetwork {
            url: url.into(),
            details: e.to_string(),
        })?;
    if !(200..300).contains(&resp.status) {
        return Err(crate::error::Error::ModsPlatformAuth {
            kind: crate::error::ModsAuthKind::Invalid,
        });
    }
    cf_keyring::set(&key)?;

    // Self-heal: any instance whose source=None mods were attempted
    // under a keyless or CF-down pass becomes eligible for backfill
    // again on the next Installed-tab open. Best-effort.
    let instances = match crate::instances::list_instances_with_status(&app) {
        Ok(xs) => xs,
        Err(e) => {
            eprintln!("[mods_set_curseforge_key] could not list instances for reset: {e}");
            return Ok(());
        }
    };
    for inst in instances {
        let root = match crate::paths::instance_dir(&app, &inst.id) {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "[mods_set_curseforge_key] no instance_dir for {}: {e}",
                    inst.id
                );
                continue;
            }
        };
        if let Err(e) =
            crate::mods::installed::reset_enrichment_attempts_for_unresolved(&root).await
        {
            eprintln!(
                "[mods_set_curseforge_key] reset failed for {}: {e}",
                inst.id
            );
        }
    }
    Ok(())
}

/// Remove the stored CurseForge API key. No-op if no key is set.
#[tauri::command]
#[specta::specta]
pub async fn mods_clear_curseforge_key() -> crate::error::Result<()> {
    cf_keyring::clear()
}

/// Size in bytes of the shared mod cache directory (under the launcher's
/// app-data dir). Used by the Settings panel to show "Cache: X MB".
#[tauri::command]
#[specta::specta]
pub async fn mods_cache_size_bytes(app: tauri::AppHandle) -> crate::error::Result<f64> {
    // f64 not u64: specta forbids exporting BigInt-style types to TS.
    // 2^53 bytes (~9 PiB) is far beyond any plausible mod cache size.
    let dd = data_dir(&app)?;
    let n = crate::mods::cache::size_bytes(&dd).await?;
    Ok(n as f64)
}

/// Delete every cached mod jar. Returns the number of bytes reclaimed.
/// Installed instance copies are untouched — only the shared cache.
#[tauri::command]
#[specta::specta]
pub async fn mods_clear_cache(app: tauri::AppHandle) -> crate::error::Result<f64> {
    // f64 not u64 — same reason as mods_cache_size_bytes.
    let dd = data_dir(&app)?;
    let n = crate::mods::cache::clear(&dd).await?;
    Ok(n as f64)
}
