//! Resolve a bare loader mod-id (e.g. `balm`) to an installable platform
//! project, and verify a downloaded candidate actually provides that id.
//! The platform calls are injected so the orchestration is unit-testable.

use crate::mods::local::{read_jar_embedded_providers, read_jar_manifest_deps};

/// Case- and `_`/`-`-insensitive id normalization for cross-source matching.
fn norm_id(id: &str) -> String {
    id.trim().to_ascii_lowercase().replace('-', "_")
}

/// True iff `jar_bytes` declares `dep_id` among the mod-ids it provides — its
/// own `[[mods]]` / fabric / quilt id, a `provides` alias, or a JIJ submodule.
/// Best-effort: an unreadable jar yields `false`.
pub fn jar_provides(jar_bytes: &[u8], dep_id: &str) -> bool {
    let wanted = norm_id(dep_id);
    let Ok(manifest) = read_jar_manifest_deps(jar_bytes) else {
        return false;
    };
    manifest
        .provided
        .iter()
        .map(|p| p.mod_id.clone())
        .chain(
            read_jar_embedded_providers(jar_bytes)
                .into_iter()
                .map(|p| p.mod_id),
        )
        .any(|id| norm_id(&id) == wanted)
}

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
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn jar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

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

    #[test]
    fn jar_provides_matches_own_id_case_and_separator_insensitive() {
        let balm = jar(&[(
            "META-INF/neoforge.mods.toml",
            b"[[mods]]\nmodId=\"balm\"\nversion=\"9.0.0\"\n",
        )]);
        assert!(jar_provides(&balm, "balm"));
        assert!(jar_provides(&balm, "BALM"));
        assert!(!jar_provides(&balm, "waystones"));
    }

    #[test]
    fn jar_provides_matches_jij_submodule() {
        let inner = jar(&[(
            "fabric.mod.json",
            br#"{"id":"cloth-config","version":"1.0"}"#,
        )]);
        let outer = jar(&[
            ("fabric.mod.json", br#"{"id":"somelib","version":"1.0"}"#),
            ("META-INF/jars/cloth-config.jar", &inner),
        ]);
        assert!(jar_provides(&outer, "cloth_config"));
    }

    #[test]
    fn jar_provides_false_on_unreadable_jar() {
        assert!(!jar_provides(b"not a zip", "anything"));
    }
}
