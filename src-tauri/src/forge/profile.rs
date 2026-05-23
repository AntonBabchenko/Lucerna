//! Helpers for assembling `VersionDetails` returned by installer
//! handlers. Phase 1 legacy inlines the assembly in
//! `installer/legacy.rs`; Phase 2 transitional + Phase 3 modern go
//! through here.

use crate::versions::version_json::VersionDetails;

pub fn assemble_from_transitional(version_details: VersionDetails) -> VersionDetails {
    version_details
}

/// Modern-era VersionDetails assembly seam. Phase 3 keeps this as a
/// pass-through identical to `assemble_from_transitional`; future work
/// (NeoForge bootstrap shim, additional Forge bootstrap layers) can
/// extend this without touching `installer/modern.rs`.
pub fn assemble_from_modern(version_details: VersionDetails) -> VersionDetails {
    version_details
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::version_json::parse;

    #[test]
    fn assemble_is_idempotent() {
        let raw = r#"{
            "id": "1.16.5-forge-36.2.42",
            "inheritsFrom": "1.16.5",
            "mainClass": "cpw.mods.modlauncher.Launcher",
            "libraries": []
        }"#;
        let parsed = parse(raw).unwrap();
        let assembled = assemble_from_transitional(parsed.clone());
        assert_eq!(assembled.id, parsed.id);
    }

    #[test]
    fn assemble_from_modern_is_idempotent() {
        let raw = r#"{
            "id": "1.20.4-forge-49.0.49",
            "inheritsFrom": "1.20.4",
            "mainClass": "net.minecraftforge.bootstrap.ForgeBootstrap",
            "libraries": []
        }"#;
        let parsed = parse(raw).unwrap();
        let assembled = assemble_from_modern(parsed.clone());
        assert_eq!(assembled.id, parsed.id);
        assert_eq!(
            assembled.main_class,
            "net.minecraftforge.bootstrap.ForgeBootstrap"
        );
    }
}
