//! Дисковая (`ServerFile`) и IPC (`ServerWithStatus`) формы сервера.

use crate::instances::schema::LoaderKind;
use serde::{Deserialize, Serialize};
use specta::Type;

/// SFTP upload target configuration. The password is stored in the OS keyring,
/// never in this struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UploadConfig {
    pub host: String,
    #[serde(default = "default_sftp_port")]
    pub port: u16,
    pub user: String,
    pub remote_path: String,
    /// SHA-256 host-key fingerprint accepted on first connect (TOFU).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_host_fp: Option<String>,
}

fn default_sftp_port() -> u16 {
    22
}

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
    /// Log signature of the latest server crash the user already acted on
    /// (e.g. removed client mods). Suppresses re-nagging on that same log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handled_log_sig: Option<String>,
    /// SFTP upload target. Password lives in the OS keyring, never here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<UploadConfig>,
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
    pub upload: Option<UploadConfig>,
    /// Whether a keyring password is stored for the upload target.
    pub upload_password_set: bool,
}

impl ServerWithStatus {
    pub fn from_file(
        file: &ServerFile,
        running: bool,
        pid: Option<u32>,
        port: Option<u16>,
        upload_password_set: bool,
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
            upload: file.upload.clone(),
            upload_password_set,
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
            handled_log_sig: None,
            upload: None,
        }
    }

    #[test]
    fn server_file_skips_none_handled_sig() {
        let s = ServerFile {
            id: "x".into(),
            name: "n".into(),
            mc_version: "1.20.1".into(),
            loader: crate::instances::schema::LoaderKind::Forge,
            loader_version: Some("47.4.10".into()),
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 1.0,
            eula_accepted: true,
            created_from_instance: None,
            handled_log_sig: None,
            upload: None,
        };
        assert!(!serde_json::to_string(&s)
            .unwrap()
            .contains("handled_log_sig"));
    }
    #[test]
    fn server_file_handled_sig_roundtrip() {
        let s = ServerFile {
            id: "x".into(),
            name: "n".into(),
            mc_version: "1.20.1".into(),
            loader: crate::instances::schema::LoaderKind::Forge,
            loader_version: None,
            max_heap_mb: 2048,
            extra_jvm_args: String::new(),
            created_unix_ms: 1.0,
            eula_accepted: true,
            created_from_instance: None,
            handled_log_sig: Some("abc".into()),
            upload: None,
        };
        let back: ServerFile = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.handled_log_sig.as_deref(), Some("abc"));
    }
    #[test]
    fn old_server_json_without_handled_sig_deserializes() {
        let j = r#"{"id":"x","name":"n","mc_version":"1.20.1","loader":"forge","loader_version":null,"max_heap_mb":2048,"extra_jvm_args":"","created_unix_ms":1.0,"eula_accepted":true}"#;
        let s: ServerFile = serde_json::from_str(j).unwrap();
        assert_eq!(s.handled_log_sig, None);
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
        let w = ServerWithStatus::from_file(&s, true, Some(4321), Some(25565), false);
        assert_eq!(w.id, s.id);
        assert!(w.running);
        assert_eq!(w.pid, Some(4321));
        assert_eq!(w.port, Some(25565));
    }

    #[test]
    fn upload_config_roundtrip_and_skips_none() {
        let mut s = sample();
        assert!(!serde_json::to_string(&s).unwrap().contains("\"upload\""));
        s.upload = Some(UploadConfig {
            host: "h".into(),
            port: 22,
            user: "u".into(),
            remote_path: "/srv".into(),
            known_host_fp: None,
        });
        let back: ServerFile = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.upload.as_ref().unwrap().host, "h");
        assert_eq!(back.upload.as_ref().unwrap().port, 22);
    }
    #[test]
    fn old_server_json_without_upload_deserializes() {
        let j = r#"{"id":"x","name":"n","mc_version":"1.20.1","loader":"forge","loader_version":null,"max_heap_mb":2048,"extra_jvm_args":"","created_unix_ms":1.0,"eula_accepted":true}"#;
        let s: ServerFile = serde_json::from_str(j).unwrap();
        assert!(s.upload.is_none());
    }
    #[test]
    fn with_status_carries_upload_and_password_flag() {
        let mut s = sample();
        s.upload = Some(UploadConfig {
            host: "h".into(),
            port: 22,
            user: "u".into(),
            remote_path: "/s".into(),
            known_host_fp: None,
        });
        let w = ServerWithStatus::from_file(&s, false, None, None, true);
        assert_eq!(w.upload.as_ref().unwrap().host, "h");
        assert!(w.upload_password_set);
    }
}
