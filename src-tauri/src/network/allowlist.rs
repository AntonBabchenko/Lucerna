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
//! ## Env override
//!
//! `LUCERNA_EXTRA_ALLOWED_HOSTS=h1,h2,…` adds extra patterns at
//! runtime. Its purpose is integration tests, so wiremock URLs
//! (`127.0.0.1`) aren't flagged as violations — but it resolves via
//! `test_seam`, which falls back to the real process env, so an
//! operator-set value takes effect in release builds too. It is
//! unset unless someone sets it deliberately. Documented as an
//! accepted trade-off in `docs/SECURITY.md` Part C and qualified in
//! `docs/PRINCIPLES.md` Part A commitment 1 and `PRIVACY.md` §3.

/// Mirror of `docs/PRINCIPLES.md` Part A item #2. The redundant
/// `piston-*.mojang.com` rows are intentional: they protect against
/// a future reader who deletes the `*.mojang.com` wildcard without
/// realising piston-* depend on it.
///
/// **Path scope.** Matching is host-level: an allowlisted host is
/// allowed at any path. That is deliberate, not an unfinished job —
/// every entry here is reached with paths the launcher builds itself
/// or reads out of that host's own API response (a release asset's
/// `browser_download_url`, a CDN URL from a version record), so a
/// path allowlist would have to enumerate whatever those services
/// return and would break on their next routing change while adding
/// nothing an attacker could not already do from an allowlisted
/// host. The PRINCIPLES.md table says the same for `api.github.com`
/// ("the allowlist matches on host, not path"); keep the two in step
/// if this ever becomes path-scoped.
const ALLOWED_PATTERNS: &[&str] = &[
    "*.minecraft.net",
    "*.mojang.com",
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "api.github.com",
    // Release-asset browser_download_url host. Redirects to the asset
    // CDN (host varies / changes over time), which reqwest follows
    // internally — the chokepoint only checks the initial URL. That is
    // acceptable here because update integrity rests on cosign + SHA-256
    // verification of the bytes, NOT on the transport host. Kept as a
    // single concrete host (not a wildcard) per the narrow-allowlist rule.
    "github.com",
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
    // FTB (Feed The Beast) modpack source. api = metadata, dist = file CDN.
    // CF-ref files reuse the existing edge/mediafilez forgecdn hosts above.
    "api.modpacks.ch",
    "dist.modpacks.ch",
    // Vanilla Tweaks datapack builder. One host serves both the per-family
    // category JSON and the zip a build request produces, so one concrete
    // entry covers the whole feature.
    "vanillatweaks.net",
    // ATLauncher modpack source. api = catalogue metadata, download.nodecdn.net
    // = Configs.json manifest + server-download mod files.
    "api.atlauncher.com",
    "download.nodecdn.net",
    // v0.2.0 Slice B (revived as cluster C) — Microsoft authentication chain.
    "login.microsoftonline.com",
    "login.live.com",
    "user.auth.xboxlive.com",
    "xsts.auth.xboxlive.com",
    "api.minecraftservices.com",
    // Own-server hosting (#6): public-IP echo for port-forward guidance. A
    // user-initiated, on-demand lookup (the user asks "what's my public
    // address?") — never automatic. ipify returns ONLY the caller's public IP
    // as plain text and sets no cookies; no request data beyond the bare GET is
    // sent. See docs/PRINCIPLES.md Part A item #2 and docs/SECURITY.md.
    "api.ipify.org",
    // Server plugins feature — Paper/Purpur cores. fill.papermc.io = Fill v3
    // build metadata; fill-data.papermc.io = the hash-addressed jar CDN the
    // API's download URLs point at (taken from the API response, never
    // constructed). The legacy api.papermc.io/v2 is shut down (HTTP 410 since
    // 2026-07-01) and deliberately NOT listed.
    "fill.papermc.io",
    "fill-data.papermc.io",
    // Purpur core builds (community-run infra; md5-verified downloads).
    "api.purpurmc.org",
    // Hangar plugin repository (PaperMC). hangar.papermc.io = api/v1 search +
    // versions; hangarcdn.papermc.io = hosted plugin files (the API download
    // endpoint 301s there; both ends are pinned here). Externally-hosted
    // plugin files (externalUrl) are NEVER downloaded in-app — the UI opens
    // the project page in the system browser instead.
    "hangar.papermc.io",
    "hangarcdn.papermc.io",
    // v0.21.0 — AI translation pre-fill. Exact hosts only (no wildcard): these
    // are OpenAI-compatible chat-completion endpoints the user opts into with
    // their own key. A local model is NOT here — loopback has its own narrow
    // seam in `network::loopback`.
    "api.anthropic.com",
    "generativelanguage.googleapis.com",
    "api.groq.com",
];

/// True if `host` matches any pattern in `ALLOWED_PATTERNS` or in
/// the `LUCERNA_EXTRA_ALLOWED_HOSTS` env override.
pub fn is_host_allowed(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Some(extra) = crate::test_seam::resolve("LUCERNA_EXTRA_ALLOWED_HOSTS") {
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
            crate::diag!("network: {initiator} refused — host not on allowlist: {url}");
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
    fn override_enables_extra_hosts() {
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        assert!(is_host_allowed("127.0.0.1"));
        assert!(is_host_allowed("localhost"));
    }

    #[test]
    fn override_empty_is_noop() {
        let _seam = crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "")]);
        assert!(!is_host_allowed("127.0.0.1"));
        assert!(!is_host_allowed(""));
    }

    #[test]
    fn vanillatweaks_host_is_allowed() {
        // The builder's catalogue GET and its build POST share one host, so
        // one concrete entry covers the whole feature.
        assert!(is_host_allowed("vanillatweaks.net"));
        assert!(check_url_allowed(
            "https://vanillatweaks.net/assets/resources/json/1.21/dpcategories.json",
            "vt-catalogue"
        )
        .is_ok());
        // Bare host, so exact match only — same rule the FTB and ipify
        // entries are pinned to.
        assert!(!is_host_allowed("evil.vanillatweaks.net"));
        assert!(!is_host_allowed("vanillatweaks.net.evil"));
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
        assert!(ALLOWED_PATTERNS.contains(&"api.modpacks.ch"));
        assert!(ALLOWED_PATTERNS.contains(&"dist.modpacks.ch"));
        assert!(ALLOWED_PATTERNS.contains(&"api.ipify.org"));
        // 7 from v0.1.0 + 4 from Slice A + 3 from v0.4.0 + 3 from v0.5.0 + 2 from v0.6.0 + 5 from cluster C + 1 github.com (auto-update) + 2 FTB hosts + 2 ATLauncher hosts + 1 ipify (hosting public-IP echo) + 3 Paper/Purpur core hosts + 2 Hangar hosts + 3 AI translation provider hosts + 1 Vanilla Tweaks host.
        assert_eq!(ALLOWED_PATTERNS.len(), 39);
    }

    #[test]
    fn ipify_host_allowed_exact_match_only() {
        assert!(is_host_allowed("api.ipify.org"));
        assert!(!is_host_allowed("evil.api.ipify.org"));
        assert!(!is_host_allowed("api.ipify.org.evil"));
        assert!(!is_host_allowed("ipify.org")); // bare apex not allowed
    }

    #[test]
    fn ftb_hosts_are_allowed_exact_match_only() {
        assert!(is_host_allowed("api.modpacks.ch"));
        assert!(is_host_allowed("dist.modpacks.ch"));
        assert!(!is_host_allowed("evil.api.modpacks.ch"));
        assert!(!is_host_allowed("evilapi.modpacks.ch"));
        assert!(!is_host_allowed("modpacks.ch")); // bare apex not allowed
    }

    #[test]
    fn atlauncher_hosts_are_allowed_exact_match_only() {
        assert!(is_host_allowed("api.atlauncher.com"));
        assert!(is_host_allowed("download.nodecdn.net"));
        assert!(!is_host_allowed("evil.api.atlauncher.com"));
        assert!(!is_host_allowed("evildownload.nodecdn.net"));
    }

    #[test]
    fn github_com_allowed_for_release_assets() {
        // Release asset browser_download_url hosts are github.com (which
        // redirects to the asset CDN; integrity rests on cosign + SHA-256,
        // not the transport host).
        assert!(is_host_allowed("github.com"));
        assert!(!is_host_allowed("notgithub.com"));
        assert!(!is_host_allowed("github.com.evil"));
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
    fn paper_purpur_core_hosts_are_allowed_exact_match_only() {
        assert!(is_host_allowed("fill.papermc.io"));
        assert!(is_host_allowed("fill-data.papermc.io"));
        assert!(is_host_allowed("api.purpurmc.org"));
        assert!(!is_host_allowed("evil.fill.papermc.io"));
        assert!(!is_host_allowed("evilfill.papermc.io"));
        assert!(!is_host_allowed("papermc.io")); // bare apex not allowed
        assert!(!is_host_allowed("purpurmc.org"));
        // The dead legacy API host must NOT be reachable (sunset 2026-07-01).
        assert!(!is_host_allowed("api.papermc.io"));
    }

    #[test]
    fn hangar_hosts_are_allowed_exact_match_only() {
        assert!(is_host_allowed("hangar.papermc.io"));
        assert!(is_host_allowed("hangarcdn.papermc.io"));
        assert!(!is_host_allowed("evil.hangar.papermc.io"));
        assert!(!is_host_allowed("evilhangar.papermc.io"));
    }

    #[test]
    fn ai_provider_hosts_are_allowed_and_lookalikes_are_not() {
        assert!(is_host_allowed("api.anthropic.com"));
        assert!(is_host_allowed("generativelanguage.googleapis.com"));
        assert!(is_host_allowed("api.groq.com"));
        assert!(!is_host_allowed("anthropic.com"));
        assert!(!is_host_allowed("api.anthropic.com.attacker.net"));
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
