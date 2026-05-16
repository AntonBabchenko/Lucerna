//! Host allowlist for outbound HTTP. Implements
//! `is_host_allowed(host)` with exact-match and `*.subdomain`
//! wildcard semantics.
//!
//! ## Source-of-truth contract
//!
//! `ALLOWED_PATTERNS` below mirrors the table in
//! `docs/PRINCIPLES.md` Part A item #2 ("Default-allowed network
//! destinations"). The principle declares this code as the single
//! source of truth; the doc cites it.
//!
//! ## Wildcard semantics
//!
//! `*.mojang.com` matches `auth.mojang.com`, `piston-meta.mojang.com`,
//! etc. It does NOT match:
//! - `mojang.com` (no subdomain component)
//! - `evilmojang.com` (suffix matches but no dot boundary)
//! - `xmojang.com` (ditto)
//!
//! ## Env override (tests only)
//!
//! `FTLAUNCHER_EXTRA_ALLOWED_HOSTS=h1,h2,…` adds extra patterns at
//! runtime. Empty in production. Used by integration tests so
//! wiremock URLs (`127.0.0.1`) aren't flagged as violations.

/// Mirror of `docs/PRINCIPLES.md` Part A item #2. The redundant
/// `piston-*.mojang.com` rows are intentional: they protect against
/// a future reader who deletes the `*.mojang.com` wildcard without
/// realising piston-* depend on it.
///
/// **Path scope.** The PRINCIPLES.md table lists `api.github.com`
/// with a path scope (`/repos/AntonBabchenko/FTlauncher/releases`).
/// Our check is host-level only — we accept any path under
/// `api.github.com`. Path-level allowlisting is deferred until we
/// actually call GitHub (the self-update path doesn't exist yet in
/// v0.1.0). Same applies to `api.modrinth.com` / `api.curseforge.com`
/// — they're listed for future opt-in but no path scope is enforced.
const ALLOWED_PATTERNS: &[&str] = &[
    "*.minecraft.net",
    "*.mojang.com",
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "api.github.com",
    "api.modrinth.com",
    "api.curseforge.com",
    // v0.2.0 — Fabric + Quilt loader meta and library mirrors.
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    // v0.4.0 — Forge meta + installer mirrors. neoforged is added now
    // for v0.4.1 to avoid touching this list twice.
    "maven.minecraftforge.net",
    "files.minecraftforge.net",
    "maven.neoforged.net",
];

/// True if `host` matches any pattern in `ALLOWED_PATTERNS` or in
/// the `FTLAUNCHER_EXTRA_ALLOWED_HOSTS` env override.
pub fn is_host_allowed(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Ok(extra) = std::env::var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS") {
        for pat in extra.split(',').map(|s| s.trim().to_ascii_lowercase()) {
            if !pat.is_empty() && host_matches_pattern(&host, &pat) {
                return true;
            }
        }
    }
    ALLOWED_PATTERNS
        .iter()
        .any(|pat| host_matches_pattern(&host, pat))
}

/// The documented allowlist patterns (without the env override).
/// Exposed so UI / commands can present "what is the allowlist?"
/// to users without re-parsing this file.
pub fn allowed_host_patterns() -> Vec<&'static str> {
    ALLOWED_PATTERNS.to_vec()
}

fn host_matches_pattern(host: &str, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // Anchored host-suffix: host must end with `.suffix` and have
        // at least one character before that dot.
        host.len() > suffix.len() + 1
            && host.ends_with(suffix)
            && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
    } else {
        host == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    // Serializes the env-var tests below: they mutate
    // FTLAUNCHER_EXTRA_ALLOWED_HOSTS, which is process-global and shared
    // across cargo's parallel test threads.
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn wildcard_matches_subdomain() {
        assert!(is_host_allowed("auth.mojang.com"));
        assert!(is_host_allowed("piston-meta.mojang.com"));
        assert!(is_host_allowed("session.minecraft.net"));
    }

    #[test]
    fn wildcard_rejects_root_with_no_subdomain() {
        assert!(!is_host_allowed("mojang.com"));
        assert!(!is_host_allowed("minecraft.net"));
    }

    #[test]
    fn wildcard_rejects_homoglyph_suffix() {
        assert!(!is_host_allowed("evilmojang.com"));
        assert!(!is_host_allowed("xmojang.com"));
        assert!(!is_host_allowed("notminecraft.net"));
    }

    #[test]
    fn exact_match_works() {
        assert!(is_host_allowed("api.github.com"));
        assert!(is_host_allowed("api.modrinth.com"));
        assert!(is_host_allowed("api.curseforge.com"));
    }

    #[test]
    fn exact_match_rejects_subdomain_of_exact_entry() {
        assert!(!is_host_allowed("evil.api.github.com"));
        assert!(!is_host_allowed("api.github.com.evil"));
    }

    #[test]
    fn empty_host_is_rejected() {
        assert!(!is_host_allowed(""));
    }

    #[test]
    fn case_insensitive() {
        assert!(is_host_allowed("AUTH.MOJANG.COM"));
        assert!(is_host_allowed("Api.GitHub.Com"));
    }

    #[test]
    fn trailing_dot_tolerated() {
        assert!(is_host_allowed("auth.mojang.com."));
        assert!(is_host_allowed("api.github.com."));
    }

    #[test]
    fn env_override_enables_extra_hosts() {
        let _g = env_lock();
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        assert!(is_host_allowed("127.0.0.1"));
        assert!(is_host_allowed("localhost"));
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[test]
    fn env_override_empty_is_noop() {
        let _g = env_lock();
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "");
        assert!(!is_host_allowed("127.0.0.1"));
        assert!(!is_host_allowed(""));
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[test]
    fn allowed_host_patterns_returns_documented_list() {
        let patterns = allowed_host_patterns();
        assert!(patterns.contains(&"*.mojang.com"));
        assert!(patterns.contains(&"*.minecraft.net"));
        assert!(patterns.contains(&"api.github.com"));
        assert!(patterns.contains(&"meta.fabricmc.net"));
        assert!(patterns.contains(&"meta.quiltmc.org"));
        assert!(patterns.contains(&"maven.minecraftforge.net"));
        assert!(patterns.contains(&"files.minecraftforge.net"));
        assert!(patterns.contains(&"maven.neoforged.net"));
        // 7 from v0.1.0 + 4 from Slice A + 3 from v0.4.0.
        assert_eq!(patterns.len(), 14);
    }

    #[test]
    fn fabric_quilt_hosts_are_allowed() {
        assert!(is_host_allowed("meta.fabricmc.net"));
        assert!(is_host_allowed("maven.fabricmc.net"));
        assert!(is_host_allowed("meta.quiltmc.org"));
        assert!(is_host_allowed("maven.quiltmc.org"));
        // Exact-match — these are not wildcards.
        assert!(!is_host_allowed("evil.meta.fabricmc.net"));
        assert!(!is_host_allowed("evilmeta.fabricmc.net"));
    }
}
