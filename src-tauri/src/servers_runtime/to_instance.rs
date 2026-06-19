//! Reverse of "server from instance": build a client instance that mirrors a
//! launcher-owned server (same MC version + loader + mods), optionally
//! pre-registering the server in the instance's multiplayer list. The server
//! is read read-only; nothing under it is written.

/// Multiplayer address for the saved-servers entry. A launcher server runs
/// locally, so the host is always `localhost`; the port comes from the
/// server's `server.properties` (`None` → Mojang's default 25565).
pub(crate) fn address_for_port(port: Option<u16>) -> String {
    format!("localhost:{}", port.unwrap_or(25565))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_uses_explicit_port() {
        assert_eq!(address_for_port(Some(25570)), "localhost:25570");
    }

    #[test]
    fn address_defaults_to_25565_when_absent() {
        assert_eq!(address_for_port(None), "localhost:25565");
    }
}
