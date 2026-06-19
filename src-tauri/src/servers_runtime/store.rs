//! Атомарное read/write `server.json` + перечисление/удаление. Зеркалит
//! `instances::store` (tmp+rename), малформ при листинге → skip-with-warning.

use crate::error::{Error, Result};
use crate::servers_runtime::schema::ServerFile;
use std::path::Path;

pub fn read_server_json(path: &Path) -> Result<ServerFile> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))
}

pub fn write_server_json(path: &Path, value: &ServerFile) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::io(path.display().to_string(), "no parent dir"))?;
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialize: {e}")))?;
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Все валидные серверы под `<base>/servers/*/server.json`, отсортированы по id.
/// Малформ/нечитаемые пропускаются с предупреждением в stderr.
pub fn list_all(base: &Path) -> Result<Vec<ServerFile>> {
    let servers = base.join("servers");
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&servers) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(Error::io(servers.display().to_string(), e)),
    };
    for entry in entries.flatten() {
        let json = entry.path().join("server.json");
        match read_server_json(&json) {
            Ok(s) => out.push(s),
            Err(e) => eprintln!("servers: skipping {}: {e}", json.display()),
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Рекурсивно удалить `<base>/servers/<id>`. Идемпотентно (нет папки → Ok).
pub fn delete_server(base: &Path, id: &str) -> Result<()> {
    let root = base.join("servers").join(id);
    match std::fs::remove_dir_all(&root) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(root.display().to_string(), e)),
    }
}

/// Переименовать сервер: RMW `server.json`. Имя триммится; пустое после
/// трима — ошибка (фронт это гейтит, бэкенд защищается на границе).
/// Возвращает обновлённый `ServerFile`.
pub fn rename_server(base: &Path, id: &str, name: &str) -> Result<ServerFile> {
    let json = crate::paths::server_paths(base, id).json;
    let mut file = read_server_json(&json)?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::io("<server name>", "name cannot be empty"));
    }
    file.name = trimmed.to_string();
    write_server_json(&json, &file)?;
    Ok(file)
}

/// Обновить рантайм-конфиг сервера (heap + extra JVM args): RMW `server.json`.
/// Применяется при следующем старте. Возвращает обновлённый `ServerFile`.
pub fn update_runtime_config(
    base: &Path,
    id: &str,
    max_heap_mb: u32,
    extra_jvm_args: &str,
) -> Result<ServerFile> {
    let json = crate::paths::server_paths(base, id).json;
    let mut file = read_server_json(&json)?;
    file.max_heap_mb = max_heap_mb;
    file.extra_jvm_args = extra_jvm_args.to_string();
    write_server_json(&json, &file)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;
    use tempfile::tempdir;

    fn sample(id: &str) -> ServerFile {
        ServerFile {
            id: id.into(),
            name: "S".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 1.0,
            eula_accepted: false,
            created_from_instance: None,
            handled_log_sig: None,
            upload: None,
        }
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempdir().unwrap();
        let p = crate::paths::server_paths(dir.path(), "srv-1");
        write_server_json(&p.json, &sample("srv-1")).unwrap();
        assert_eq!(read_server_json(&p.json).unwrap(), sample("srv-1"));
    }

    #[test]
    fn list_all_skips_malformed_and_returns_sorted() {
        let dir = tempdir().unwrap();
        for id in ["srv-b", "srv-a"] {
            let p = crate::paths::server_paths(dir.path(), id);
            write_server_json(&p.json, &sample(id)).unwrap();
        }
        let bad = dir.path().join("servers/srv-bad/server.json");
        std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
        std::fs::write(&bad, "not json").unwrap();

        let all = list_all(dir.path()).unwrap();
        let ids: Vec<_> = all.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["srv-a", "srv-b"]);
    }

    #[test]
    fn delete_removes_root_dir() {
        let dir = tempdir().unwrap();
        let p = crate::paths::server_paths(dir.path(), "srv-1");
        write_server_json(&p.json, &sample("srv-1")).unwrap();
        std::fs::create_dir_all(&p.runtime).unwrap();
        delete_server(dir.path(), "srv-1").unwrap();
        assert!(!p.root.exists());
    }

    #[test]
    fn rename_server_changes_name_and_persists() {
        let dir = tempdir().unwrap();
        let p = crate::paths::server_paths(dir.path(), "srv-1");
        write_server_json(&p.json, &sample("srv-1")).unwrap();
        let updated = rename_server(dir.path(), "srv-1", "  My Server  ").unwrap();
        assert_eq!(updated.name, "My Server");
        assert_eq!(read_server_json(&p.json).unwrap().name, "My Server");
    }

    #[test]
    fn rename_server_rejects_empty_after_trim() {
        let dir = tempdir().unwrap();
        let p = crate::paths::server_paths(dir.path(), "srv-1");
        write_server_json(&p.json, &sample("srv-1")).unwrap();
        let r = rename_server(dir.path(), "srv-1", "   ");
        assert!(r.is_err(), "empty name must be rejected");
        assert_eq!(read_server_json(&p.json).unwrap().name, "S");
    }

    #[test]
    fn update_runtime_config_sets_heap_and_args() {
        let dir = tempdir().unwrap();
        let p = crate::paths::server_paths(dir.path(), "srv-1");
        write_server_json(&p.json, &sample("srv-1")).unwrap();
        let updated = update_runtime_config(dir.path(), "srv-1", 6144, "-XX:+UseG1GC").unwrap();
        assert_eq!(updated.max_heap_mb, 6144);
        assert_eq!(updated.extra_jvm_args, "-XX:+UseG1GC");
        let back = read_server_json(&p.json).unwrap();
        assert_eq!(back.max_heap_mb, 6144);
        assert_eq!(back.extra_jvm_args, "-XX:+UseG1GC");
    }
}
