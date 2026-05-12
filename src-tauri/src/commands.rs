use specta::Type;

#[derive(Debug, serde::Serialize, Type)]
pub struct Greeting {
    pub message: String,
}

#[tauri::command]
#[specta::specta]
pub fn greet(name: String) -> Greeting {
    Greeting {
        message: format!("Hello, {name}! — FTlauncher is alive."),
    }
}

/// Return the most recent network activity entries (up to 200).
#[tauri::command]
#[specta::specta]
pub fn network_activity() -> Vec<crate::network::AuditEntry> {
    crate::network::recent()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_includes_name() {
        let g = greet("World".to_string());
        assert!(g.message.contains("World"));
        assert!(g.message.contains("FTlauncher"));
    }
}
