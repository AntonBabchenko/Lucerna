//! Где взять серверный артефакт по лоадеру. Чистое построение URL/координат;
//! фактическое скачивание/installServer — в `create.rs` + `process::`.
//!
//! Forge/NeoForge здесь нет: их installer добывает `forge::meta::
//! fetch_installer_bytes` (SHA-1-верификация по maven-sidecar + общий с
//! клиентом кеш) — см. `provision_loader` в `commands::servers_runtime`.

pub fn fabric_server_jar_url(mc: &str, loader: &str, installer: &str) -> String {
    format!("https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/{installer}/server/jar")
}

pub fn quilt_server_jar_url(mc: &str, loader: &str, installer: &str) -> String {
    format!("https://meta.quiltmc.org/v3/versions/loader/{mc}/{loader}/{installer}/server/jar")
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
}
