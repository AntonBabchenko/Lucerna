//! EULA-гейт. `eula.txt` пишется только после явного согласия пользователя;
//! сборка/запуск сервера без согласия возвращают `ServerEulaNotAccepted`.

use crate::error::{Error, Result};
use std::path::Path;

/// Записать `eula.txt`. Вызывать ТОЛЬКО когда пользователь поставил галочку.
pub fn write_eula(path: &Path, accepted: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let body = format!(
        "#By changing the setting below to TRUE you agree to the Minecraft EULA (https://aka.ms/MinecraftEULA)\neula={accepted}\n"
    );
    std::fs::write(path, body).map_err(|e| Error::io(path.display().to_string(), e))
}

/// Гейт: вернуть ошибку, если EULA не принят.
pub fn require_accepted(accepted: bool) -> Result<()> {
    if accepted {
        Ok(())
    } else {
        Err(Error::ServerEulaNotAccepted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_accepted_writes_true() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("eula.txt");
        write_eula(&path, true).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("eula=true"));
    }

    #[test]
    fn require_accepted_errors_when_false() {
        assert!(matches!(
            require_accepted(false),
            Err(crate::error::Error::ServerEulaNotAccepted)
        ));
        assert!(require_accepted(true).is_ok());
    }
}
