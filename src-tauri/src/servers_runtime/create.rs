//! Оркестрация сборки сервера: скачать артефакт, записать конфиги, скопировать
//! моды. Сетевые шаги (добавляются в следующих задачах) принимают `&AppHandle`;
//! чистые шаги (копирование) — нет.

use crate::error::{Error, Result};
use crate::servers_runtime::schema::ServerFile;
use std::path::Path;

/// Собрать vanilla-сервер по уже разрешённым `jar_url`/`sha1`.
///
/// Последовательность:
/// 1. Проверить согласие с EULA (иначе `ServerEulaNotAccepted`).
/// 2. Создать директорию `<base>/servers/<id>/runtime/`.
/// 3. Записать `server.json`.
/// 4. Скачать `server.jar` в `runtime/server.jar`, проверив SHA-1.
/// 5. Записать `eula.txt`.
///
/// `base` = корень app-data (например, `%APPDATA%/com.lucerna.app`).
pub async fn create_vanilla_server(
    base: &Path,
    file: &ServerFile,
    jar_url: &str,
    sha1: &str,
) -> Result<()> {
    crate::servers_runtime::eula::require_accepted(file.eula_accepted)?;
    let p = crate::paths::server_paths(base, &file.id);
    std::fs::create_dir_all(&p.runtime)
        .map_err(|e| Error::io(p.runtime.display().to_string(), e))?;
    crate::servers_runtime::store::write_server_json(&p.json, file)?;
    crate::network::download::download_no_emit(
        jar_url,
        &p.runtime.join("server.jar"),
        sha1,
        "servers",
    )
    .await?;
    crate::servers_runtime::eula::write_eula(&p.runtime.join("eula.txt"), file.eula_accepted)?;
    Ok(())
}

/// Найти серверный download-URL и SHA-1 для vanilla-версии через манифест Mojang.
///
/// Возвращает `(url, sha1)` из `downloads.server` в per-version JSON.
/// Ошибка `ServerJarUnavailable` если версия отсутствует в манифесте или у неё
/// нет серверного артефакта (очень старые версии).
pub async fn resolve_vanilla_jar(mc_version: &str) -> Result<(String, String)> {
    let manifest = crate::versions::manifest::list_manifest().await?;
    let entry = manifest
        .iter()
        .find(|e| e.id == mc_version)
        .ok_or_else(|| Error::ServerJarUnavailable {
            loader: "vanilla".into(),
            mc_version: mc_version.to_string(),
            reason: "version not in manifest".into(),
        })?;
    let json_text = crate::network::get_text(&entry.url, "servers").await?;
    let details = crate::versions::version_json::parse(&json_text)
        .map_err(|e| Error::io("<version_json>", format!("parse: {e}")))?;
    let server =
        details
            .downloads
            .and_then(|d| d.server)
            .ok_or_else(|| Error::ServerJarUnavailable {
                loader: "vanilla".into(),
                mc_version: mc_version.to_string(),
                reason: "no server download in version JSON".into(),
            })?;
    Ok((server.url, server.sha1))
}

/// Скопировать модовые файлы (`.jar`, `.jar.disabled`) из `src` в `dest`.
/// Отсутствующий `src` — не ошибка (0 скопировано). Возвращает число файлов.
pub fn copy_instance_mods(src: &Path, dest: &Path) -> Result<usize> {
    let entries = match std::fs::read_dir(src) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(Error::io(src.display().to_string(), e)),
    };
    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest.display().to_string(), e))?;
    let mut copied = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".jar") || name.ends_with(".jar.disabled") {
            let to = dest.join(entry.file_name());
            std::fs::copy(&path, &to).map_err(|e| Error::io(to.display().to_string(), e))?;
            copied += 1;
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn copies_only_jars_into_dest_mods() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("inst/mods");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.jar"), b"a").unwrap();
        std::fs::write(src.join("b.jar.disabled"), b"b").unwrap();
        std::fs::write(src.join("notes.txt"), b"x").unwrap();

        let dest = dir.path().join("srv/runtime/mods");
        let n = copy_instance_mods(&src, &dest).unwrap();
        assert_eq!(n, 2, "копируем .jar и .jar.disabled, пропускаем .txt");
        assert!(dest.join("a.jar").exists());
        assert!(dest.join("b.jar.disabled").exists());
        assert!(!dest.join("notes.txt").exists());
    }

    #[test]
    fn missing_source_is_ok_zero_copied() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("srv/runtime/mods");
        let n = copy_instance_mods(&dir.path().join("nope"), &dest).unwrap();
        assert_eq!(n, 0);
    }
}
