//! Whether a probed Java can run a Minecraft version (spec §4.3).

use super::path::Existence;
use super::probe::{parse_probe_output, Arch, JavaInfo};
use serde::Serialize;
use specta::Type;
use std::path::Path;

/// Why a chosen Java cannot be used. Typed so the UI can say it in the
/// user's language (same reasoning as `error::DatapackRejection`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaUnusableReason {
    /// The file is not there.
    NotFound,
    /// The file could not be read (permissions, a broken link, a directory).
    Unreadable { details: String },
    /// The path does not name a Java executable.
    NotJava,
    /// It would not say what it is: it could not start, exited with an error,
    /// timed out, or answered without its version or architecture.
    ProbeFailed { details: String },
    /// Older than the Minecraft version needs.
    TooOld { found: u32, required: u32 },
    /// Built for another architecture than this launcher, whose natives it
    /// therefore cannot load.
    WrongArch { found: String, expected: String },
}

/// A usable Java's standing against what the Minecraft version needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaVerdict {
    /// The major the version names.
    Matches,
    /// Newer than the version names. Allowed; the UI says so.
    Newer { found: u32, required: u32 },
    /// The instance is not installed, so its need is unknown yet; launch
    /// checks it.
    RequirementUnknown,
}

/// What the UI and the log are told about a usable Java.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct JavaReport {
    /// The executable that is stored and run.
    pub path: String,
    pub version: String,
    pub major: u32,
    pub vendor: Option<String>,
    pub arch: String,
    pub verdict: JavaVerdict,
    /// True where Settings' graphics-card choice does not reach this Java: on
    /// Windows it is a registry entry keyed by the executable, and Lucerna
    /// writes it only for its own runtime (spec §8.3).
    pub gpu_choice_ignored: bool,
}

/// The facts gathered about a stored executable by I/O, before judging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Facts {
    NotFound,
    Unreadable(String),
    /// The probe's combined output, or why it produced none.
    Probed(Result<String, String>),
}

impl Facts {
    /// The facts a stat alone settles: `None` when the file is present and
    /// must be probed.
    pub fn before_probe(e: Existence) -> Option<Facts> {
        match e {
            Existence::Present => None,
            Existence::NotFound => Some(Facts::NotFound),
            Existence::Unreadable(d) => Some(Facts::Unreadable(d)),
        }
    }
}

/// Judge a probed Java. `required = None` means the instance is not installed.
pub fn judge(
    info: &JavaInfo,
    required: Option<u32>,
    host: &Arch,
) -> Result<JavaVerdict, JavaUnusableReason> {
    if let Some(req) = required {
        if info.major < req {
            return Err(JavaUnusableReason::TooOld {
                found: info.major,
                required: req,
            });
        }
    }
    if &info.arch != host {
        return Err(JavaUnusableReason::WrongArch {
            found: info.arch.label(),
            expected: host.label(),
        });
    }
    Ok(match required {
        None => JavaVerdict::RequirementUnknown,
        Some(r) if info.major == r => JavaVerdict::Matches,
        Some(r) => JavaVerdict::Newer {
            found: info.major,
            required: r,
        },
    })
}

/// Everything decided about a custom Java, from facts. Every "could not tell"
/// is a refusal: there is no arm that answers "use it anyway".
pub fn judge_custom(
    exe: &Path,
    facts: Facts,
    required: Option<u32>,
    host: &Arch,
) -> Result<JavaReport, JavaUnusableReason> {
    let output = match facts {
        Facts::NotFound => return Err(JavaUnusableReason::NotFound),
        Facts::Unreadable(details) => return Err(JavaUnusableReason::Unreadable { details }),
        Facts::Probed(Err(details)) => return Err(JavaUnusableReason::ProbeFailed { details }),
        Facts::Probed(Ok(text)) => text,
    };
    let info = parse_probe_output(&output)
        .map_err(|details| JavaUnusableReason::ProbeFailed { details })?;
    let verdict = judge(&info, required, host)?;
    Ok(JavaReport {
        path: exe.to_string_lossy().into_owned(),
        version: info.version,
        major: info.major,
        vendor: info.vendor,
        arch: info.arch.label(),
        verdict,
        gpu_choice_ignored: cfg!(windows),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(major: u32, arch: Arch) -> JavaInfo {
        JavaInfo {
            version: format!("{major}.0.1"),
            major,
            vendor: None,
            arch,
        }
    }

    #[test]
    fn older_than_needed_is_refused() {
        assert_eq!(
            judge(&info(17, Arch::X64), Some(21), &Arch::X64),
            Err(JavaUnusableReason::TooOld {
                found: 17,
                required: 21
            })
        );
    }

    #[test]
    fn another_architecture_is_refused() {
        assert_eq!(
            judge(&info(21, Arch::X86), Some(21), &Arch::X64),
            Err(JavaUnusableReason::WrongArch {
                found: "x86".into(),
                expected: "x64".into()
            })
        );
    }

    #[test]
    fn the_named_major_matches() {
        assert_eq!(
            judge(&info(21, Arch::X64), Some(21), &Arch::X64),
            Ok(JavaVerdict::Matches)
        );
    }

    #[test]
    fn newer_is_allowed_and_said() {
        assert_eq!(
            judge(&info(21, Arch::X64), Some(8), &Arch::X64),
            Ok(JavaVerdict::Newer {
                found: 21,
                required: 8
            })
        );
    }

    #[test]
    fn an_uninstalled_instance_still_checks_the_arch() {
        assert_eq!(
            judge(&info(8, Arch::X64), None, &Arch::X64),
            Ok(JavaVerdict::RequirementUnknown)
        );
        assert!(judge(&info(8, Arch::Aarch64), None, &Arch::X64).is_err());
    }

    #[test]
    fn every_could_not_tell_is_a_refusal() {
        let exe = Path::new("/j/bin/java");
        assert_eq!(
            judge_custom(exe, Facts::NotFound, Some(8), &Arch::X64),
            Err(JavaUnusableReason::NotFound)
        );
        assert_eq!(
            judge_custom(exe, Facts::Unreadable("denied".into()), Some(8), &Arch::X64),
            Err(JavaUnusableReason::Unreadable {
                details: "denied".into()
            })
        );
        assert_eq!(
            judge_custom(
                exe,
                Facts::Probed(Err("timed out".into())),
                Some(8),
                &Arch::X64
            ),
            Err(JavaUnusableReason::ProbeFailed {
                details: "timed out".into()
            })
        );
        assert!(matches!(
            judge_custom(
                exe,
                Facts::Probed(Ok("nonsense".into())),
                Some(8),
                &Arch::X64
            ),
            Err(JavaUnusableReason::ProbeFailed { .. })
        ));
    }

    #[test]
    fn a_good_answer_becomes_a_report() {
        let out =
            "    java.version = 21.0.4\n    java.vendor = Eclipse Adoptium\n    os.arch = amd64\n";
        let r = judge_custom(
            Path::new("/j/bin/java"),
            Facts::Probed(Ok(out.into())),
            Some(21),
            &Arch::X64,
        )
        .unwrap();
        assert_eq!(r.major, 21);
        assert_eq!(r.vendor.as_deref(), Some("Eclipse Adoptium"));
        assert_eq!(r.verdict, JavaVerdict::Matches);
        assert_eq!(r.gpu_choice_ignored, cfg!(windows));
    }
}
