//! The Java a game runs on when it is not Lucerna's own runtime.
//!
//! Lucerna downloads the Mojang JRE each Minecraft version names (`jre::`).
//! A user may instead point an instance or a server at a Java of their own, or
//! name one per Java major in Settings. This module decides which one a launch
//! uses (`resolve`), what a chosen executable is (`path`, `probe`), whether it
//! can run the game (`verdict`), and which Javas are installed on this machine
//! (`detect`). It never answers a chosen Java that fails with the managed
//! runtime: see `launch_java`.

pub mod detect;
pub mod install_phase;
pub mod path;
pub mod probe;
pub mod verdict;

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

pub use verdict::{JavaReport, JavaUnusableReason, JavaVerdict};

/// The Java major of Mojang's `jre-legacy` component — what a version JSON
/// without `javaVersion` (pre-1.17) runs on. Launch already falls back to that
/// component (`jre::DEFAULT_LEGACY_COMPONENT`); this is its major.
pub const LEGACY_MAJOR: u32 = 8;

/// What an instance or a server says about its Java. Stored as
/// `{"kind": "bundled"}` / `{"kind": "custom", "path": "…"}`; absent = follow
/// Settings' default for the Java major the version needs, else Lucerna's
/// runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaChoice {
    /// Always Lucerna's own runtime, whatever Settings names as the default.
    Bundled,
    /// This executable.
    Custom { path: String },
}

/// One row of Settings → Game → Java: the Java every instance and server
/// whose Minecraft version needs `major` uses, unless it chose another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct JavaDefault {
    pub major: u32,
    pub path: String,
}

/// Where a chosen Java came from — so a refusal can say which setting to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaOrigin {
    /// The instance's own choice.
    Instance,
    /// Settings → Game → Java, the row for this major.
    Default { major: u32 },
    /// The server's own choice.
    Server,
}

/// Whose choice is being resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Instance,
    Server,
}

/// Where a launch takes its Java from, after the choice is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaSource {
    /// Lucerna's runtime for the version's `javaVersion.component`.
    Managed,
    /// An executable the user chose, and where they chose it.
    Custom { path: PathBuf, origin: JavaOrigin },
}

/// Resolve a choice to a source. Pure. `required` is the Java major the
/// version needs; a Settings row for another major never applies.
pub fn resolve(
    choice: Option<&JavaChoice>,
    owner: Owner,
    defaults: &[JavaDefault],
    required: u32,
) -> JavaSource {
    match choice {
        Some(JavaChoice::Custom { path }) => JavaSource::Custom {
            path: PathBuf::from(path),
            origin: match owner {
                Owner::Instance => JavaOrigin::Instance,
                Owner::Server => JavaOrigin::Server,
            },
        },
        Some(JavaChoice::Bundled) => JavaSource::Managed,
        None => match defaults.iter().find(|d| d.major == required) {
            Some(d) => JavaSource::Custom {
                path: PathBuf::from(&d.path),
                origin: JavaOrigin::Default { major: required },
            },
            None => JavaSource::Managed,
        },
    }
}

/// The Settings defaults a launch needs. Read only when the choice defers to
/// them (`None`). A settings file that cannot be read is a refusal, never
/// "no defaults": that guess would run Lucerna's runtime for a user whose
/// Settings name another Java for this version (CLAUDE.md, Fallback
/// discipline: direction).
pub fn defaults_for<R>(
    choice: Option<&JavaChoice>,
    read: R,
) -> crate::error::Result<Vec<JavaDefault>>
where
    R: FnOnce() -> Result<Vec<JavaDefault>, String>,
{
    if choice.is_some() {
        return Ok(Vec::new());
    }
    read().map_err(|details| crate::error::Error::JavaSettingsUnreadable { details })
}

/// Settings' per-major Java rows, as `app.json` holds them now. An
/// unreadable file is an `Err` with its reason — never an empty list
/// (`defaults_for` turns it into a refusal for a launch that needs it).
pub fn settings_defaults(app: &tauri::AppHandle) -> Result<Vec<JavaDefault>, String> {
    let path = crate::paths::app_file(app).map_err(|e| e.to_string())?;
    crate::instances::store::read_app_json(&path)
        .map(|af| af.general.java_defaults)
        .map_err(|e| e.to_string())
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

/// Whether Lucerna's runtime for a component is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    Present,
    Missing,
    /// It could not be told (a stat failed) — kept apart from `Missing`.
    Unreadable(String),
}

/// Lucerna's runtime is present when both its install marker and its
/// executable are there. `try_exists`, so "absent" and "could not tell" stay
/// apart (CLAUDE.md, Fallback discipline: discrimination).
pub fn jre_presence(marker: &Path, exe: &Path) -> Presence {
    for p in [marker, exe] {
        match p.try_exists() {
            Ok(true) => {}
            Ok(false) => return Presence::Missing,
            Err(e) => return Presence::Unreadable(format!("{}: {e}", p.display())),
        }
    }
    Presence::Present
}

/// Probe and judge a custom executable. The one place I/O meets the pure
/// verdict: existence first (no spawn for a missing file), then the probe.
pub async fn check(exe: &Path, required: Option<u32>) -> Result<JavaReport, JavaUnusableReason> {
    let facts = match verdict::Facts::before_probe(path::existence(exe)) {
        Some(f) => f,
        None => verdict::Facts::Probed(crate::process::probe_java(&path::console_exe(exe)).await),
    };
    verdict::judge_custom(exe, facts, required, &probe::host_arch())
}

fn origin_words(origin: JavaOrigin) -> String {
    match origin {
        JavaOrigin::Instance => "chosen for this instance".into(),
        JavaOrigin::Default { major } => format!("Settings' default for Java {major}"),
        JavaOrigin::Server => "chosen for this server".into(),
    }
}

/// The executable a launch spawns.
///
/// `Managed` → `managed` as is. `Custom` → the stored path is normalised again
/// (`instance.json` and `app.json` are files anyone can edit; the UI's check is
/// not the boundary), then probed and judged against `required` by `check`,
/// and any refusal is the launch's error. There is deliberately no path from a
/// failed custom Java to `managed`: a user who chose a Java and silently gets
/// another is the bad fallback CLAUDE.md names.
pub async fn launch_java<F, Fut>(
    source: &JavaSource,
    managed: PathBuf,
    required: Option<u32>,
    check: F,
) -> crate::error::Result<PathBuf>
where
    F: FnOnce(PathBuf, Option<u32>) -> Fut,
    Fut: std::future::Future<Output = Result<JavaReport, JavaUnusableReason>>,
{
    let need = required.map_or_else(|| "unknown".to_string(), |r| format!("Java {r}"));
    let (stored, origin) = match source {
        JavaSource::Managed => {
            crate::diag!(
                "java: Lucerna's runtime {} (this version needs {need})",
                managed.display()
            );
            return Ok(managed);
        }
        JavaSource::Custom { path, origin } => (path, *origin),
    };
    let refuse = |reason: JavaUnusableReason| {
        crate::diag!(
            "java: the Java {} cannot be used: {} ({reason:?})",
            origin_words(origin),
            stored.display()
        );
        crate::error::Error::JavaUnusable {
            path: stored.to_string_lossy().into_owned(),
            origin,
            reason,
        }
    };
    let exe = path::normalize(&stored.to_string_lossy()).map_err(refuse)?;
    let report = check(exe.clone(), required).await.map_err(refuse)?;
    crate::diag!(
        "java: {}: {} (Java {}, {}, {:?}; this version needs {need})",
        origin_words(origin),
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

    fn custom(path: &str) -> JavaChoice {
        JavaChoice::Custom { path: path.into() }
    }

    fn row(major: u32, path: &str) -> JavaDefault {
        JavaDefault {
            major,
            path: path.into(),
        }
    }

    #[test]
    fn no_choice_and_no_default_runs_on_the_managed_runtime() {
        assert_eq!(resolve(None, Owner::Instance, &[], 21), JavaSource::Managed);
    }

    #[test]
    fn no_choice_follows_the_settings_row_for_the_needed_major() {
        assert_eq!(
            resolve(
                None,
                Owner::Instance,
                &[row(17, "/j17/bin/java"), row(21, "/j21/bin/java")],
                21
            ),
            JavaSource::Custom {
                path: PathBuf::from("/j21/bin/java"),
                origin: JavaOrigin::Default { major: 21 }
            }
        );
    }

    #[test]
    fn a_row_for_another_major_never_applies() {
        assert_eq!(
            resolve(None, Owner::Instance, &[row(17, "/j17/bin/java")], 21),
            JavaSource::Managed
        );
    }

    #[test]
    fn bundled_ignores_the_settings_rows() {
        assert_eq!(
            resolve(
                Some(&JavaChoice::Bundled),
                Owner::Instance,
                &[row(21, "/j21/bin/java")],
                21
            ),
            JavaSource::Managed
        );
    }

    #[test]
    fn an_own_choice_wins_over_the_settings_row_and_names_its_owner() {
        assert_eq!(
            resolve(
                Some(&custom("/mine/bin/java")),
                Owner::Instance,
                &[row(21, "/j21/bin/java")],
                21
            ),
            JavaSource::Custom {
                path: PathBuf::from("/mine/bin/java"),
                origin: JavaOrigin::Instance
            }
        );
        assert!(matches!(
            resolve(Some(&custom("/s/bin/java")), Owner::Server, &[], 21),
            JavaSource::Custom {
                origin: JavaOrigin::Server,
                ..
            }
        ));
    }

    #[test]
    fn resolve_never_answers_an_unreadable_settings_file_with_the_managed_runtime() {
        let r = defaults_for(None, || Err("app.json: expected value at line 1".into()));
        match r {
            Err(crate::error::Error::JavaSettingsUnreadable { details }) => {
                assert!(details.contains("app.json"), "{details}")
            }
            other => panic!("expected JavaSettingsUnreadable, got {other:?}"),
        }
    }

    #[test]
    fn defaults_are_not_read_when_the_choice_does_not_defer_to_them() {
        let r = defaults_for(Some(&JavaChoice::Bundled), || {
            panic!("an own choice never reads Settings")
        });
        assert_eq!(r.unwrap(), Vec::<JavaDefault>::new());
    }

    #[test]
    fn readable_defaults_pass_through() {
        let r = defaults_for(None, || Ok(vec![row(8, "/j8/bin/java")])).unwrap();
        assert_eq!(r, vec![row(8, "/j8/bin/java")]);
    }

    #[test]
    fn presence_tells_missing_from_present() {
        let t = tempfile::tempdir().unwrap();
        let marker = t.path().join(".installed");
        let exe = t.path().join("bin").join("java");
        assert_eq!(jre_presence(&marker, &exe), Presence::Missing);
        std::fs::write(&marker, b"x").unwrap();
        assert_eq!(jre_presence(&marker, &exe), Presence::Missing);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(jre_presence(&marker, &exe), Presence::Present);
    }

    #[test]
    fn choice_serialises_with_a_kind_tag() {
        let c = custom("/j/bin/java");
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
    fn java_exe_under(dir: &Path) -> PathBuf {
        dir.join("bin")
            .join(if cfg!(windows) { "javaw.exe" } else { "java" })
    }

    fn chosen(path: PathBuf) -> JavaSource {
        JavaSource::Custom {
            path,
            origin: JavaOrigin::Instance,
        }
    }

    #[tokio::test]
    async fn launch_java_never_answers_a_failed_custom_java_with_the_managed_one() {
        let managed = PathBuf::from("/lucerna/jres/java-runtime-delta/bin/java");
        let gone = java_exe_under(&std::env::temp_dir().join("lucerna-gone-jdk"));
        let r = launch_java(
            &chosen(gone.clone()),
            managed.clone(),
            Some(21),
            |_, _| async { Err(JavaUnusableReason::NotFound) },
        )
        .await;
        match r {
            Err(crate::error::Error::JavaUnusable {
                path,
                origin,
                reason,
            }) => {
                assert_eq!(path, gone.to_string_lossy());
                assert_eq!(origin, JavaOrigin::Instance);
                assert_eq!(reason, JavaUnusableReason::NotFound);
            }
            other => panic!("expected JavaUnusable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_failed_settings_default_says_it_came_from_settings() {
        let gone = java_exe_under(&std::env::temp_dir().join("lucerna-gone-default"));
        let source = JavaSource::Custom {
            path: gone,
            origin: JavaOrigin::Default { major: 17 },
        };
        let r = launch_java(&source, PathBuf::from("/m"), Some(17), |_, _| async {
            Err(JavaUnusableReason::NotFound)
        })
        .await;
        assert!(matches!(
            r,
            Err(crate::error::Error::JavaUnusable {
                origin: JavaOrigin::Default { major: 17 },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn launch_java_refuses_a_stored_path_that_is_not_a_java_without_probing() {
        // instance.json is a file on disk anyone can edit: the stored path is
        // validated again at launch, not only when the UI saved it.
        let r = launch_java(
            &chosen(PathBuf::from("relative/notepad.exe")),
            PathBuf::from("/m"),
            Some(21),
            |_, _| async { panic!("a path that is not a Java executable is never run") },
        )
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
        let r = launch_java(
            &JavaSource::Managed,
            managed.clone(),
            Some(21),
            |_, _| async { panic!("the managed runtime is not probed") },
        )
        .await
        .unwrap();
        assert_eq!(r, managed);
    }

    #[tokio::test]
    async fn launch_java_runs_a_usable_custom_java_with_the_requirement() {
        let exe = java_exe_under(&std::env::temp_dir().join("lucerna-jdk-21"));
        let r = launch_java(
            &chosen(exe.clone()),
            PathBuf::from("/m"),
            Some(17),
            |exe, req| async move {
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
            },
        )
        .await
        .unwrap();
        assert_eq!(r, exe);
    }

    #[test]
    fn the_gpu_preference_reaches_only_lucernas_runtime() {
        assert!(gpu_applies_to(&JavaSource::Managed));
        assert!(!gpu_applies_to(&chosen(PathBuf::from("/j/bin/java"))));
    }
}
