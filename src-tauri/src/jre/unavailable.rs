//! Telling "Lucerna's runtime cannot be had" apart from other failures of
//! `ensure_jre` (spec 2026-10-11-custom-java §7.1). Only the former offers the
//! rescue — a Java on this computer standing in for the game.

use crate::error::{Error, JreStage, JreUnavailableReason};

/// Pure: an `ensure_jre` error → `JreUnavailable` when it means the runtime
/// cannot be obtained; anything else passes through unchanged.
///
/// A transfer failure (`Network`, `HostNotAllowed`, `HashMismatch`, `Io`)
/// counts only while the runtime is ABSENT locally: `ensure_jre` fetches
/// Mojang's manifest before it looks at the install marker, so an offline user
/// whose runtime is complete also gets a network error — that is not evidence
/// the runtime is missing. A marker that could not be checked is not evidence
/// either, so it passes the error through too.
pub fn classify(
    err: Error,
    component: &str,
    major: u32,
    stage: JreStage,
    marker_present: Result<bool, String>,
) -> Error {
    let reason = match &err {
        Error::UnsupportedPlatform { os, arch } => JreUnavailableReason::UnsupportedPlatform {
            os: os.clone(),
            arch: arch.clone(),
        },
        Error::UnknownVersion { id } => JreUnavailableReason::UnknownComponent { id: id.clone() },
        Error::Network { .. }
        | Error::HostNotAllowed { .. }
        | Error::HashMismatch { .. }
        | Error::Io { .. } => match marker_present {
            Ok(false) => JreUnavailableReason::Download {
                cause: Box::new(err.clone()),
            },
            Ok(true) => return err,
            Err(why) => {
                crate::diag!("jre: cannot tell whether {component} is installed ({why})");
                return err;
            }
        },
        _ => return err,
    };
    Error::JreUnavailable {
        component: component.to_string(),
        major,
        stage,
        reason,
    }
}

/// `classify` with the install marker checked on disk.
pub fn unavailable(
    err: Error,
    component: &str,
    major: u32,
    stage: JreStage,
    app: &tauri::AppHandle,
) -> Error {
    let marker = super::install::marker_path(component, app)
        .map_err(|e| e.to_string())
        .and_then(|p| p.try_exists().map_err(|e| e.to_string()));
    classify(err, component, major, stage, marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net() -> Error {
        Error::Network {
            url: "https://piston-meta.mojang.com/x".into(),
            failure: crate::error::NetworkFailure::Unreachable,
            details: "dns".into(),
        }
    }

    fn reason(e: Error) -> Option<JreUnavailableReason> {
        match e {
            Error::JreUnavailable { reason, .. } => Some(reason),
            _ => None,
        }
    }

    #[test]
    fn a_failed_fetch_of_an_absent_runtime_is_unavailable() {
        let r = reason(classify(
            net(),
            "java-runtime-delta",
            21,
            JreStage::GameRuntime,
            Ok(false),
        ));
        assert!(matches!(r, Some(JreUnavailableReason::Download { .. })));
    }

    #[test]
    fn a_failed_fetch_while_the_runtime_is_there_is_not_a_rescue() {
        let e = classify(
            net(),
            "java-runtime-delta",
            21,
            JreStage::GameRuntime,
            Ok(true),
        );
        assert!(matches!(e, Error::Network { .. }));
    }

    #[test]
    fn an_uncheckable_marker_is_not_a_rescue_either() {
        let e = classify(
            net(),
            "java-runtime-delta",
            21,
            JreStage::GameRuntime,
            Err("denied".into()),
        );
        assert!(matches!(e, Error::Network { .. }));
    }

    #[test]
    fn no_runtime_for_this_system_is_unavailable_whatever_is_on_disk() {
        let e = Error::UnsupportedPlatform {
            os: "linux".into(),
            arch: "aarch64".into(),
        };
        let r = reason(classify(
            e,
            "jre-legacy",
            8,
            JreStage::InstallerTools,
            Ok(true),
        ));
        assert!(matches!(
            r,
            Some(JreUnavailableReason::UnsupportedPlatform { .. })
        ));
    }

    #[test]
    fn an_unknown_component_is_unavailable() {
        let e = Error::UnknownVersion {
            id: "jre/java-runtime-zeta@linux".into(),
        };
        let r = reason(classify(
            e,
            "java-runtime-zeta",
            25,
            JreStage::GameRuntime,
            Ok(false),
        ));
        assert!(matches!(
            r,
            Some(JreUnavailableReason::UnknownComponent { .. })
        ));
    }

    #[test]
    fn other_errors_pass_through() {
        let e = classify(
            Error::InstanceBusy,
            "x",
            8,
            JreStage::GameRuntime,
            Ok(false),
        );
        assert!(matches!(e, Error::InstanceBusy));
    }

    #[test]
    fn a_download_reason_keeps_its_cause_typed() {
        // The UI words the cause by its own rules (a network failure names the
        // site and why), so it travels as the error, not as English text.
        match reason(classify(
            net(),
            "java-runtime-delta",
            21,
            JreStage::GameRuntime,
            Ok(false),
        )) {
            Some(JreUnavailableReason::Download { cause }) => assert_eq!(*cause, net()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_stage_and_major_travel_with_it() {
        match classify(
            net(),
            "java-runtime-gamma",
            17,
            JreStage::InstallerTools,
            Ok(false),
        ) {
            Error::JreUnavailable {
                component,
                major,
                stage,
                ..
            } => {
                assert_eq!(component, "java-runtime-gamma");
                assert_eq!(major, 17);
                assert_eq!(stage, JreStage::InstallerTools);
            }
            other => panic!("{other:?}"),
        }
    }
}
