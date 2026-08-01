//! Forge / NeoForge mod-descriptor reader (`META-INF/mods.toml`,
//! `META-INF/neoforge.mods.toml`).
//!
//! The primary path is a real TOML parse. `toml` is already in the runtime
//! dependency tree via `tauri-utils` / `tauri-plugin-fs`, so declaring it
//! directly adds no crate to the binary — it is a version unification.
//! Descriptors that are not valid TOML (an unescaped `\` in a Windows path is
//! the realistic authoring mistake) fall back to the hardened regex scanner in
//! [`crate::mods::local`], which recovers what it can rather than losing every
//! dependency of that jar.
//!
//! ## Key precedence is descriptor-aware, because the loaders differ
//!
//! * `META-INF/neoforge.mods.toml` — FancyModLoader 1.21.1 `ModInfo.java:218`
//!   reads **only** `type`, uppercases it before `DependencyType::valueOf`, and
//!   defaults to `REQUIRED` when the key is absent. `mandatory` is never read.
//! * `META-INF/mods.toml` — MinecraftForge `ModInfo.java:211-217` (every branch
//!   through 26.2) reads **only** `mandatory`, and treats it as a required
//!   field. NeoForge ≤ 1.20.4 also used this filename and read `type` first
//!   with a `mandatory` fallback, so `type` applies here only when `mandatory`
//!   is absent.
//!
//! The `required = <bool>` key that a few jars carry (owo-lib, biomes-o-plenty,
//! terrablender) is read by **no** loader. We deliberately ignore it rather
//! than diverge from FML: honouring a `required = false` would silently drop a
//! dependency the loader still enforces.

use crate::mods::local::{DeclaredDep, DepSide, DependencyKind, ProvidedMod};
use crate::mods::version_range::RangeFamily;

/// Which descriptor file the text came from — decides key precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Descriptor {
    /// `META-INF/mods.toml` — MinecraftForge, and NeoForge ≤ 1.20.4.
    LegacyForge,
    /// `META-INF/neoforge.mods.toml` — NeoForge ≥ 1.20.6.
    NeoForge,
}

/// What one descriptor declares. Versions are raw — `${file.jarVersion}` is
/// resolved by the caller, which is the only side that can read `MANIFEST.MF`.
#[derive(Debug, Default, PartialEq)]
pub struct ForgeDescriptor {
    pub provided: Vec<ProvidedMod>,
    pub deps: Vec<DeclaredDep>,
}

/// Map a declared `type` value onto a kind. FML uppercases then calls
/// `DependencyType::valueOf`, so matching is case-insensitive; an unrecognised
/// value makes the jar invalid for the loader outright, and `Required` is the
/// closest reading of "this jar will not load as it stands".
pub(crate) fn kind_from_type(raw: &str) -> DependencyKind {
    match raw.trim().to_ascii_lowercase().as_str() {
        "optional" => DependencyKind::Optional,
        "incompatible" => DependencyKind::Incompatible,
        "discouraged" => DependencyKind::Discouraged,
        _ => DependencyKind::Required,
    }
}

fn kind_of(tbl: &toml::Table, descriptor: Descriptor) -> DependencyKind {
    let ty = tbl.get("type").and_then(|v| v.as_str());
    match descriptor {
        // NeoForge ≥ 1.20.6: `type` only, default REQUIRED.
        Descriptor::NeoForge => ty.map_or(DependencyKind::Required, kind_from_type),
        // MinecraftForge: `mandatory` only. `type` is the NeoForge ≤ 1.20.4
        // fallback for jars that shipped it under the legacy filename.
        Descriptor::LegacyForge => match (tbl.get("mandatory").and_then(|v| v.as_bool()), ty) {
            (Some(true), _) => DependencyKind::Required,
            (Some(false), _) => DependencyKind::Optional,
            (None, Some(t)) => kind_from_type(t),
            (None, None) => DependencyKind::Required,
        },
    }
}

fn side_of(tbl: &toml::Table) -> DepSide {
    match tbl
        .get("side")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_uppercase())
        .as_deref()
    {
        Some("CLIENT") => DepSide::Client,
        Some("SERVER") => DepSide::Server,
        _ => DepSide::Both,
    }
}

/// `[[mods]]` is an array of tables, `mods = [{…}]` is the same thing inline,
/// and `dependencies.<owner>` is legally either an array of tables or a single
/// table. Normalise all four shapes to a list of tables.
fn as_tables(v: &toml::Value) -> Vec<&toml::Table> {
    match v {
        toml::Value::Table(t) => vec![t],
        toml::Value::Array(a) => a.iter().filter_map(|x| x.as_table()).collect(),
        _ => Vec::new(),
    }
}

/// Parse a descriptor. `None` means "not valid TOML" — the caller falls back to
/// the regex scanner. A valid descriptor that simply declares nothing yields an
/// empty [`ForgeDescriptor`], which is a different (and correct) outcome.
pub fn parse(text: &str, descriptor: Descriptor) -> Option<ForgeDescriptor> {
    let root: toml::Table = text.parse().ok()?;
    let mut out = ForgeDescriptor::default();

    if let Some(mods) = root.get("mods") {
        for m in as_tables(mods) {
            let Some(mod_id) = m.get("modId").and_then(|v| v.as_str()) else {
                continue;
            };
            out.provided.push(ProvidedMod {
                mod_id: mod_id.to_string(),
                version: m
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
    }

    if let Some(toml::Value::Table(owners)) = root.get("dependencies") {
        for owner in owners.values() {
            for d in as_tables(owner) {
                let Some(dep_id) = d.get("modId").and_then(|v| v.as_str()) else {
                    continue;
                };
                out.deps.push(DeclaredDep {
                    dep_id: dep_id.to_string(),
                    // An omitted versionRange is FML's UNBOUNDED
                    // (`ModInfo.java:221-223`), i.e. any version — which is
                    // exactly how an empty range string is evaluated.
                    range: d
                        .get("versionRange")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    kind: kind_of(d, descriptor),
                    side: side_of(d),
                    family: RangeFamily::Maven,
                });
            }
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_descriptor_reads_type_and_ignores_commented_mandatory() {
        // The AsyncParticles shape: a live `incompatible` block followed by a
        // commented-out block carrying `#mandatory = true`. A TOML parser sees
        // the comment for what it is.
        let txt = r#"
[[mods]]
modId = "asyncparticles"
version = "21.1.0b-alpha.2"

[[dependencies.asyncparticles]]
modId = "create"
type = "incompatible"
versionRange = "(,6.0.9]"
side = "CLIENT"

#[[dependencies.asyncparticles]]
#modId = "mixinextras"
#mandatory = true
"#;
        let d = parse(txt, Descriptor::NeoForge).expect("valid toml");
        assert_eq!(d.deps.len(), 1);
        assert_eq!(d.deps[0].dep_id, "create");
        assert_eq!(d.deps[0].kind, DependencyKind::Incompatible);
        assert_eq!(d.deps[0].range, "(,6.0.9]");
        assert_eq!(d.deps[0].side, DepSide::Client);
        assert_eq!(d.provided[0].version.as_deref(), Some("21.1.0b-alpha.2"));
    }

    #[test]
    fn legacy_mods_toml_lets_mandatory_win_over_type() {
        // MinecraftForge reads ONLY `mandatory`, so a pasted NeoForge `type`
        // must not downgrade a dependency the loader still enforces.
        let txt = r#"
[[dependencies.examplemod]]
modId = "curios"
mandatory = true
type = "optional"
versionRange = "[1.20.1-5.4.0,)"
"#;
        let d = parse(txt, Descriptor::LegacyForge).expect("valid toml");
        assert_eq!(d.deps[0].kind, DependencyKind::Required);
    }

    #[test]
    fn legacy_mods_toml_falls_back_to_type_when_mandatory_is_absent() {
        // NeoForge ≤ 1.20.4 shipped `type` under the legacy filename.
        let txt = "[[dependencies.m]]\nmodId=\"curios\"\ntype=\"optional\"\n";
        let d = parse(txt, Descriptor::LegacyForge).expect("valid toml");
        assert_eq!(d.deps[0].kind, DependencyKind::Optional);
    }

    #[test]
    fn neoforge_descriptor_ignores_mandatory_entirely() {
        // FML 1.21.1 `ModInfo.java:218-219` reads only `type`; absent => REQUIRED.
        let txt = "[[dependencies.m]]\nmodId=\"curios\"\nmandatory=false\n";
        let d = parse(txt, Descriptor::NeoForge).expect("valid toml");
        assert_eq!(d.deps[0].kind, DependencyKind::Required);
    }

    #[test]
    fn required_key_is_ignored_because_no_loader_reads_it() {
        let txt = "[[dependencies.m]]\nmodId=\"curios\"\nrequired=false\n";
        let d = parse(txt, Descriptor::NeoForge).expect("valid toml");
        assert_eq!(d.deps[0].kind, DependencyKind::Required);
    }

    #[test]
    fn type_value_is_case_insensitive_and_unknown_falls_back_to_required() {
        let up = parse(
            "[[dependencies.m]]\nmodId=\"a\"\ntype=\"REQUIRED\"\n",
            Descriptor::NeoForge,
        )
        .unwrap();
        assert_eq!(up.deps[0].kind, DependencyKind::Required);
        let junk = parse(
            "[[dependencies.m]]\nmodId=\"a\"\ntype=\"whatever\"\n",
            Descriptor::NeoForge,
        )
        .unwrap();
        assert_eq!(junk.deps[0].kind, DependencyKind::Required);
    }

    #[test]
    fn discouraged_is_recognised() {
        let d = parse(
            "[[dependencies.m]]\nmodId=\"a\"\ntype=\"discouraged\"\n",
            Descriptor::NeoForge,
        )
        .unwrap();
        assert_eq!(d.deps[0].kind, DependencyKind::Discouraged);
    }

    #[test]
    fn dependencies_owner_may_be_a_single_table_not_an_array() {
        let txt = "[dependencies]\nexamplemod = { modId = \"curios\", type = \"optional\" }\n";
        let d = parse(txt, Descriptor::NeoForge).expect("valid toml");
        assert_eq!(d.deps.len(), 1);
        assert_eq!(d.deps[0].kind, DependencyKind::Optional);
    }

    #[test]
    fn single_quoted_inline_mods_array_is_read() {
        // veinminer's shape — invisible to every double-quote regex.
        let txt = "mods = [{ modId = 'mr_veinminer', version = '1.1.0' }]\n";
        let d = parse(txt, Descriptor::NeoForge).expect("valid toml");
        assert_eq!(d.provided.len(), 1);
        assert_eq!(d.provided[0].mod_id, "mr_veinminer");
        assert_eq!(d.provided[0].version.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn missing_version_range_is_empty_which_evaluates_as_any() {
        let d = parse("[[dependencies.m]]\nmodId=\"a\"\n", Descriptor::NeoForge).unwrap();
        assert_eq!(d.deps[0].range, "");
    }

    #[test]
    fn hash_inside_a_quoted_value_is_not_a_comment() {
        let txt = "[[dependencies.m]]\nmodId=\"a\"\nversionRange=\"[#PLACEHOLDER#,)\"\n";
        let d = parse(txt, Descriptor::NeoForge).unwrap();
        assert_eq!(d.deps[0].range, "[#PLACEHOLDER#,)");
    }

    #[test]
    fn invalid_toml_returns_none_so_the_caller_can_fall_back() {
        // An unescaped backslash in a Windows path — a real authoring mistake.
        assert!(parse(
            "[[mods]]\nlogoFile=\"assets\\icon.png\"\n",
            Descriptor::NeoForge
        )
        .is_none());
    }
}
