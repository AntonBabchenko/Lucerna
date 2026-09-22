//! GPU preference → OS store, done properly: "Automatic" touches nothing; an
//! explicit choice writes ONE field of the per-exe value and records what it
//! replaced; switching back to Automatic restores exactly that, only where the
//! field is still Lucerna's. Spec:
//! docs/superpowers/specs/2026-09-22-gpu-automatic-leaves-os-alone-design.md

pub mod plan;
pub mod record;

use crate::instances::schema::GpuPreference;
use crate::platform::gpu::{gpu_pref_field, GpuRegistry};
use plan::Probe;
use record::{Read, Record};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// One writer at a time across the registry AND the record: a launch's
/// `apply` and a save's `retire_all` are both async commands and would
/// otherwise interleave (retire drops the entry, apply re-writes the field —
/// a value with no record). Never held across an await.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> io::Result<MutexGuard<'static, ()>> {
    LOCK.lock().map_err(|_| {
        io::Error::new(
            io::ErrorKind::Other,
            "the GPU preference lock is poisoned — restart Lucerna",
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    Written,
    AlreadyOurs,
    /// The value could not be read; nothing was written or recorded.
    Refused(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Retired {
    pub restored: usize,
    pub forgotten: usize,
    pub kept: usize,
}

/// Beside the data-location pointer: per user and per machine, like the hive.
pub fn record_path(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(crate::paths::default_app_data_dir(app)?.join(record::FILE))
}

/// `Some(true)` when the record has entries, `Some(false)` when absent or
/// empty, `None` when it cannot be read (the caller decides what that means).
pub fn has_entries(record_path: &Path) -> Option<bool> {
    match record::read(record_path) {
        Ok(Read::Absent) => Some(false),
        Ok(Read::Present(r)) => Some(!r.entries.is_empty()),
        Err(_) => None,
    }
}

fn probe(registry: &dyn GpuRegistry, exe: &Path) -> Probe {
    match registry.read(exe) {
        Ok(None) => Probe::Absent,
        Ok(Some(v)) => Probe::Present(v),
        Err(e) => Probe::Unreadable(e.to_string()),
    }
}

/// Absent = empty; corrupt = `Err` — nothing may be touched on the strength
/// of a record that cannot be read.
fn load(record_path: &Path) -> io::Result<Record> {
    Ok(match record::read(record_path)? {
        Read::Absent => Record::default(),
        Read::Present(r) => r,
    })
}

/// Apply `pref` to `exe`. `Auto` returns without touching the registry or
/// the record. Otherwise: read → plan → write the registry → record it.
pub fn apply(
    registry: &dyn GpuRegistry,
    record_path: &Path,
    exe: &Path,
    pref: GpuPreference,
) -> io::Result<Applied> {
    // RED STUB (push 1): reads the registry whatever the preference, writes
    // nothing, records nothing.
    let _ = (gpu_pref_field(pref), lock()?, load(record_path)?);
    let _ = probe(registry, exe);
    Ok(Applied::AlreadyOurs)
}

/// Put back what Lucerna replaced for every recorded exe whose field is still
/// Lucerna's; forget entries that are the user's now or gone; keep the ones
/// that could not be read or restored (retried on a later save).
pub fn retire_all(registry: &dyn GpuRegistry, record_path: &Path) -> io::Result<Retired> {
    // RED STUB (push 1): reads every recorded exe and restores nothing.
    let _guard = lock()?;
    let rec = load(record_path)?;
    for entry in &rec.entries {
        let _ = probe(registry, Path::new(&entry.exe));
    }
    Ok(Retired::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        values: RefCell<HashMap<PathBuf, String>>,
        calls: RefCell<usize>,
        unreadable: bool,
    }
    impl GpuRegistry for Fake {
        fn read(&self, exe: &Path) -> io::Result<Option<String>> {
            *self.calls.borrow_mut() += 1;
            if self.unreadable {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
            }
            Ok(self.values.borrow().get(exe).cloned())
        }
        fn write(&self, exe: &Path, value: &str) -> io::Result<()> {
            *self.calls.borrow_mut() += 1;
            self.values
                .borrow_mut()
                .insert(exe.to_path_buf(), value.to_owned());
            Ok(())
        }
        fn delete(&self, exe: &Path) -> io::Result<()> {
            *self.calls.borrow_mut() += 1;
            self.values.borrow_mut().remove(exe);
            Ok(())
        }
    }
    fn exe() -> PathBuf {
        PathBuf::from("C:/x/jres/17/bin/javaw.exe")
    }
    fn temp_record() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(record::FILE);
        (d, p)
    }
    fn value(fake: &Fake) -> Option<String> {
        fake.values.borrow().get(&exe()).cloned()
    }
    fn record_is_absent(p: &Path) -> bool {
        matches!(std::fs::metadata(p), Err(e) if e.kind() == io::ErrorKind::NotFound)
    }
    fn with_entry(rec: &Path, written: &str, previous: Option<&str>) {
        let mut r = record::Record::default();
        r.upsert(record::Entry {
            exe: exe().to_string_lossy().into_owned(),
            written: written.into(),
            previous: previous.map(str::to_owned),
        });
        record::write(rec, &r).unwrap();
    }

    #[test]
    fn automatic_touches_neither_the_registry_nor_the_record() {
        let (_d, rec) = temp_record();
        let fake = Fake::default();
        // A choice the user made in Windows Settings for Lucerna's javaw.
        fake.values
            .borrow_mut()
            .insert(exe(), "GpuPreference=2;".into());
        assert_eq!(
            apply(&fake, &rec, &exe(), GpuPreference::Auto).unwrap(),
            Applied::AlreadyOurs
        );
        assert_eq!(*fake.calls.borrow(), 0);
        assert!(record_is_absent(&rec));
        assert_eq!(value(&fake).as_deref(), Some("GpuPreference=2;"));
    }

    #[test]
    fn retire_with_no_record_touches_nothing() {
        let (_d, rec) = temp_record();
        let fake = Fake::default();
        fake.values
            .borrow_mut()
            .insert(exe(), "GpuPreference=2;".into());
        assert_eq!(retire_all(&fake, &rec).unwrap(), Retired::default());
        assert_eq!(*fake.calls.borrow(), 0);
        assert_eq!(value(&fake).as_deref(), Some("GpuPreference=2;"));
    }

    #[test]
    fn retire_forgets_a_field_that_is_not_ours_and_leaves_it_alone() {
        let (_d, rec) = temp_record();
        with_entry(&rec, "2", None);
        let fake = Fake::default();
        // The user changed it in Windows Settings since.
        fake.values
            .borrow_mut()
            .insert(exe(), "GpuPreference=1;".into());
        let out = retire_all(&fake, &rec).unwrap();
        assert_eq!(
            out,
            Retired {
                restored: 0,
                forgotten: 1,
                kept: 0
            }
        );
        assert_eq!(value(&fake).as_deref(), Some("GpuPreference=1;"));
        assert_eq!(has_entries(&rec), Some(false));
    }

    #[test]
    fn apply_then_retire_puts_the_previous_field_back_and_keeps_the_rest() {
        let (_d, rec) = temp_record();
        let fake = Fake::default();
        fake.values
            .borrow_mut()
            .insert(exe(), "GpuPreference=1;SwapEffectUpgradeEnable=1;".into());
        assert_eq!(
            apply(&fake, &rec, &exe(), GpuPreference::HighPerformance).unwrap(),
            Applied::Written
        );
        assert_eq!(
            value(&fake).as_deref(),
            Some("GpuPreference=2;SwapEffectUpgradeEnable=1;")
        );
        // Idempotent: a second apply is AlreadyOurs — one read, no write.
        let before = *fake.calls.borrow();
        assert_eq!(
            apply(&fake, &rec, &exe(), GpuPreference::HighPerformance).unwrap(),
            Applied::AlreadyOurs
        );
        assert_eq!(*fake.calls.borrow(), before + 1);
        let out = retire_all(&fake, &rec).unwrap();
        assert_eq!(
            out,
            Retired {
                restored: 1,
                forgotten: 0,
                kept: 0
            }
        );
        assert_eq!(
            value(&fake).as_deref(),
            Some("GpuPreference=1;SwapEffectUpgradeEnable=1;")
        );
        assert_eq!(has_entries(&rec), Some(false));
    }

    #[test]
    fn apply_on_an_absent_value_then_retire_deletes_it() {
        let (_d, rec) = temp_record();
        let fake = Fake::default();
        assert_eq!(
            apply(&fake, &rec, &exe(), GpuPreference::PowerSaving).unwrap(),
            Applied::Written
        );
        assert_eq!(value(&fake).as_deref(), Some("GpuPreference=1;"));
        retire_all(&fake, &rec).unwrap();
        assert_eq!(value(&fake), None);
    }

    #[test]
    fn a_corrupt_record_refuses_both_and_touches_nothing() {
        let (_d, rec) = temp_record();
        std::fs::write(&rec, b"{").unwrap();
        let fake = Fake::default();
        fake.values
            .borrow_mut()
            .insert(exe(), "GpuPreference=1;".into());
        assert!(apply(&fake, &rec, &exe(), GpuPreference::HighPerformance).is_err());
        assert!(retire_all(&fake, &rec).is_err());
        assert_eq!(*fake.calls.borrow(), 0);
        assert_eq!(value(&fake).as_deref(), Some("GpuPreference=1;"));
    }

    #[test]
    fn an_unreadable_value_is_refused_and_not_recorded() {
        let (_d, rec) = temp_record();
        let fake = Fake {
            unreadable: true,
            ..Fake::default()
        };
        assert_eq!(
            apply(&fake, &rec, &exe(), GpuPreference::HighPerformance).unwrap(),
            Applied::Refused("denied".into())
        );
        assert!(record_is_absent(&rec));
    }

    #[test]
    fn two_exes_keep_two_entries() {
        let (_d, rec) = temp_record();
        let fake = Fake::default();
        let other = PathBuf::from("C:/x/jres/21/bin/javaw.exe");
        apply(&fake, &rec, &exe(), GpuPreference::HighPerformance).unwrap();
        apply(&fake, &rec, &other, GpuPreference::HighPerformance).unwrap();
        let record::Read::Present(r) = record::read(&rec).unwrap() else {
            panic!("expected a record");
        };
        assert_eq!(r.entries.len(), 2);
        assert_eq!(retire_all(&fake, &rec).unwrap().restored, 2);
        assert_eq!(fake.values.borrow().len(), 0);
    }
}
