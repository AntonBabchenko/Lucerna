//! The Java a game runs on when it is not Lucerna's own runtime.
//!
//! Lucerna downloads the Mojang JRE each Minecraft version names (`jre::`).
//! A user may instead point an instance at a Java of their own. This module
//! decides which one a launch uses (`resolve`), what a chosen executable is
//! (`path`, `probe`), whether it can run the game (`verdict`), and which Javas
//! are installed on this machine (`detect`). It never answers a chosen Java
//! that fails with the managed runtime: see `launch_java`.

pub mod detect;
pub mod path;
pub mod probe;
pub mod verdict;

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

pub use verdict::{JavaReport, JavaUnusableReason, JavaVerdict};

/// The Java major of Mojang's `jre-legacy` component — what a version JSON
/// without `javaVersion` (pre-1.17) runs on. Launch already falls back to that
/// component (`jre::DEFAULT_LEGACY_COMPONENT`); this is its major.
pub const LEGACY_MAJOR: u32 = 8;

/// What an instance says about its Java. Stored in `instance.json` as
/// `{"kind": "bundled"}` / `{"kind": "custom", "path": "…"}`; absent = the
/// default (Lucerna's runtime).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaChoice {
    /// Always Lucerna's own runtime. Distinct from "absent" once Settings can
    /// name a default per Java version; until then the two behave the same.
    Bundled,
    /// This executable.
    Custom { path: String },
}

/// Where a launch takes its Java from, after the choice is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaSource {
    /// Lucerna's runtime for the version's `javaVersion.component`.
    Managed,
    /// An executable the user chose.
    Custom(PathBuf),
}

/// Resolve an instance's choice to a source. Pure.
pub fn resolve(choice: Option<&JavaChoice>) -> JavaSource {
    match choice {
        Some(JavaChoice::Custom { path }) => JavaSource::Custom(PathBuf::from(path)),
        Some(JavaChoice::Bundled) | None => JavaSource::Managed,
    }
}

/// The Java major a version needs: `javaVersion.majorVersion`, or the legacy
/// runtime's major for a version JSON without it (what launch already runs).
pub fn required_major(details: &crate::versions::version_json::VersionDetails) -> u32 {
    details
        .java_version
        .as_ref()
        .map(|jv| jv.major_version)
        .unwrap_or(LEGACY_MAJOR)
}

/// Settings' graphics-card choice is written to the Windows registry keyed by
/// executable path. Lucerna's own runtime is Lucerna's; a system Java's
/// `javaw.exe` is shared with every other Java program on the machine, so it
/// is left alone (spec §8.3).
pub fn gpu_applies_to(source: &JavaSource) -> bool {
    matches!(source, JavaSource::Managed)
}

/// Probe and judge a custom executable. The one place I/O meets the pure
/// verdict: existence first (no spawn for a missing file), then the probe.
pub async fn check(
    exe: &std::path::Path,
    required: Option<u32>,
) -> Result<JavaReport, JavaUnusableReason> {
    let facts = match verdict::Facts::before_probe(path::existence(exe)) {
        Some(f) => f,
        None => verdict::Facts::Probed(crate::process::probe_java(&path::console_exe(exe)).await),
    };
    verdict::judge_custom(exe, facts, required, &probe::host_arch())
}

/// The executable a launch spawns.
///
/// `Managed` → `managed` as is. `Custom` → the stored path is normalised again
/// (`instance.json` is a file anyone can edit; the UI's check is not the
/// boundary), then probed and judged against `required` by `check`, and any
/// refusal is the launch's error. There is deliberately no path from a failed
/// custom Java to `managed`: a user who chose a Java and silently gets another
/// is the bad fallback CLAUDE.md names.
pub async fn launch_java<F, Fut>(
    source: &JavaSource,
    managed: PathBuf,
    required: u32,
    check: F,
) -> crate::error::Result<PathBuf>
where
    F: FnOnce(PathBuf, Option<u32>) -> Fut,
    Fut: std::future::Future<Output = Result<JavaReport, JavaUnusableReason>>,
{
    let stored = match source {
        JavaSource::Managed => {
            crate::diag!(
                "java: Lucerna's runtime {} (this version needs Java {required})",
                managed.display()
            );
            return Ok(managed);
        }
        JavaSource::Custom(stored) => stored,
    };
    let refuse = |reason: JavaUnusableReason| {
        crate::diag!(
            "java: the Java chosen for this instance cannot be used: {} ({reason:?})",
            stored.display()
        );
        crate::error::Error::JavaUnusable {
            path: stored.to_string_lossy().into_owned(),
            reason,
        }
    };
    let exe = path::normalize(&stored.to_string_lossy()).map_err(refuse)?;
    let report = check(exe.clone(), Some(required)).await.map_err(refuse)?;
    crate::diag!(
        "java: chosen for this instance: {} (Java {}, {}, {:?}; this version needs Java {required})",
        exe.display(),
        report.version,
        report.arch,
        report.verdict
    );
    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_choice_runs_on_the_managed_runtime() {
        assert_eq!(resolve(None), JavaSource::Managed);
    }

    #[test]
    fn bundled_runs_on_the_managed_runtime() {
        assert_eq!(resolve(Some(&JavaChoice::Bundled)), JavaSource::Managed);
    }

    #[test]
    fn a_custom_choice_names_its_executable() {
        let c = JavaChoice::Custom {
            path: "/opt/jdk-21/bin/java".into(),
        };
        assert_eq!(
            resolve(Some(&c)),
            JavaSource::Custom(PathBuf::from("/opt/jdk-21/bin/java"))
        );
    }

    #[test]
    fn choice_serialises_with_a_kind_tag() {
        let c = JavaChoice::Custom {
            path: "/j/bin/java".into(),
        };
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"kind":"custom","path":"/j/bin/java"}"#
        );
        assert_eq!(
            serde_json::from_str::<JavaChoice>(r#"{"kind":"bundled"}"#).unwrap(),
            JavaChoice::Bundled
        );
    }

    /// An absolute executable path this platform's `path::normalize` accepts.
    fn java_exe_under(dir: &std::path::Path) -> PathBuf {
        dir.join("bin")
            .join(if cfg!(windows) { "javaw.exe" } else { "java" })
    }

    #[tokio::test]
    async fn launch_java_never_answers_a_failed_custom_java_with_the_managed_one() {
        let managed = PathBuf::from("/lucerna/jres/java-runtime-delta/bin/java");
        let gone = java_exe_under(&std::env::temp_dir().join("lucerna-gone-jdk"));
        let chosen = JavaSource::Custom(gone.clone());
        let r = launch_java(&chosen, managed.clone(), 21, |_, _| async {
            Err(JavaUnusableReason::NotFound)
        })
        .await;
        match r {
            Err(crate::error::Error::JavaUnusable { path, reason }) => {
                assert_eq!(path, gone.to_string_lossy());
                assert_eq!(reason, JavaUnusableReason::NotFound);
            }
            other => panic!("expected JavaUnusable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn launch_java_refuses_a_stored_path_that_is_not_a_java_without_probing() {
        // instance.json is a file on disk anyone can edit: the stored path is
        // validated again at launch, not only when the UI saved it.
        let chosen = JavaSource::Custom(PathBuf::from("relative/notepad.exe"));
        let r = launch_java(&chosen, PathBuf::from("/m"), 21, |_, _| async {
            panic!("a path that is not a Java executable is never run")
        })
        .await;
        assert!(matches!(
            r,
            Err(crate::error::Error::JavaUnusable {
                reason: JavaUnusableReason::NotJava,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn launch_java_runs_managed_without_probing() {
        let managed = PathBuf::from("/lucerna/jres/x/bin/java");
        let r = launch_java(&JavaSource::Managed, managed.clone(), 21, |_, _| async {
            panic!("the managed runtime is not probed")
        })
        .await
        .unwrap();
        assert_eq!(r, managed);
    }

    #[tokio::test]
    async fn launch_java_runs_a_usable_custom_java_with_the_requirement() {
        let exe = java_exe_under(&std::env::temp_dir().join("lucerna-jdk-21"));
        let chosen = JavaSource::Custom(exe.clone());
        let r = launch_java(&chosen, PathBuf::from("/m"), 17, |exe, req| async move {
            assert_eq!(req, Some(17));
            Ok(JavaReport {
                path: exe.to_string_lossy().into_owned(),
                version: "21.0.4".into(),
                major: 21,
                vendor: None,
                arch: "x64".into(),
                verdict: JavaVerdict::Newer {
                    found: 21,
                    required: 17,
                },
                gpu_choice_ignored: false,
            })
        })
        .await
        .unwrap();
        assert_eq!(r, exe);
    }

    #[test]
    fn the_gpu_preference_reaches_only_lucernas_runtime() {
        assert!(gpu_applies_to(&JavaSource::Managed));
        assert!(!gpu_applies_to(&JavaSource::Custom(PathBuf::from(
            "/j/bin/java"
        ))));
    }
}
