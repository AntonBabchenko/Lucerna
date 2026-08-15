//! Парсер/сериализатор `server.properties`. Сохраняет порядок строк и
//! комментарии (формат `java.util.Properties`, без экранирования —
//! достаточно для MC, который пишет простые `key=value`).

/// Строка файла: комментарий/пустая (raw) либо пара key=value.
#[derive(Debug, Clone, PartialEq)]
enum Line {
    Raw(String),
    Pair { key: String, value: String },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServerProperties {
    lines: Vec<Line>,
}

impl ServerProperties {
    pub fn parse(text: &str) -> Self {
        let mut lines = Vec::new();
        for raw in text.lines() {
            let trimmed = raw.trim_start();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                lines.push(Line::Raw(raw.to_string()));
            } else if let Some(eq) = raw.find('=') {
                lines.push(Line::Pair {
                    key: raw[..eq].to_string(),
                    value: raw[eq + 1..].to_string(),
                });
            } else {
                lines.push(Line::Raw(raw.to_string()));
            }
        }
        Self { lines }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().rev().find_map(|l| match l {
            Line::Pair { key: k, value } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Обновить значение существующего ключа на месте, иначе дописать в конец.
    pub fn set(&mut self, key: &str, value: &str) {
        for l in self.lines.iter_mut() {
            if let Line::Pair { key: k, value: v } = l {
                if k == key {
                    *v = value.to_string();
                    return;
                }
            }
        }
        self.lines.push(Line::Pair {
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    /// Check whether a curated key's value is valid. Unknown keys always return `true`.
    fn is_valid(key: &str, value: &str) -> bool {
        match key {
            "server-port" | "query.port" | "rcon.port" => {
                value.parse::<u16>().map(|n| n >= 1).unwrap_or(false)
            }
            // Unsigned counts / levels / timeouts.
            "max-players"
            | "view-distance"
            | "simulation-distance"
            | "spawn-protection"
            | "op-permission-level"
            | "function-permission-level"
            | "player-idle-timeout"
            | "max-world-size"
            | "entity-broadcast-range-percentage"
            | "max-chained-neighbor-updates"
            | "pause-when-empty-seconds"
            | "rate-limit" => value.parse::<u32>().is_ok(),
            // Signed, where -1 disables the feature.
            "max-tick-time" | "network-compression-threshold" => value.parse::<i64>().is_ok(),
            "difficulty" => matches!(value, "peaceful" | "easy" | "normal" | "hard"),
            "gamemode" => matches!(value, "survival" | "creative" | "adventure" | "spectator"),
            "pvp"
            | "online-mode"
            | "white-list"
            | "spawn-monsters"
            | "spawn-animals"
            | "spawn-npcs"
            | "allow-flight"
            | "allow-nether"
            | "hardcore"
            | "force-gamemode"
            | "generate-structures"
            | "enable-command-block"
            | "enforce-whitelist"
            | "enforce-secure-profile"
            | "hide-online-players"
            | "prevent-proxy-connections"
            | "use-native-transport"
            | "enable-status"
            | "accepts-transfers"
            | "enable-rcon"
            | "broadcast-rcon-to-ops"
            | "enable-query"
            | "require-resource-pack"
            | "enable-jmx-monitoring"
            | "broadcast-console-to-ops"
            | "log-ips"
            | "sync-chunk-writes" => matches!(value, "true" | "false"),
            _ => true,
        }
    }

    /// Collect all key-value pairs as owned tuples (in document order).
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.lines
            .iter()
            .filter_map(|l| match l {
                Line::Pair { key, value } => Some((key.clone(), value.clone())),
                _ => None,
            })
            .collect()
    }

    /// Установить курируемое значение с валидацией по ключу. Неизвестные
    /// ключи допускаются как есть (raw-редактор пишет напрямую через `set`).
    pub fn set_validated(&mut self, key: &str, value: &str) -> crate::error::Result<()> {
        if !Self::is_valid(key, value) {
            return Err(crate::error::Error::ServerInvalidProperty {
                key: key.to_string(),
                value: value.to_string(),
                reason: "value out of range or not allowed".into(),
            });
        }
        self.set(key, value);
        Ok(())
    }

    /// Validate every curated key currently present. Unknown keys pass.
    pub fn validate(&self) -> crate::error::Result<()> {
        for (k, v) in self.pairs() {
            if !Self::is_valid(&k, &v) {
                return Err(crate::error::Error::ServerInvalidProperty {
                    key: k,
                    value: v,
                    reason: "value out of range or not allowed".into(),
                });
            }
        }
        Ok(())
    }

    /// Сериализовать обратно с завершающим `\n` на каждой строке.
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            match l {
                Line::Raw(s) => out.push_str(s),
                Line::Pair { key, value } => {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(value);
                }
            }
            out.push('\n');
        }
        out
    }
}

/// Read a server's `server.properties` as raw text. Absent → `Ok("")`: a
/// server that has never started has no file yet, and every caller treats
/// "no file" as "all defaults". Any other read failure propagates as a real
/// error carrying the real path — collapsing "could not read" into "empty"
/// would let a caller that rewrites the file afterwards replace the user's
/// whole config with a near-empty one. Same NotFound discrimination as
/// `whitelist::read_array`.
pub fn read_properties_file(path: &std::path::Path) -> crate::error::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(raw),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(crate::error::Error::io(path.display().to_string(), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "#Minecraft server properties\n#Mon Jun 17\nmotd=Hello\nserver-port=25565\npvp=true\n";

    #[test]
    fn parse_reads_keys() {
        let p = ServerProperties::parse(SAMPLE);
        assert_eq!(p.get("motd"), Some("Hello"));
        assert_eq!(p.get("server-port"), Some("25565"));
        assert_eq!(p.get("missing"), None);
    }

    #[test]
    fn serialize_preserves_order_and_comments() {
        let p = ServerProperties::parse(SAMPLE);
        assert_eq!(p.serialize(), SAMPLE);
    }

    #[test]
    fn set_existing_key_updates_in_place() {
        let mut p = ServerProperties::parse(SAMPLE);
        p.set("motd", "Bye");
        assert_eq!(p.get("motd"), Some("Bye"));
        assert!(p
            .serialize()
            .starts_with("#Minecraft server properties\n#Mon Jun 17\nmotd=Bye\n"));
    }

    #[test]
    fn set_new_key_appends() {
        let mut p = ServerProperties::parse(SAMPLE);
        p.set("difficulty", "hard");
        assert!(p.serialize().ends_with("difficulty=hard\n"));
    }

    #[test]
    fn set_validated_accepts_good_port() {
        let mut p = ServerProperties::default();
        assert!(p.set_validated("server-port", "25565").is_ok());
        assert_eq!(p.get("server-port"), Some("25565"));
    }

    #[test]
    fn set_validated_rejects_bad_port() {
        let mut p = ServerProperties::default();
        let r = p.set_validated("server-port", "70000");
        assert!(matches!(
            r,
            Err(crate::error::Error::ServerInvalidProperty { .. })
        ));
    }

    #[test]
    fn set_validated_rejects_bad_difficulty() {
        let mut p = ServerProperties::default();
        assert!(p.set_validated("difficulty", "peaceful").is_ok());
        assert!(p.set_validated("difficulty", "nightmare").is_err());
    }

    #[test]
    fn set_validated_rejects_bad_bool() {
        let mut p = ServerProperties::default();
        assert!(p.set_validated("pvp", "true").is_ok());
        assert!(p.set_validated("pvp", "maybe").is_err());
    }

    #[test]
    fn validate_flags_bad_curated_value() {
        let mut p = ServerProperties::default();
        p.set("server-port", "70000");
        p.set("motd", "hi");
        assert!(p.validate().is_err());
        let mut ok = ServerProperties::default();
        ok.set("server-port", "25565");
        ok.set("custom-unknown", "whatever");
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn set_validated_accepts_new_bool_keys() {
        let mut p = ServerProperties::default();
        for k in [
            "hardcore",
            "enforce-whitelist",
            "enable-rcon",
            "sync-chunk-writes",
        ] {
            assert!(p.set_validated(k, "true").is_ok(), "{k} true");
            assert!(p.set_validated(k, "false").is_ok(), "{k} false");
            assert!(p.set_validated(k, "yes").is_err(), "{k} yes");
        }
    }

    #[test]
    fn set_validated_checks_signed_disable_keys() {
        let mut p = ServerProperties::default();
        // -1 disables the watchdog / compression — must be accepted.
        assert!(p.set_validated("max-tick-time", "-1").is_ok());
        assert!(p
            .set_validated("network-compression-threshold", "-1")
            .is_ok());
        assert!(p.set_validated("max-tick-time", "60000").is_ok());
        assert!(p.set_validated("max-tick-time", "abc").is_err());
    }

    #[test]
    fn set_validated_checks_new_count_keys() {
        let mut p = ServerProperties::default();
        assert!(p.set_validated("spawn-protection", "16").is_ok());
        assert!(p.set_validated("op-permission-level", "4").is_ok());
        assert!(p.set_validated("player-idle-timeout", "-3").is_err()); // u32
    }

    #[test]
    fn read_properties_file_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let raw = read_properties_file(&dir.path().join("server.properties")).unwrap();
        assert_eq!(raw, "");
    }

    #[test]
    fn read_properties_file_returns_existing_content_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.properties");
        std::fs::write(&path, SAMPLE).unwrap();
        assert_eq!(read_properties_file(&path).unwrap(), SAMPLE);
    }

    #[test]
    fn read_properties_file_unreadable_is_an_error_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.properties");
        // A directory at the file's path makes read_to_string fail with
        // something other than NotFound on every platform — the same shape as
        // a file we have no permission to read. Same trick as
        // `write_at_refuses_to_overwrite_when_it_cannot_back_up` in
        // src/datapacks/level_dat.rs.
        std::fs::create_dir(&path).unwrap();
        let r = read_properties_file(&path);
        assert!(
            matches!(r, Err(crate::error::Error::Io { .. })),
            "got: {r:?}"
        );
    }

    #[test]
    fn read_properties_file_error_names_the_real_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.properties");
        std::fs::create_dir(&path).unwrap();
        match read_properties_file(&path) {
            Err(crate::error::Error::Io { path: p, .. }) => {
                assert!(p.ends_with("server.properties"), "got path: {p}");
            }
            other => panic!("expected an Io error carrying the path, got: {other:?}"),
        }
    }
}
