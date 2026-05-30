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
//! `LUCERNA_EXTRA_ALLOWED_HOSTS=h1,h2,…` adds extra patterns at
//! runtime. Empty in production. Used by integration tests so
//! wiremock URLs (`127.0.0.1`) aren't flagged as violations.

/// Mirror of `docs/PRINCIPLES.md` Part A item #2. The redundant
/// `piston-*.mojang.com` rows are intentional: they protect against
/// a future reader who deletes the `*.mojang.com` wildcard without
/// realising piston-* depend on it.
///
/// **Path scope.** The PRINCIPLES.md table lists `api.github.com`
/// with a path scope (`/repos/AntonBabchenko/Lucerna/releases`).
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
    // v0.5.0 — Mod browser file CDNs.
    "cdn.modrinth.com",
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
    // v0.6.0 — Log sharing via mclo.gs paste service.
    "api.mclo.gs",
    "mclo.gs",
    // v0.2.0 Slice B (revived as cluster C) — Microsoft authentication chain.
    "login.microsoftonline.com",
    "login.live.com",
    "user.auth.xboxlive.com",
    "xsts.auth.xboxlive.com",
    "api.minecraftservices.com",
];

/// True if `host` matches any pattern in `ALLOWED_PATTERNS` or in
/// the `LUCERNA_EXTRA_ALLOWED_HOSTS` env override.
pub fn is_host_allowed(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Ok(extra) = std::env::var("LUCERNA_EXTRA_ALLOWED_HOSTS") {
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

/// Verify `url`'s host is on the allowlist. Returns
/// `Err(Error::HostNotAllowed)` for a disallowed or unparseable host.
/// `initiator` is the calling module — logged to stderr on rejection so
/// a blocked request is diagnosable, but kept out of the typed error
/// (the URL is the user-facing context).
pub fn check_url_allowed(url: &str, initiator: &str) -> crate::error::Result<()> {
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string));
    match host {
        Some(h) if is_host_allowed(&h) => Ok(()),
        _ => {
            eprintln!("network: {initiator} refused — host not on allowlist: {url}");
            Err(crate::error::Error::HostNotAllowed {
                url: url.to_string(),
            })
        }
    }
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

    // Serializes the env-var tests below: they mutate
    // LUCERNA_EXTRA_ALLOWED_HOSTS, which is process-global and shared
    // across cargo's parallel test threads. Uses the crate-wide lock so
    // these tests also serialize with wiremock tests in other modules.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
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
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        assert!(is_host_allowed("127.0.0.1"));
        assert!(is_host_allowed("localhost"));
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[test]
    fn env_override_empty_is_noop() {
        let _g = env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "");
        assert!(!is_host_allowed("127.0.0.1"));
        assert!(!is_host_allowed(""));
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");
    }

    #[test]
    fn allowed_patterns_match_documented_list() {
        assert!(ALLOWED_PATTERNS.contains(&"*.mojang.com"));
        assert!(ALLOWED_PATTERNS.contains(&"*.minecraft.net"));
        assert!(ALLOWED_PATTERNS.contains(&"api.github.com"));
        assert!(ALLOWED_PATTERNS.contains(&"meta.fabricmc.net"));
        assert!(ALLOWED_PATTERNS.contains(&"meta.quiltmc.org"));
        assert!(ALLOWED_PATTERNS.contains(&"maven.minecraftforge.net"));
        assert!(ALLOWED_PATTERNS.contains(&"files.minecraftforge.net"));
        assert!(ALLOWED_PATTERNS.contains(&"maven.neoforged.net"));
        assert!(ALLOWED_PATTERNS.contains(&"cdn.modrinth.com"));
        assert!(ALLOWED_PATTERNS.contains(&"edge.forgecdn.net"));
        assert!(ALLOWED_PATTERNS.contains(&"mediafilez.forgecdn.net"));
        // 7 from v0.1.0 + 4 from Slice A + 3 from v0.4.0 + 3 from v0.5.0 + 2 from v0.6.0 + 5 from cluster C.
        assert_eq!(ALLOWED_PATTERNS.len(), 24);
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

    #[test]
    fn mod_cdn_hosts_are_allowed_exact_match_only() {
        assert!(is_host_allowed("cdn.modrinth.com"));
        assert!(is_host_allowed("edge.forgecdn.net"));
        assert!(is_host_allowed("mediafilez.forgecdn.net"));
        // Exact match — not wildcards.
        assert!(!is_host_allowed("evil.cdn.modrinth.com"));
        assert!(!is_host_allowed("evilmediafilez.forgecdn.net"));
    }

    #[test]
    fn check_url_allowed_accepts_allowlisted_host() {
        assert!(check_url_allowed("https://piston-meta.mojang.com/x", "test").is_ok());
    }

    #[test]
    fn check_url_allowed_rejects_offlist_host() {
        let r = check_url_allowed("https://evil.example/x", "test");
        assert!(matches!(r, Err(crate::error::Error::HostNotAllowed { .. })));
    }

    #[test]
    fn check_url_allowed_rejects_unparseable_url() {
        let r = check_url_allowed("not a url", "test");
        assert!(matches!(r, Err(crate::error::Error::HostNotAllowed { .. })));
    }
}
