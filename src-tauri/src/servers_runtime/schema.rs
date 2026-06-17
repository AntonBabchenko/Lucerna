//! Дисковая (`ServerFile`) и IPC (`ServerWithStatus`) формы сервера.

use crate::instances::schema::LoaderKind;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerFile {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub max_heap_mb: u32,
    pub extra_jvm_args: String,
    /// f64 — ограничение specta-typescript (нет u64); в пределах 2^53.
    pub created_unix_ms: f64,
    /// EULA принят явным действием в визарде. Без него старт (План 2) запрещён.
    pub eula_accepted: bool,
    /// Инстанс-источник для модели «из инстанса». `None` для standalone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_from_instance: Option<String>,
}

/// Что видит UI: `ServerFile` + рантайм-статус (заполняется в Плане 2).
#[derive(Debug, Clone, Serialize, Type)]
pub struct ServerWithStatus {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub max_heap_mb: u32,
    pub extra_jvm_args: String,
    pub created_unix_ms: f64,
    pub eula_accepted: bool,
    pub created_from_instance: Option<String>,
    pub running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
}

impl ServerWithStatus {
    pub fn from_file(
        file: &ServerFile,
        running: bool,
        pid: Option<u32>,
        port: Option<u16>,
    ) -> Self {
        Self {
            id: file.id.clone(),
            name: file.name.clone(),
            mc_version: file.mc_version.clone(),
            loader: file.loader,
            loader_version: file.loader_version.clone(),
            max_heap_mb: file.max_heap_mb,
            extra_jvm_args: file.extra_jvm_args.clone(),
            created_unix_ms: file.created_unix_ms,
            eula_accepted: file.eula_accepted,
            created_from_instance: file.created_from_instance.clone(),
            running,
            pid,
            port,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::LoaderKind;

    fn sample() -> ServerFile {
        ServerFile {
            id: "srv-aaaa".into(),
            name: "Сервер для друзей".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.5".into()),
            max_heap_mb: 4096,
            extra_jvm_args: String::new(),
            created_unix_ms: 1_700_000_000_000.0,
            eula_accepted: false,
            created_from_instance: Some("inst-1".into()),
        }
    }

    #[test]
    fn server_file_roundtrip() {
        let s = sample();
        let json = serde_json::to_string(&s).unwrap();
        let back: ServerFile = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn standalone_skips_created_from_instance() {
        let mut s = sample();
        s.created_from_instance = None;
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("created_from_instance"), "got: {json}");
    }

    #[test]
    fn with_status_from_file_carries_runtime_fields() {
        let s = sample();
        let w = ServerWithStatus::from_file(&s, true, Some(4321), Some(25565));
        assert_eq!(w.id, s.id);
        assert!(w.running);
        assert_eq!(w.pid, Some(4321));
        assert_eq!(w.port, Some(25565));
    }
}
