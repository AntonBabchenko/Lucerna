//! Whether an instance's install needs Lucerna's Java runtime (spec
//! 2026-10-11-custom-java §7.1): phase 1b of `versions::install_version` skips
//! it for an instance that resolves to a usable Java of its own or of Settings,
//! and installs it otherwise.
//!
//! Anything that cannot be told — the instance or Settings unreadable, a chosen
//! Java whose path or probe fails — installs it. Downloading a runtime runs
//! nothing, and the launch still refuses a chosen Java that fails, so the
//! restrictive answer here is "install".

use std::path::PathBuf;

use super::{resolve, JavaChoice, JavaDefault, JavaReport, JavaSource, JavaUnusableReason, Owner};
use crate::versions::install::JrePhase;

/// The decision itself, over its inputs: the instance's choice, the Settings
/// rows (or why they could not be read), the major the version needs, and a
/// probe. Pure but for the probe, which the caller supplies.
pub async fn decide<P, Fut>(
    choice: Option<&JavaChoice>,
    defaults: crate::error::Result<Vec<JavaDefault>>,
    required: u32,
    probe: P,
) -> JrePhase
where
    P: FnOnce(PathBuf, u32) -> Fut,
    Fut: std::future::Future<Output = Result<JavaReport, JavaUnusableReason>>,
{
    let defaults = match defaults {
        Ok(d) => d,
        Err(e) => {
            crate::diag!("install: {e}; installing Lucerna's Java");
            return JrePhase::Install;
        }
    };
    let JavaSource::Custom { path, .. } = resolve(choice, Owner::Instance, &defaults, required)
    else {
        return JrePhase::Install;
    };
    let exe = match super::path::normalize(&path.to_string_lossy()) {
        Ok(exe) => exe,
        Err(reason) => {
            crate::diag!(
                "install: the chosen Java {} is not usable ({reason:?}); installing Lucerna's Java",
                path.display()
            );
            return JrePhase::Install;
        }
    };
    match probe(exe.clone(), required).await {
        Ok(_) => JrePhase::Skip { java: exe },
        Err(reason) => {
            crate::diag!(
                "install: the chosen Java {} cannot run this ({reason:?}); installing Lucerna's Java",
                exe.display()
            );
            JrePhase::Install
        }
    }
}

/// [`decide`] for an installed instance, once its version's JSON is known.
pub async fn for_instance(
    app: &tauri::AppHandle,
    instance_id: &str,
    details: &crate::versions::version_json::VersionDetails,
) -> JrePhase {
    let instance = match crate::instances::read_instance(app, instance_id) {
        Ok(i) => i,
        Err(e) => {
            crate::diag!("install: cannot read {instance_id} ({e}); installing Lucerna's Java");
            return JrePhase::Install;
        }
    };
    let defaults = super::defaults_for(instance.java.as_ref(), || super::settings_defaults(app));
    decide(
        instance.java.as_ref(),
        defaults,
        super::required_major(details),
        |exe, required| async move { super::check(&exe, Some(required)).await },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::java::JavaVerdict;
    use std::path::Path;

    fn exe_under(dir: &Path) -> PathBuf {
        dir.join("bin")
            .join(if cfg!(windows) { "javaw.exe" } else { "java" })
    }

    fn report(exe: &Path) -> JavaReport {
        JavaReport {
            path: exe.to_string_lossy().into_owned(),
            version: "21.0.4".into(),
            major: 21,
            vendor: None,
            arch: "x64".into(),
            verdict: JavaVerdict::Matches,
            gpu_choice_ignored: true,
        }
    }

    async fn usable(exe: PathBuf, _required: u32) -> Result<JavaReport, JavaUnusableReason> {
        Ok(report(&exe))
    }

    async fn unusable(_exe: PathBuf, _required: u32) -> Result<JavaReport, JavaUnusableReason> {
        Err(JavaUnusableReason::NotFound)
    }

    async fn never(_exe: PathBuf, _required: u32) -> Result<JavaReport, JavaUnusableReason> {
        panic!("Lucerna's runtime is not probed")
    }

    #[tokio::test]
    async fn an_instance_on_lucernas_java_installs_it() {
        assert_eq!(decide(None, Ok(vec![]), 21, never).await, JrePhase::Install);
        let pinned = JavaChoice::Bundled;
        let rows = Ok(vec![JavaDefault {
            major: 21,
            path: "/elsewhere/bin/java".into(),
        }]);
        assert_eq!(
            decide(Some(&pinned), rows, 21, never).await,
            JrePhase::Install
        );
    }

    #[tokio::test]
    async fn a_usable_java_of_its_own_skips_lucernas() {
        let dir = tempfile::tempdir().unwrap();
        let exe = exe_under(dir.path());
        let own = JavaChoice::Custom {
            path: exe.to_string_lossy().into_owned(),
        };
        assert_eq!(
            decide(Some(&own), Ok(vec![]), 21, usable).await,
            JrePhase::Skip { java: exe }
        );
    }

    #[tokio::test]
    async fn a_usable_settings_row_skips_lucernas() {
        let dir = tempfile::tempdir().unwrap();
        let exe = exe_under(dir.path());
        let rows = Ok(vec![JavaDefault {
            major: 21,
            path: exe.to_string_lossy().into_owned(),
        }]);
        assert_eq!(
            decide(None, rows, 21, usable).await,
            JrePhase::Skip { java: exe }
        );
    }

    #[tokio::test]
    async fn a_chosen_java_that_fails_its_probe_installs_lucernas() {
        let dir = tempfile::tempdir().unwrap();
        let own = JavaChoice::Custom {
            path: exe_under(dir.path()).to_string_lossy().into_owned(),
        };
        assert_eq!(
            decide(Some(&own), Ok(vec![]), 21, unusable).await,
            JrePhase::Install
        );
    }

    #[tokio::test]
    async fn a_chosen_path_that_is_not_a_java_installs_lucernas() {
        let own = JavaChoice::Custom {
            path: "relative/notepad.exe".into(),
        };
        assert_eq!(
            decide(Some(&own), Ok(vec![]), 21, never).await,
            JrePhase::Install
        );
    }

    #[tokio::test]
    async fn settings_it_could_not_read_install_lucernas() {
        let unreadable = Err(Error::JavaSettingsUnreadable {
            details: "expected value".into(),
        });
        assert_eq!(decide(None, unreadable, 21, never).await, JrePhase::Install);
    }

    // A row answers only for its own major.
    #[tokio::test]
    async fn a_row_for_another_major_is_not_used() {
        let rows = Ok(vec![JavaDefault {
            major: 17,
            path: "/j17/bin/java".into(),
        }]);
        assert_eq!(decide(None, rows, 21, never).await, JrePhase::Install);
    }
}
