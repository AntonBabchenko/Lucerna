//! `<instance>/lucerna/settings-sync.json`: which kinds this instance syncs,
//! a join still to complete, and the last exit-sync failure.
//!
//! A missing sidecar means "not a member". A sidecar that cannot be read or
//! parsed is an error, never "not a member": treating it as absent would
//! silently stop syncing (or, on a later write, drop the user's choices).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::failure::{self, At, FileReason, SyncFile};
use super::{sidecar_path, Kind};
use crate::error::{Error, Result};

pub const SIDECAR_VERSION: u32 = 1;

/// One kind's membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindState {
    pub enabled: bool,
    #[serde(default)]
    pub join: Join,
}

/// A join still to complete at the next sync point that can apply the kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Join {
    #[default]
    Done,
    /// Take the shared copy (servers / history merge; hotbars replace).
    UseShared,
    /// Hotbars only: this instance's hotbars become the shared ones.
    MakeShared,
}

/// Why a sync did not happen, as the UI says it: the kind that was syncing
/// (`None`: the sync settings themselves), the file (`None`: not a file
/// problem) and a reason class. The raw error is in the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SyncFailure {
    pub kind: Option<Kind>,
    pub file: Option<SyncFile>,
    pub reason: FileReason,
}

/// A sync failure as the launch reports it.
impl From<SyncFailure> for Error {
    fn from(f: SyncFailure) -> Self {
        Error::SettingsSyncFailed {
            sync_kind: f.kind,
            file: f.file,
            reason: f.reason,
        }
    }
}

impl SyncFailure {
    /// The failure of syncing `kind`, from the error that stopped it. The
    /// error's own text (path, OS or parser message) is written to the log
    /// here: it is the only place it is kept.
    pub fn logged(kind: Option<Kind>, e: &crate::error::Error) -> Self {
        crate::diag!(
            "settings sync: {} did not sync ({e})",
            kind.map_or("the sync settings", Kind::file)
        );
        let (file, reason) = super::failure::classify(e);
        Self { kind, file, reason }
    }
}

/// The sidecar as stored. Kinds are kept by name so a kind this build does
/// not know (written by a newer Lucerna) survives a read-modify-write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub version: u32,
    #[serde(default)]
    kinds: BTreeMap<String, KindState>,
    #[serde(default)]
    pub last_error: Option<SyncFailure>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Self {
            version: SIDECAR_VERSION,
            kinds: BTreeMap::new(),
            last_error: None,
        }
    }
}

fn kind_key(kind: Kind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        // Unreachable: `Kind` serializes to a plain snake_case string.
        .unwrap_or_default()
}

impl Sidecar {
    pub fn kind(&self, kind: Kind) -> Option<&KindState> {
        self.kinds.get(&kind_key(kind))
    }

    pub fn set_kind(&mut self, kind: Kind, state: KindState) {
        self.kinds.insert(kind_key(kind), state);
    }

    pub fn enabled(&self, kind: Kind) -> bool {
        self.kind(kind).is_some_and(|k| k.enabled)
    }
}

/// What the sidecar says about membership.
#[derive(Debug, Clone, PartialEq)]
pub enum SidecarRead {
    NotMember,
    Member(Sidecar),
}

pub fn read(instance_root: &Path) -> Result<SidecarRead> {
    let path = sidecar_path(instance_root);
    let Some(bytes) = super::fsio::read_if_present(At::settings(), &path)? else {
        return Ok(SidecarRead::NotMember);
    };
    serde_json::from_slice::<Sidecar>(&bytes)
        .map(SidecarRead::Member)
        .map_err(|e| failure::damaged(At::settings(), &path, format!("parse: {e}")))
}

pub fn write(instance_root: &Path, sidecar: &Sidecar) -> Result<()> {
    let path = sidecar_path(instance_root);
    let json = serde_json::to_vec_pretty(sidecar)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialize: {e}")))?;
    super::fsio::write_atomic(At::settings(), &path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(enabled: bool) -> Sidecar {
        let mut s = Sidecar::default();
        s.set_kind(
            Kind::Servers,
            KindState {
                enabled,
                join: Join::UseShared,
            },
        );
        s
    }

    #[test]
    fn no_sidecar_is_not_a_member() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(read(td.path()).unwrap(), SidecarRead::NotMember);
    }

    #[test]
    fn a_written_sidecar_reads_back() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), &member(true)).unwrap();
        assert_eq!(read(td.path()).unwrap(), SidecarRead::Member(member(true)));
    }

    #[test]
    fn a_garbled_sidecar_is_an_error_not_absence() {
        let td = tempfile::tempdir().unwrap();
        let p = sidecar_path(td.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{ not json").unwrap();
        assert!(read(td.path()).is_err());
    }

    #[test]
    fn a_kind_from_a_newer_lucerna_survives_a_rewrite() {
        let td = tempfile::tempdir().unwrap();
        let p = sidecar_path(td.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            br#"{"version":1,"kinds":{"servers":{"enabled":true},"options":{"enabled":true,"join":"done"}}}"#,
        )
        .unwrap();
        let SidecarRead::Member(mut s) = read(td.path()).unwrap() else {
            panic!("member");
        };
        assert!(s.enabled(Kind::Servers));
        s.set_kind(
            Kind::History,
            KindState {
                enabled: true,
                join: Join::Done,
            },
        );
        write(td.path(), &s).unwrap();
        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"options\""), "{raw}");
    }
}
