//! Где взять серверный артефакт по лоадеру. Чистое построение URL/координат;
//! фактическое скачивание/installServer — в `create.rs` + `process::`.

pub fn fabric_server_jar_url(mc: &str, loader: &str, installer: &str) -> String {
    format!("https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/{installer}/server/jar")
}

pub fn quilt_server_jar_url(mc: &str, loader: &str, installer: &str) -> String {
    format!("https://meta.quiltmc.org/v3/versions/loader/{mc}/{loader}/{installer}/server/jar")
}

pub fn forge_installer_url(mc: &str, forge: &str) -> String {
    format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc}-{forge}/forge-{mc}-{forge}-installer.jar"
    )
}

pub fn neoforge_installer_url(version: &str) -> String {
    format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fabric_server_jar_url_built_from_versions() {
        let url = fabric_server_jar_url("1.20.4", "0.16.5", "1.0.1");
        assert_eq!(
            url,
            "https://meta.fabricmc.net/v2/versions/loader/1.20.4/0.16.5/1.0.1/server/jar"
        );
    }

    #[test]
    fn quilt_server_jar_url_built_from_versions() {
        let url = quilt_server_jar_url("1.20.4", "0.26.0", "0.9.2");
        assert_eq!(
            url,
            "https://meta.quiltmc.org/v3/versions/loader/1.20.4/0.26.0/0.9.2/server/jar"
        );
    }

    #[test]
    fn forge_installer_url_built() {
        let url = forge_installer_url("1.20.4", "49.0.30");
        assert_eq!(
            url,
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.4-49.0.30/forge-1.20.4-49.0.30-installer.jar"
        );
    }

    #[test]
    fn neoforge_installer_url_built() {
        let url = neoforge_installer_url("20.4.237");
        assert_eq!(
            url,
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-installer.jar"
        );
    }
}
