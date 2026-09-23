// Post-update "What's new" support commands. The changelog itself is rendered
// entirely on the frontend from the embedded CHANGELOG.md; the backend only
// (a) reports the running version so the UI can tell an update happened, and
// (b) persists which version's changelog the user has already been shown so the
// prompt fires once per version. `changelog_mark_seen` mirrors `update_dismiss`
// — a read-modify-write of app.json that leaves everything else untouched.

/// The running launcher version (compile-time `CARGO_PKG_VERSION`), the same
/// source the updater and the CHANGELOG headings use. Infallible.
#[tauri::command]
#[specta::specta]
pub async fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Persist that the user has been shown the post-update changelog for
/// `version`, so the "What's new" prompt is not shown again for it.
#[tauri::command]
#[specta::specta]
pub async fn changelog_mark_seen(
    app: tauri::AppHandle,
    version: String,
) -> crate::error::Result<()> {
    let path =
        crate::paths::app_file(&app).map_err(|e| crate::error::Error::io("<app_file>", e))?;
    crate::instances::store::update_app_json(&path, |af| {
        af.changelog_seen_version = Some(version);
        crate::instances::store::Verdict::Write
    })
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// Build identity (Settings → About, "Copy version info")
// ---------------------------------------------------------------------------

/// Which build this is. A beta tester must be able to tell rc.1 from rc.2 from
/// the release, and a bug report must say so without retyping anything.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildKind {
    /// Built by the release workflow from a version tag.
    Tagged { tag: String },
    /// A tag was baked in but is not a version tag. Shown as it is (sanitised)
    /// rather than dropped: an official build must never read as a local one.
    Unrecognised { raw: String },
    /// A release-profile build with no tag — `pnpm tauri build` on someone's
    /// machine, or a fork's workflow that does not set one.
    Local,
    /// A debug build (`pnpm tauri dev`).
    Development,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct BuildInfo {
    pub version: String,
    pub build: BuildKind,
    /// Short commit (7 hex chars), when the build recorded a valid one.
    pub commit: Option<String>,
    /// `owner/repo` when the build came from a repository other than Lucerna's.
    pub fork: Option<String>,
    pub os: String,
    pub arch: String,
}

/// What the compiler baked in, before any of it is trusted.
pub struct RawBuild<'a> {
    pub version: &'a str,
    pub tag: Option<&'a str>,
    pub commit: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub debug: bool,
    pub os: &'a str,
    pub arch: &'a str,
}

const UPSTREAM_REPO: &str = "AntonBabchenko/Lucerna";

/// Pure: validate the baked-in values. Nothing from the environment is shown
/// unchecked.
pub fn build_info_from(raw: RawBuild<'_>) -> BuildInfo {
    let build = match raw.tag.filter(|t| !t.is_empty()) {
        // A debug build is a development build whatever was baked in: the
        // release workflow never builds debug.
        _ if raw.debug => BuildKind::Development,
        None => BuildKind::Local,
        Some(tag) if is_version_tag(tag) => BuildKind::Tagged {
            tag: tag.to_owned(),
        },
        Some(tag) => BuildKind::Unrecognised {
            raw: tag
                .chars()
                .filter(|c| c.is_ascii_graphic())
                .take(MAX_SHOWN_TAG)
                .collect(),
        },
    };
    let commit = raw
        .commit
        .filter(|c| (7..=40).contains(&c.len()) && c.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|c| c[..7].to_owned());
    let fork = raw
        .repo
        .filter(|r| !r.is_empty() && *r != UPSTREAM_REPO)
        .map(str::to_owned);
    BuildInfo {
        version: raw.version.to_owned(),
        build,
        commit,
        fork,
        os: raw.os.to_owned(),
        arch: raw.arch.to_owned(),
    }
}

/// An unrecognised tag is shown, but never at unbounded length.
const MAX_SHOWN_TAG: usize = 40;

/// `vMAJOR.MINOR.PATCH` with an optional `-rc.N` — the only tags the release
/// workflow runs on and the only ones worth trusting as a version.
fn is_version_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix('v') else {
        return false;
    };
    let (core, rc) = match rest.split_once("-rc.") {
        Some((core, n)) => (core, Some(n)),
        None => (rest, None),
    };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| digits(p)) && rc.is_none_or(digits)
}

/// The running build's identity. Infallible: every input is compile-time.
#[tauri::command]
#[specta::specta]
pub async fn app_build_info() -> BuildInfo {
    build_info_from(RawBuild {
        version: env!("CARGO_PKG_VERSION"),
        tag: option_env!("LUCERNA_BUILD_TAG"),
        commit: option_env!("LUCERNA_BUILD_COMMIT"),
        repo: option_env!("LUCERNA_BUILD_REPO"),
        debug: cfg!(debug_assertions),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    })
}

#[cfg(test)]
mod build_info_tests {
    use super::*;

    fn raw<'a>(
        tag: Option<&'a str>,
        commit: Option<&'a str>,
        repo: Option<&'a str>,
    ) -> RawBuild<'a> {
        RawBuild {
            version: "0.25.0",
            tag,
            commit,
            repo,
            debug: false,
            os: "windows",
            arch: "x86_64",
        }
    }

    #[test]
    fn a_debug_build_is_development_whatever_was_baked_in() {
        let mut r = raw(Some("v0.25.0"), None, None);
        r.debug = true;
        assert_eq!(build_info_from(r).build, BuildKind::Development);
    }

    #[test]
    fn no_tag_is_a_local_build() {
        assert_eq!(
            build_info_from(raw(None, None, None)).build,
            BuildKind::Local
        );
        assert_eq!(
            build_info_from(raw(Some(""), None, None)).build,
            BuildKind::Local
        );
    }

    #[test]
    fn a_version_tag_is_shown_as_it_is() {
        for tag in ["v0.25.0", "v0.25.0-rc.2", "v10.0.123"] {
            assert_eq!(
                build_info_from(raw(Some(tag), None, None)).build,
                BuildKind::Tagged { tag: tag.into() }
            );
        }
    }

    #[test]
    fn any_other_tag_is_unrecognised_not_local() {
        for tag in ["v0.25", "main", "v1.2.3-beta", "0.25.0", "v1.2.3-rc."] {
            assert_eq!(
                build_info_from(raw(Some(tag), None, None)).build,
                BuildKind::Unrecognised { raw: tag.into() },
                "{tag}"
            );
        }
    }

    #[test]
    fn an_unrecognised_tag_is_sanitised_and_cut() {
        let long = "x".repeat(60);
        let info = build_info_from(raw(Some(&long), None, None));
        assert_eq!(
            info.build,
            BuildKind::Unrecognised {
                raw: "x".repeat(40)
            }
        );
        let info = build_info_from(raw(Some("bad\u{7}tag\u{202e}"), None, None));
        assert_eq!(
            info.build,
            BuildKind::Unrecognised {
                raw: "badtag".into()
            }
        );
    }

    #[test]
    fn a_commit_is_kept_short_and_only_when_it_is_hex() {
        let sha = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
        assert_eq!(
            build_info_from(raw(None, Some(sha), None))
                .commit
                .as_deref(),
            Some("a1b2c3d")
        );
        assert_eq!(
            build_info_from(raw(None, Some("a1b2c3d"), None))
                .commit
                .as_deref(),
            Some("a1b2c3d")
        );
        assert_eq!(
            build_info_from(raw(None, Some("xyz1234"), None)).commit,
            None
        );
        assert_eq!(
            build_info_from(raw(None, Some("a1b2c3"), None)).commit,
            None
        );
        assert_eq!(build_info_from(raw(None, Some(""), None)).commit, None);
    }

    #[test]
    fn a_build_from_another_repository_says_so() {
        let info = build_info_from(raw(Some("v0.25.0"), None, Some("someone/Lucerna")));
        assert_eq!(info.fork.as_deref(), Some("someone/Lucerna"));
        assert_eq!(
            build_info_from(raw(None, None, Some(UPSTREAM_REPO))).fork,
            None
        );
        assert_eq!(build_info_from(raw(None, None, Some(""))).fork, None);
        assert_eq!(build_info_from(raw(None, None, None)).fork, None);
    }

    #[test]
    fn version_and_platform_pass_through() {
        let info = build_info_from(raw(None, None, None));
        assert_eq!(info.version, "0.25.0");
        assert_eq!(
            (info.os.as_str(), info.arch.as_str()),
            ("windows", "x86_64")
        );
    }
}
