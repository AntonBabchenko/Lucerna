//! Pure in-memory mutations on an [`AccountFile`]. The `&AppHandle`-bound
//! commands in [`super`] are thin read → mutate → write wrappers around these,
//! so the orchestration rules — active-id reassignment on remove, offline
//! idempotency, first-account auto-activation, set-active validation — are
//! unit-testable without a Tauri runtime (the file IO and keychain cleanup stay
//! in the wrappers).

use crate::accounts::offline::derive_offline_uuid;
use crate::accounts::store::{Account, AccountFile, AccountKind};
use crate::error::{Error, Result};

/// Set the active account by id. `Err(Error::AccountNotSet)` if no such id
/// exists; leaves `active_id` untouched in that case.
pub fn set_active(file: &mut AccountFile, id: &str) -> Result<()> {
    if !file.accounts.iter().any(|a| a.id == id) {
        return Err(Error::AccountNotSet);
    }
    file.active_id = Some(id.to_string());
    Ok(())
}

/// Remove the account with `id`. If it was the active account, the first
/// remaining account becomes active; if none remain, `active_id` becomes
/// `None`. Removing an unknown id is a no-op (active selection unchanged).
pub fn remove(file: &mut AccountFile, id: &str) {
    let was_active = file.active_id.as_deref() == Some(id);
    file.accounts.retain(|a| a.id != id);
    if was_active {
        file.active_id = file.accounts.first().map(|a| a.id.clone());
    }
}

/// Add an offline account whose uuid is deterministically derived from `name`.
/// Idempotent: an existing account with the same uuid is returned unchanged
/// (no duplicate, active selection untouched). The first account ever added
/// auto-activates.
pub fn add_offline(file: &mut AccountFile, name: &str) -> Account {
    let uuid = derive_offline_uuid(name).to_string();
    if let Some(existing) = file.accounts.iter().find(|a| a.uuid == uuid).cloned() {
        return existing;
    }
    let account = Account {
        id: format!("of-{}", uuid::Uuid::new_v4()),
        kind: AccountKind::Offline,
        name: name.to_string(),
        uuid,
        expires_at: None,
    };
    file.accounts.push(account.clone());
    if file.active_id.is_none() {
        file.active_id = Some(account.id.clone());
    }
    account
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline(id: &str, uuid: &str) -> Account {
        Account {
            id: id.into(),
            kind: AccountKind::Offline,
            name: format!("name-{uuid}"),
            uuid: uuid.into(),
            expires_at: None,
        }
    }

    fn file_with(accounts: Vec<Account>, active: Option<&str>) -> AccountFile {
        // `..default()` fills `version` (the migration discriminator) so the
        // test stays correct if the current schema version bumps.
        AccountFile {
            accounts,
            active_id: active.map(str::to_string),
            ..AccountFile::default()
        }
    }

    // --- set_active ---

    #[test]
    fn set_active_to_existing_id_succeeds() {
        let mut f = file_with(vec![offline("a", "u1"), offline("b", "u2")], Some("a"));
        set_active(&mut f, "b").unwrap();
        assert_eq!(f.active_id.as_deref(), Some("b"));
    }

    #[test]
    fn set_active_to_unknown_id_errors_and_leaves_active_unchanged() {
        let mut f = file_with(vec![offline("a", "u1")], Some("a"));
        let err = set_active(&mut f, "ghost").unwrap_err();
        assert!(matches!(err, Error::AccountNotSet));
        assert_eq!(f.active_id.as_deref(), Some("a"));
    }

    // --- remove ---

    #[test]
    fn remove_active_account_promotes_first_remaining() {
        let mut f = file_with(
            vec![offline("a", "u1"), offline("b", "u2"), offline("c", "u3")],
            Some("a"),
        );
        remove(&mut f, "a");
        assert_eq!(f.accounts.len(), 2);
        // First remaining (b) becomes active.
        assert_eq!(f.active_id.as_deref(), Some("b"));
    }

    #[test]
    fn remove_non_active_account_leaves_active_unchanged() {
        let mut f = file_with(vec![offline("a", "u1"), offline("b", "u2")], Some("a"));
        remove(&mut f, "b");
        assert_eq!(f.accounts.len(), 1);
        assert_eq!(f.active_id.as_deref(), Some("a"));
    }

    #[test]
    fn remove_last_account_clears_active() {
        let mut f = file_with(vec![offline("a", "u1")], Some("a"));
        remove(&mut f, "a");
        assert!(f.accounts.is_empty());
        assert_eq!(f.active_id, None);
    }

    #[test]
    fn remove_unknown_id_is_a_noop() {
        let mut f = file_with(vec![offline("a", "u1")], Some("a"));
        remove(&mut f, "ghost");
        assert_eq!(f.accounts.len(), 1);
        assert_eq!(f.active_id.as_deref(), Some("a"));
    }

    // --- add_offline ---

    #[test]
    fn add_offline_to_empty_file_pushes_and_auto_activates() {
        let mut f = file_with(vec![], None);
        let acc = add_offline(&mut f, "Steve");
        assert_eq!(f.accounts.len(), 1);
        assert_eq!(acc.kind, AccountKind::Offline);
        assert_eq!(acc.name, "Steve");
        // First account ever added becomes active.
        assert_eq!(f.active_id.as_deref(), Some(acc.id.as_str()));
    }

    #[test]
    fn add_offline_does_not_steal_active_from_an_existing_account() {
        let mut f = file_with(vec![offline("a", "u1")], Some("a"));
        let acc = add_offline(&mut f, "Steve");
        assert_eq!(f.accounts.len(), 2);
        // Active selection is preserved — only the FIRST add auto-activates.
        assert_eq!(f.active_id.as_deref(), Some("a"));
        assert_ne!(acc.id, "a");
    }

    #[test]
    fn add_offline_is_idempotent_on_the_same_name() {
        let mut f = file_with(vec![], None);
        let first = add_offline(&mut f, "Steve");
        let again = add_offline(&mut f, "Steve");
        // Same name → same derived uuid → existing entry returned, no duplicate.
        assert_eq!(f.accounts.len(), 1);
        assert_eq!(first.id, again.id);
        assert_eq!(first.uuid, again.uuid);
    }

    #[test]
    fn add_offline_same_name_returns_existing_even_with_different_active() {
        let mut f = file_with(vec![], None);
        let first = add_offline(&mut f, "Steve");
        let second = add_offline(&mut f, "Alex");
        // Two distinct accounts; first stayed active.
        assert_eq!(f.accounts.len(), 2);
        assert_ne!(first.uuid, second.uuid);
        assert_eq!(f.active_id.as_deref(), Some(first.id.as_str()));
    }
}
