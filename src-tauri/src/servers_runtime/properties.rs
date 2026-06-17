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
}
