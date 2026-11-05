//! Saying what a refused file is (spec D3, §4.2 step 7): one CurseForge
//! fingerprint request for every refused `.jar` / `.zip` of a call. A file
//! over the cap, or a lookup that could not run, is refused without a name —
//! and the row says CurseForge was not asked.

use std::path::Path;

use crate::mods::curseforge::CurseForgeClient;
use crate::mods::manual_downloads::state::is_actionable;
use crate::mods::manual_downloads::types::SupplyRow;
use crate::mods::modpack::schema::MissingModStatus;

/// Mod jars are a few MiB; past this, fingerprinting (which reads the whole
/// file) is not worth the memory for a nicer refusal.
pub const FINGERPRINT_CAP: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Unmatched {
    pub path: String,
    pub len: u64,
}

fn not_asked(u: &Unmatched) -> SupplyRow {
    SupplyRow::NotInPack {
        path: u.path.clone(),
        found: None,
        asked: false,
    }
}

async fn fingerprint(path: &Path) -> std::io::Result<u32> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        std::fs::read(&path).map(|b| crate::mods::enrich::curseforge_fingerprint(&b))
    })
    .await
    .map_err(|j| std::io::Error::other(j.to_string()))?
}

pub async fn name_refusals(
    client: Option<&CurseForgeClient>,
    unmatched: &[Unmatched],
    statuses: &[MissingModStatus],
) -> Vec<SupplyRow> {
    let mut rows: Vec<Option<SupplyRow>> = vec![None; unmatched.len()];
    let mut asks: Vec<(usize, u32)> = Vec::new();
    for (i, u) in unmatched.iter().enumerate() {
        if client.is_none() || u.len > FINGERPRINT_CAP {
            rows[i] = Some(not_asked(u));
            continue;
        }
        match fingerprint(Path::new(&u.path)).await {
            Ok(fp) => asks.push((i, fp)),
            Err(e) => {
                crate::diag!("[manual-downloads] could not fingerprint {}: {e}", u.path);
                rows[i] = Some(not_asked(u));
            }
        }
    }
    let hits = match client {
        Some(c) if !asks.is_empty() => {
            let fps: Vec<u32> = asks.iter().map(|(_, fp)| *fp).collect();
            match c.files_by_murmur(&fps).await {
                Ok(map) => Some(map),
                Err(e) => {
                    crate::diag!("[manual-downloads] fingerprint lookup failed: {e}");
                    None
                }
            }
        }
        _ => None,
    };
    for (i, fp) in asks {
        let u = &unmatched[i];
        rows[i] = Some(match hits.as_ref() {
            None => not_asked(u),
            Some(map) => match map.get(&fp) {
                None => SupplyRow::NotInPack {
                    path: u.path.clone(),
                    found: None,
                    asked: true,
                },
                Some(hit) => {
                    let found = hit
                        .version_number
                        .clone()
                        .unwrap_or_else(|| hit.version_id.clone());
                    match statuses.iter().find(|s| {
                        is_actionable(&s.entry)
                            && s.entry.project_id.as_deref() == Some(hit.project_id.as_str())
                    }) {
                        Some(s) => SupplyRow::OtherVersion {
                            path: u.path.clone(),
                            found,
                            wanted: s.entry.filename.clone(),
                        },
                        None => SupplyRow::NotInPack {
                            path: u.path.clone(),
                            found: Some(found),
                            asked: true,
                        },
                    }
                }
            },
        });
    }
    // Every slot was filled above: each index is either refused unasked or asked.
    rows.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn without_a_client_a_refusal_says_curseforge_was_not_asked() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jar");
        std::fs::write(&p, b"x").unwrap();
        let rows = name_refusals(
            None,
            &[Unmatched {
                path: p.display().to_string(),
                len: 1,
            }],
            &[],
        )
        .await;
        assert_eq!(
            rows,
            vec![SupplyRow::NotInPack {
                path: p.display().to_string(),
                found: None,
                asked: false
            }]
        );
    }
}
