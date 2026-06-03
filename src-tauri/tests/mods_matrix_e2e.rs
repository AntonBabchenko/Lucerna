#![allow(dead_code)]
//! Mod-install matrix e2e: for each (loader x popular MC) combo, install N
//! randomly-sampled COMPATIBLE mods from live Modrinth and assert the install
//! + dependency pipeline succeeds (right version chosen, deps resolved, jars on
//! disk, no errors). The install analogue of loader_matrix_e2e.rs.
//!
//! Gated behind #[ignore] — real network + downloads. Run:
//!   cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test mods_matrix_e2e -- --ignored --nocapture
//! Subset overrides for a fast single-combo run:
//!   LUCERNA_MOD_MATRIX_MC=1.21.1 LUCERNA_MOD_MATRIX_LOADER=fabric \
//!   LUCERNA_MOD_MATRIX_N=3 cargo test --test mods_matrix_e2e -- --ignored --nocapture

use lucerna_lib::instances::schema::LoaderKind;

const MC_VERSIONS_DEFAULT: &[&str] = &[
    "1.16.5", "1.18.2", "1.20.1", "1.20.4", "1.21.1", "1.21.8",
];

fn mc_versions() -> Vec<String> {
    match std::env::var("LUCERNA_MOD_MATRIX_MC") {
        Ok(s) if !s.trim().is_empty() => s.split(',').map(|s| s.trim().to_string()).collect(),
        _ => MC_VERSIONS_DEFAULT.iter().map(|s| s.to_string()).collect(),
    }
}

/// NeoForge applies to MC >= 1.20.1 (fork point). Mirrors loader_matrix_e2e.
fn neoforge_applies(mc: &str) -> bool {
    let p: Vec<u32> = mc.split('.').filter_map(|s| s.parse().ok()).collect();
    let (maj, min, pat) = (p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0), p.get(2).copied().unwrap_or(0));
    if maj != 1 { return maj > 1; }
    if min != 20 { return min > 20; }
    pat >= 1
}

/// Mod loaders for a combo (Vanilla excluded — vanilla can't take mods).
/// Fabric/Quilt from 1.16; Forge from 1.6; NeoForge per neoforge_applies.
fn loaders_for(mc: &str) -> Vec<LoaderKind> {
    let p: Vec<u32> = mc.split('.').filter_map(|s| s.parse().ok()).collect();
    let mm = (p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0));
    let mut out = Vec::new();
    if mm >= (1, 16) { out.push(LoaderKind::Fabric); out.push(LoaderKind::Quilt); }
    if mm >= (1, 6) { out.push(LoaderKind::Forge); }
    if neoforge_applies(mc) { out.push(LoaderKind::NeoForge); }
    out
}

/// Apply the LUCERNA_MOD_MATRIX_LOADER subset filter (csv of loader names).
fn loader_enabled(l: LoaderKind) -> bool {
    let want = match std::env::var("LUCERNA_MOD_MATRIX_LOADER") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return true,
    };
    let name = match l {
        LoaderKind::Fabric => "fabric", LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge", LoaderKind::NeoForge => "neoforge",
        LoaderKind::Vanilla => "vanilla",
    };
    want.split(',').any(|w| w.trim().eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod config_tests {
    use super::*;
    #[test]
    fn neoforge_gated_at_1_20_1() {
        assert!(!neoforge_applies("1.20.0"));
        assert!(neoforge_applies("1.20.1"));
        assert!(neoforge_applies("1.21.8"));
        assert!(!neoforge_applies("1.16.5"));
    }
    #[test]
    fn loaders_for_modern_includes_all_four() {
        let ls = loaders_for("1.21.1");
        assert!(ls.contains(&LoaderKind::Fabric));
        assert!(ls.contains(&LoaderKind::Quilt));
        assert!(ls.contains(&LoaderKind::Forge));
        assert!(ls.contains(&LoaderKind::NeoForge));
        assert!(!ls.contains(&LoaderKind::Vanilla));
    }
    #[test]
    fn loaders_for_legacy_excludes_fabric_and_neoforge() {
        let ls = loaders_for("1.12.2");
        assert!(ls.contains(&LoaderKind::Forge));
        assert!(!ls.contains(&LoaderKind::Fabric));
        assert!(!ls.contains(&LoaderKind::NeoForge));
    }
}
