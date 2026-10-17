//! A new instance's membership, decided at creation by its origin (spec
//! §B7 [rev] R2): new instances, modpack imports and clients made from a
//! server join when "New instances use shared settings" is on; a clone keeps
//! its source's choice. The first-run seed writes default settings (sync off)
//! together with its instance, so it never joins and has no origin. An instance
//! imported from another launcher is not here on purpose: it brings its own
//! settings (`structural_settings_sync_origins.rs` pins every creation path).

use std::path::{Path, PathBuf};

use super::sidecar::{self, Join, KindState, Sidecar, SidecarRead};
use super::Kind;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    New,
    ModpackImport,
    FromServer,
    Clone { source_root: PathBuf },
}

/// Write the new instance's sidecar when its origin joins. `Ok(true)` when it
/// joined. An `Err` (an unreadable app.json or source sidecar) means it did
/// not join: the caller logs it, the creation itself stands.
pub fn on_created(app_json: &Path, instance_root: &Path, origin: Origin) -> Result<bool> {
    let kinds: Vec<Kind> = match origin {
        Origin::Clone { source_root } => match sidecar::read(&source_root)? {
            SidecarRead::NotMember => return Ok(false),
            SidecarRead::Member(s) => Kind::ALL.into_iter().filter(|k| s.enabled(*k)).collect(),
        },
        Origin::New | Origin::ModpackImport | Origin::FromServer => {
            let app = crate::instances::store::read_app_json(app_json)?;
            if !app.general.settings_sync_new_instances {
                return Ok(false);
            }
            // Every kind: one this version cannot sync yet (hotbars before
            // the game files are installed) shows so and joins when it can.
            Kind::ALL.to_vec()
        }
    };
    if kinds.is_empty() {
        return Ok(false);
    }
    let mut s = Sidecar::default();
    for kind in kinds {
        s.set_kind(
            kind,
            KindState {
                enabled: true,
                join: Join::UseShared,
            },
        );
    }
    sidecar::write(instance_root, &s)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_json(dir: &Path, on: bool) -> PathBuf {
        let p = dir.join("app.json");
        std::fs::write(
            &p,
            format!(r#"{{"general":{{"settings_sync_new_instances":{on}}}}}"#),
        )
        .unwrap();
        p
    }

    fn sidecar_of(root: &Path) -> Option<Sidecar> {
        match sidecar::read(root).unwrap() {
            SidecarRead::Member(s) => Some(s),
            SidecarRead::NotMember => None,
        }
    }

    #[test]
    fn a_new_instance_joins_every_kind_when_the_setting_is_on() {
        let td = tempfile::tempdir().unwrap();
        let inst = td.path().join("inst");
        assert!(on_created(&app_json(td.path(), true), &inst, Origin::New).unwrap());
        let s = sidecar_of(&inst).unwrap();
        for k in Kind::ALL {
            assert_eq!(
                s.kind(k),
                Some(&KindState {
                    enabled: true,
                    join: Join::UseShared
                })
            );
        }
    }

    #[test]
    fn nothing_joins_when_the_setting_is_off() {
        let td = tempfile::tempdir().unwrap();
        let inst = td.path().join("inst");
        assert!(!on_created(&app_json(td.path(), false), &inst, Origin::ModpackImport).unwrap());
        assert_eq!(sidecar_of(&inst), None);
    }

    #[test]
    fn an_unreadable_app_json_does_not_join() {
        let td = tempfile::tempdir().unwrap();
        let inst = td.path().join("inst");
        let p = td.path().join("app.json");
        std::fs::write(&p, b"{").unwrap();
        assert!(on_created(&p, &inst, Origin::New).is_err());
        assert_eq!(sidecar_of(&inst), None);
    }

    #[test]
    fn a_clone_keeps_its_sources_choice_whatever_the_setting() {
        let td = tempfile::tempdir().unwrap();
        let (src, inst) = (td.path().join("src"), td.path().join("inst"));
        let mut s = Sidecar::default();
        s.set_kind(
            Kind::Servers,
            KindState {
                enabled: true,
                join: Join::Done,
            },
        );
        s.set_kind(
            Kind::History,
            KindState {
                enabled: false,
                join: Join::Done,
            },
        );
        sidecar::write(&src, &s).unwrap();
        let joined = on_created(
            &app_json(td.path(), false),
            &inst,
            Origin::Clone {
                source_root: src.clone(),
            },
        )
        .unwrap();
        assert!(joined);
        let c = sidecar_of(&inst).unwrap();
        assert_eq!(
            c.kind(Kind::Servers),
            Some(&KindState {
                enabled: true,
                join: Join::UseShared
            })
        );
        assert!(!c.enabled(Kind::History));
    }

    #[test]
    fn a_clone_of_a_non_member_joins_nothing() {
        let td = tempfile::tempdir().unwrap();
        let (src, inst) = (td.path().join("src"), td.path().join("inst"));
        let joined = on_created(
            &app_json(td.path(), true),
            &inst,
            Origin::Clone { source_root: src },
        )
        .unwrap();
        assert!(!joined);
        assert_eq!(sidecar_of(&inst), None);
    }
}
