//! Resolve a bare loader mod-id (e.g. `balm`) to an installable platform
//! project, and verify a downloaded candidate actually provides that id.
//! The platform calls are injected so the orchestration is unit-testable.

/// Normalize a loader mod-id into candidate platform slugs to try, most-likely
/// first: the id itself, then `_`<->`-` swaps. Lowercased, de-duplicated, order
/// preserved.
pub fn slug_candidates(dep_id: &str) -> Vec<String> {
    let base = dep_id.trim().to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    push(base.clone());
    push(base.replace('_', "-"));
    push(base.replace('-', "_"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_candidates_covers_underscore_and_hyphen_forms() {
        assert_eq!(slug_candidates("balm"), vec!["balm"]);
        assert_eq!(
            slug_candidates("forge_config_api_port"),
            vec!["forge_config_api_port", "forge-config-api-port"]
        );
        assert_eq!(
            slug_candidates("Cloth-Config"),
            vec!["cloth-config", "cloth_config"]
        );
        assert!(slug_candidates("   ").is_empty());
    }
}
