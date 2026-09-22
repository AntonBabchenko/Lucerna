use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    NotAbsolute,
    NestedInCurrent,
    SameAsCurrent,
    NotEmpty,
    /// Adopt target does not look like a Lucerna data root (shape check).
    NotADataRoot,
    /// Adopt target failed the write probe; restarting into it would land
    /// straight in the fallback banner.
    NotWritable,
    /// The current data folder contains a symbolic link or junction; moving it
    /// could follow a link out of the tree or loop.
    ContainsLinks,
    /// The launcher is in a recovery session (its data folder is unavailable):
    /// the only change of location it accepts is adopting an existing Lucerna
    /// data folder. A move would copy the throwaway session root.
    FallbackAdoptOnly,
    /// The picked folder is a recovery session's throwaway root (or its
    /// parent): it is deleted when the launcher exits.
    RecoverySessionDir,
}

impl Invalid {
    /// Stable snake_case key for the UI. NEVER expose the raw `Debug` enum
    /// name over IPC — `format-error.ts` maps this key to a human sentence, so
    /// the token must stay stable and translatable.
    pub fn reason_key(&self) -> &'static str {
        match self {
            Invalid::NotAbsolute => "not_absolute",
            Invalid::NestedInCurrent => "nested",
            Invalid::SameAsCurrent => "same",
            Invalid::NotEmpty => "not_empty",
            Invalid::NotADataRoot => "not_a_data_root",
            Invalid::NotWritable => "not_writable",
            Invalid::ContainsLinks => "contains_links",
            Invalid::FallbackAdoptOnly => "fallback_adopt_only",
            Invalid::RecoverySessionDir => "recovery_session_dir",
        }
    }
}

/// Validate a proposed new root against the current root. `target_is_empty` is
/// injected (true when the dir does not exist or exists and is empty).
pub fn validate_target(
    current: &Path,
    target: &Path,
    target_is_empty: bool,
) -> Result<(), Invalid> {
    if !target.is_absolute() {
        return Err(Invalid::NotAbsolute);
    }
    if target == current {
        return Err(Invalid::SameAsCurrent);
    }
    if target.starts_with(current) {
        return Err(Invalid::NestedInCurrent);
    }
    if !target_is_empty {
        return Err(Invalid::NotEmpty);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Absolute on BOTH platforms: `/data/...` is NOT absolute on Windows (it
    // needs a drive letter), which would make every check short-circuit on
    // NotAbsolute and fail these tests on the Windows CI runner.
    fn abs(rel: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:\\{}", rel.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(format!("/{rel}"))
        }
    }

    fn cur() -> PathBuf {
        abs("data/current")
    }

    #[test]
    fn accepts_empty_absolute_sibling() {
        assert!(validate_target(&cur(), &abs("data/new"), true).is_ok());
    }
    #[test]
    fn rejects_relative() {
        assert_eq!(
            validate_target(&cur(), &PathBuf::from("rel"), true),
            Err(Invalid::NotAbsolute)
        );
    }
    #[test]
    fn rejects_same() {
        assert_eq!(
            validate_target(&cur(), &cur(), true),
            Err(Invalid::SameAsCurrent)
        );
    }
    #[test]
    fn rejects_nested() {
        assert_eq!(
            validate_target(&cur(), &cur().join("sub"), true),
            Err(Invalid::NestedInCurrent)
        );
    }
    #[test]
    fn rejects_non_empty() {
        assert_eq!(
            validate_target(&cur(), &abs("data/new"), false),
            Err(Invalid::NotEmpty)
        );
    }

    #[test]
    fn reason_keys_are_stable_snake_case() {
        assert_eq!(Invalid::NotAbsolute.reason_key(), "not_absolute");
        assert_eq!(Invalid::NestedInCurrent.reason_key(), "nested");
        assert_eq!(Invalid::SameAsCurrent.reason_key(), "same");
        assert_eq!(Invalid::NotEmpty.reason_key(), "not_empty");
        assert_eq!(Invalid::NotADataRoot.reason_key(), "not_a_data_root");
        assert_eq!(
            Invalid::FallbackAdoptOnly.reason_key(),
            "fallback_adopt_only"
        );
        assert_eq!(
            Invalid::RecoverySessionDir.reason_key(),
            "recovery_session_dir"
        );
        assert_eq!(Invalid::NotWritable.reason_key(), "not_writable");
        assert_eq!(Invalid::ContainsLinks.reason_key(), "contains_links");
    }
}
