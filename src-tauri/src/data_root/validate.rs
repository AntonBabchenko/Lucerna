use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    NotAbsolute,
    NestedInCurrent,
    SameAsCurrent,
    NotEmpty,
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

    fn cur() -> PathBuf {
        PathBuf::from("/data/current")
    }

    #[test]
    fn accepts_empty_absolute_sibling() {
        assert!(validate_target(&cur(), &PathBuf::from("/data/new"), true).is_ok());
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
            validate_target(&cur(), &PathBuf::from("/data/new"), false),
            Err(Invalid::NotEmpty)
        );
    }
}
