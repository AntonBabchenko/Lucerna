//! Pure decisions for `gpu_pref`: what to write, what to put back. No IO.

use super::record::Entry;
use crate::platform::gpu::fields;

/// The one field Lucerna owns in the `UserGpuPreferences` value.
pub const FIELD: &str = "GpuPreference";

/// What the registry holds for an exe — absent, a value, or "could not tell".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Absent,
    Present(String),
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyPlan {
    /// The field already holds what we would write, and the record says it is ours.
    AlreadyOurs,
    /// Write `value` (the whole value, other fields kept) and record `previous`.
    Write {
        value: String,
        previous: Option<String>,
    },
    /// Writing over a value we could not read would lose it unrecorded.
    RefuseUnreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetirePlan {
    /// Put the value back: write `Some(v)`, or delete when nothing is left.
    Restore { value: Option<String> },
    /// The field is the user's now, or already gone: drop the entry, touch nothing.
    Forget,
    /// Could not read: keep the entry for a later retry.
    Keep(String),
}

pub fn plan_apply(current: Probe, recorded: Option<&Entry>, field: &str) -> ApplyPlan {
    let value = match current {
        Probe::Unreadable(why) => return ApplyPlan::RefuseUnreadable(why),
        Probe::Absent => String::new(),
        Probe::Present(v) => v,
    };
    let cur = fields::get(&value, FIELD).map(str::to_owned);
    if cur.as_deref() == Some(field) && recorded.is_some_and(|e| e.written == field) {
        return ApplyPlan::AlreadyOurs;
    }
    // Still what we wrote → the older "before" stands; anything else is the
    // user's (or nothing) and becomes the new "before".
    let previous = match recorded {
        Some(e) if cur.as_deref() == Some(e.written.as_str()) => e.previous.clone(),
        _ => cur,
    };
    ApplyPlan::Write {
        value: fields::set(&value, FIELD, field),
        previous,
    }
}

pub fn plan_retire(current: Probe, entry: &Entry) -> RetirePlan {
    let value = match current {
        Probe::Unreadable(why) => return RetirePlan::Keep(why),
        Probe::Absent => return RetirePlan::Forget,
        Probe::Present(v) => v,
    };
    if fields::get(&value, FIELD) != Some(entry.written.as_str()) {
        return RetirePlan::Forget;
    }
    let restored = match &entry.previous {
        Some(prev) => fields::set(&value, FIELD, prev),
        None => fields::remove(&value, FIELD),
    };
    RetirePlan::Restore {
        value: (!restored.is_empty()).then_some(restored),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(written: &str, previous: Option<&str>) -> Entry {
        Entry {
            exe: "C:/x/javaw.exe".into(),
            written: written.into(),
            previous: previous.map(str::to_owned),
        }
    }
    fn present(v: &str) -> Probe {
        Probe::Present(v.into())
    }

    #[test]
    fn apply_refuses_an_unreadable_value() {
        assert_eq!(
            plan_apply(Probe::Unreadable("denied".into()), None, "2"),
            ApplyPlan::RefuseUnreadable("denied".into())
        );
    }

    #[test]
    fn apply_skips_a_field_that_is_already_ours() {
        assert_eq!(
            plan_apply(
                present("GpuPreference=2;X=1;"),
                Some(&entry("2", None)),
                "2"
            ),
            ApplyPlan::AlreadyOurs
        );
    }

    #[test]
    fn apply_records_a_foreign_field_as_previous_and_keeps_the_rest() {
        assert_eq!(
            plan_apply(
                present("GpuPreference=1;SwapEffectUpgradeEnable=1;"),
                None,
                "2"
            ),
            ApplyPlan::Write {
                value: "GpuPreference=2;SwapEffectUpgradeEnable=1;".into(),
                previous: Some("1".into()),
            }
        );
    }

    #[test]
    fn apply_keeps_the_older_previous_while_the_field_is_still_ours() {
        // We wrote "2" over the user's "1"; the user now picks Power saving.
        assert_eq!(
            plan_apply(
                present("GpuPreference=2;"),
                Some(&entry("2", Some("1"))),
                "1"
            ),
            ApplyPlan::Write {
                value: "GpuPreference=1;".into(),
                previous: Some("1".into()),
            }
        );
    }

    #[test]
    fn apply_on_an_absent_value_or_field_records_no_previous() {
        assert_eq!(
            plan_apply(Probe::Absent, None, "2"),
            ApplyPlan::Write {
                value: "GpuPreference=2;".into(),
                previous: None,
            }
        );
        assert_eq!(
            plan_apply(present("SwapEffectUpgradeEnable=1;"), None, "2"),
            ApplyPlan::Write {
                value: "SwapEffectUpgradeEnable=1;GpuPreference=2;".into(),
                previous: None,
            }
        );
    }

    #[test]
    fn retire_keeps_the_entry_when_the_value_cannot_be_read() {
        assert_eq!(
            plan_retire(Probe::Unreadable("denied".into()), &entry("2", None)),
            RetirePlan::Keep("denied".into())
        );
    }

    #[test]
    fn retire_restores_the_previous_field_and_keeps_the_rest() {
        assert_eq!(
            plan_retire(
                present("GpuPreference=2;SwapEffectUpgradeEnable=1;"),
                &entry("2", Some("1"))
            ),
            RetirePlan::Restore {
                value: Some("GpuPreference=1;SwapEffectUpgradeEnable=1;".into())
            }
        );
    }

    #[test]
    fn retire_removes_the_field_and_deletes_an_empty_value() {
        assert_eq!(
            plan_retire(
                present("GpuPreference=2;SwapEffectUpgradeEnable=1;"),
                &entry("2", None)
            ),
            RetirePlan::Restore {
                value: Some("SwapEffectUpgradeEnable=1;".into())
            }
        );
        assert_eq!(
            plan_retire(present("GpuPreference=2;"), &entry("2", None)),
            RetirePlan::Restore { value: None }
        );
    }

    #[test]
    fn retire_forgets_what_is_not_ours_or_gone() {
        assert_eq!(
            plan_retire(present("GpuPreference=1;"), &entry("2", None)),
            RetirePlan::Forget
        );
        assert_eq!(
            plan_retire(present("X=1;"), &entry("2", None)),
            RetirePlan::Forget
        );
        assert_eq!(
            plan_retire(Probe::Absent, &entry("2", None)),
            RetirePlan::Forget
        );
    }
}
