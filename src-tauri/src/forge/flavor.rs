//! `ForgeFlavor` distinguishes Forge from NeoForge. Both share the
//! installer pipeline (same JSON shape, same processors), only the
//! maven and promotions URLs differ.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForgeFlavor {
    Forge,
    /// Wired in v0.4.1. Defined now so the API surface is stable.
    NeoForge,
}

impl ForgeFlavor {
    /// Display name for logs and UI error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            ForgeFlavor::Forge => "forge",
            ForgeFlavor::NeoForge => "neoforge",
        }
    }

    /// URL of the maven-metadata.xml listing all released versions.
    pub fn maven_metadata_url(self) -> &'static str {
        match self {
            ForgeFlavor::Forge => {
                "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml"
            }
            ForgeFlavor::NeoForge => {
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml"
            }
        }
    }

    /// promotions_slim.json URL. `None` for NeoForge (no analog exists).
    pub fn promotions_url(self) -> Option<&'static str> {
        match self {
            ForgeFlavor::Forge => Some(
                "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
            ),
            ForgeFlavor::NeoForge => None,
        }
    }

    /// Installer JAR URL for a specific `(mc, fv)` combination.
    ///
    /// For Forge, the maven path segment is normally `<mc>-<fv>`, but
    /// some MC ranges (1.7.10, parts of 1.9) use `<mc>-<fv>-<mc>`. The
    /// optional `raw_maven_version` argument lets the meta layer
    /// supply the exact string it saw in `maven-metadata.xml`; when
    /// `None` we fall back to the canonical `<mc>-<fv>` form. NeoForge
    /// ignores both `mc` and `raw_maven_version` (its layout is
    /// `<fv>` everywhere).
    pub fn installer_url(self, mc: &str, fv: &str, raw_maven_version: Option<&str>) -> String {
        match self {
            ForgeFlavor::Forge => {
                let raw = raw_maven_version
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{mc}-{fv}"));
                format!(
                    "https://maven.minecraftforge.net/net/minecraftforge/forge/{raw}/forge-{raw}-installer.jar"
                )
            }
            ForgeFlavor::NeoForge => format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{fv}/neoforge-{fv}-installer.jar"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_str_is_lowercase() {
        assert_eq!(ForgeFlavor::Forge.as_str(), "forge");
        assert_eq!(ForgeFlavor::NeoForge.as_str(), "neoforge");
    }

    #[test]
    fn forge_maven_metadata_points_at_minecraftforge() {
        let url = ForgeFlavor::Forge.maven_metadata_url();
        assert!(url.starts_with("https://maven.minecraftforge.net/"));
        assert!(url.ends_with("/maven-metadata.xml"));
    }

    #[test]
    fn neoforge_maven_metadata_points_at_neoforged() {
        let url = ForgeFlavor::NeoForge.maven_metadata_url();
        assert!(url.starts_with("https://maven.neoforged.net/"));
        assert!(url.ends_with("/maven-metadata.xml"));
    }

    #[test]
    fn forge_promotions_url_is_some() {
        assert!(ForgeFlavor::Forge.promotions_url().is_some());
    }

    #[test]
    fn neoforge_promotions_url_is_none() {
        assert!(ForgeFlavor::NeoForge.promotions_url().is_none());
    }

    #[test]
    fn forge_installer_url_has_mc_and_fv() {
        let url = ForgeFlavor::Forge.installer_url("1.20.4", "49.0.49", None);
        assert!(url.contains("1.20.4-49.0.49"));
        assert!(url.ends_with("/forge-1.20.4-49.0.49-installer.jar"));
    }

    #[test]
    fn forge_installer_url_honours_raw_maven_version_for_legacy_quirk() {
        // 1.7.10 + Forge 10.13.4.1614 lives at the duplicate-MC-suffix path.
        let url = ForgeFlavor::Forge.installer_url(
            "1.7.10",
            "10.13.4.1614",
            Some("1.7.10-10.13.4.1614-1.7.10"),
        );
        assert!(
            url.ends_with(
                "/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10-installer.jar"
            ),
            "got: {url}"
        );
    }

    #[test]
    fn neoforge_installer_url_uses_neoforged_layout() {
        let url = ForgeFlavor::NeoForge.installer_url("1.20.4", "20.4.234", None);
        // NeoForge: version-only in maven path (no `<mc>-` prefix), unlike Forge.
        assert!(url.contains("/neoforge/20.4.234/"));
        assert!(url.ends_with("/neoforge-20.4.234-installer.jar"));
    }
}
