//! Оркестрация сборки сервера: скачать артефакт, записать конфиги, скопировать
//! моды. Сетевые шаги (добавляются в следующих задачах) принимают `&AppHandle`;
//! чистые шаги (копирование) — нет.

use crate::error::{Error, Result};
use std::path::Path;

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
