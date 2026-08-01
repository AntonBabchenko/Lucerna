//! Decompose a declared version range into typed clauses the UI can render in
//! plain language.
//!
//! This lives in Rust, next to the evaluator, on purpose. The two range
//! grammars (Maven, and the Fabric/Quilt semver predicate) are parsed in
//! exactly one place; a TypeScript humaniser would be a second parser of the
//! same grammars, free to drift from what `satisfies` actually decides — which
//! is the class of bug this module exists to prevent. `describe` reuses
//! `version_range`'s own parsing helpers rather than re-implementing them.
//!
//! Nothing here evaluates anything: a clause says what the range *means*, not
//! whether a given version fits.

use crate::mods::version_range::{
    caret_upper, is_bare_maven_spec, is_wildcard_version, parse_maven_restrictions,
    split_predicate_term, RangeFamily,
};

/// One readable statement about acceptable versions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RangeClause {
    /// Any version at all — an omitted or `*` range.
    Any,
    /// A Maven soft requirement: the version is a recommendation and every
    /// version is accepted. Must never be worded as a requirement.
    Soft { version: String },
    /// Exactly this version.
    Exact { version: String },
    /// This version or newer.
    AtLeast { version: String },
    /// Strictly newer than this.
    Above { version: String },
    /// This version or older.
    AtMost { version: String },
    /// Strictly older than this.
    Below { version: String },
    /// Between two bounds.
    Between {
        low: String,
        low_inclusive: bool,
        high: String,
        high_inclusive: bool,
    },
}

/// A whole range, decomposed. `alternatives` is an OR of ANDs: the outer list
/// is the alternatives (Maven's comma-separated groups, a predicate's `||`),
/// the inner list the terms that must all hold (a predicate's space-separated
/// terms).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RangeDescription {
    /// The string the mod actually declared. Shown to the user only when
    /// `unparseable` — quoting the mod beats inventing a phrase we cannot
    /// justify.
    pub raw: String,
    pub family: RangeFamily,
    pub alternatives: Vec<Vec<RangeClause>>,
    /// The range could not be decomposed; render `raw` verbatim.
    pub unparseable: bool,
    /// Every alternative is `Soft` or `Any` — this constrains nothing.
    pub soft: bool,
}

impl RangeDescription {
    fn unparseable(raw: &str, family: RangeFamily) -> Self {
        Self {
            raw: raw.to_string(),
            family,
            alternatives: Vec::new(),
            unparseable: true,
            soft: false,
        }
    }

    fn of(raw: &str, family: RangeFamily, alternatives: Vec<Vec<RangeClause>>) -> Self {
        let soft = !alternatives.is_empty()
            && alternatives
                .iter()
                .flatten()
                .all(|c| matches!(c, RangeClause::Any) || matches!(c, RangeClause::Soft { .. }));
        Self {
            raw: raw.to_string(),
            family,
            alternatives,
            unparseable: false,
            soft,
        }
    }
}

/// Decompose `range` under `family`.
pub fn describe(range: &str, family: RangeFamily) -> RangeDescription {
    match family {
        RangeFamily::Maven => describe_maven(range),
        RangeFamily::FabricPredicate => describe_predicate(range, family, false),
        RangeFamily::QuiltPredicate => describe_predicate(range, family, true),
    }
}

fn describe_maven(range: &str) -> RangeDescription {
    let trimmed = range.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return RangeDescription::of(range, RangeFamily::Maven, vec![vec![RangeClause::Any]]);
    }
    if is_bare_maven_spec(trimmed) {
        return RangeDescription::of(
            range,
            RangeFamily::Maven,
            vec![vec![RangeClause::Soft {
                version: trimmed.to_string(),
            }]],
        );
    }
    let Some(restrictions) = parse_maven_restrictions(trimmed) else {
        return RangeDescription::unparseable(range, RangeFamily::Maven);
    };
    if restrictions.is_empty() {
        return RangeDescription::unparseable(range, RangeFamily::Maven);
    }
    let alternatives = restrictions
        .iter()
        .map(|r| {
            let clause = match (&r.lower, &r.upper) {
                (Some(lo), Some(hi)) if lo == hi && r.lower_inclusive && r.upper_inclusive => {
                    RangeClause::Exact {
                        version: lo.clone(),
                    }
                }
                (Some(lo), Some(hi)) => RangeClause::Between {
                    low: lo.clone(),
                    low_inclusive: r.lower_inclusive,
                    high: hi.clone(),
                    high_inclusive: r.upper_inclusive,
                },
                (Some(lo), None) if r.lower_inclusive => RangeClause::AtLeast {
                    version: lo.clone(),
                },
                (Some(lo), None) => RangeClause::Above {
                    version: lo.clone(),
                },
                (None, Some(hi)) if r.upper_inclusive => RangeClause::AtMost {
                    version: hi.clone(),
                },
                (None, Some(hi)) => RangeClause::Below {
                    version: hi.clone(),
                },
                // Bound-less groups are rejected by parse_maven_restrictions.
                (None, None) => RangeClause::Any,
            };
            vec![clause]
        })
        .collect();
    RangeDescription::of(range, RangeFamily::Maven, alternatives)
}

fn describe_predicate(range: &str, family: RangeFamily, is_quilt: bool) -> RangeDescription {
    let trimmed = range.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return RangeDescription::of(range, family, vec![vec![RangeClause::Any]]);
    }
    let mut alternatives = Vec::new();
    for alt in trimmed.split("||") {
        let alt = alt.trim();
        if alt.is_empty() {
            return RangeDescription::unparseable(range, family);
        }
        let mut clauses = Vec::new();
        for term in alt.split_whitespace() {
            match describe_term(term, is_quilt) {
                Some(c) => clauses.push(c),
                None => return RangeDescription::unparseable(range, family),
            }
        }
        if clauses.is_empty() {
            return RangeDescription::unparseable(range, family);
        }
        alternatives.push(clauses);
    }
    RangeDescription::of(range, family, alternatives)
}

fn describe_term(term: &str, is_quilt: bool) -> Option<RangeClause> {
    let (op, ver) = split_predicate_term(term, is_quilt);
    if ver.is_empty() || is_wildcard_version(ver) {
        return None;
    }
    let version = ver.to_string();
    Some(match op {
        "=" => RangeClause::Exact { version },
        ">=" => RangeClause::AtLeast { version },
        ">" => RangeClause::Above { version },
        "<=" => RangeClause::AtMost { version },
        "<" => RangeClause::Below { version },
        // Render the concrete computed bound rather than "up to the next major
        // release" — the same bound the evaluator uses.
        "^" | "~" => RangeClause::Between {
            low: version.clone(),
            low_inclusive: true,
            high: caret_upper(ver, op == "^")?,
            high_inclusive: false,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::version_range::RangeFamily::{FabricPredicate, Maven, QuiltPredicate};

    #[test]
    fn maven_upper_bound_is_described_as_at_most() {
        let d = describe("(,6.0.9]", Maven);
        assert!(!d.unparseable);
        assert!(!d.soft);
        assert_eq!(
            d.alternatives,
            vec![vec![RangeClause::AtMost {
                version: "6.0.9".into()
            }]]
        );
    }

    #[test]
    fn a_bare_maven_spec_is_soft() {
        let d = describe("1.21-1.3", Maven);
        assert!(
            d.soft,
            "a bare spec constrains nothing and must not read as a requirement"
        );
        assert_eq!(
            d.alternatives,
            vec![vec![RangeClause::Soft {
                version: "1.21-1.3".into()
            }]]
        );
    }

    #[test]
    fn empty_range_is_any() {
        let d = describe("", Maven);
        assert_eq!(d.alternatives, vec![vec![RangeClause::Any]]);
        assert!(d.soft);
    }

    #[test]
    fn maven_bounds_map_to_their_clauses() {
        assert_eq!(
            describe("[1.3.51,)", Maven).alternatives,
            vec![vec![RangeClause::AtLeast {
                version: "1.3.51".into()
            }]]
        );
        assert_eq!(
            describe("(1.0,)", Maven).alternatives,
            vec![vec![RangeClause::Above {
                version: "1.0".into()
            }]]
        );
        assert_eq!(
            describe("(,1.0)", Maven).alternatives,
            vec![vec![RangeClause::Below {
                version: "1.0".into()
            }]]
        );
        assert_eq!(
            describe("[1.0]", Maven).alternatives,
            vec![vec![RangeClause::Exact {
                version: "1.0".into()
            }]]
        );
        assert_eq!(
            describe("[1.0,2.0)", Maven).alternatives,
            vec![vec![RangeClause::Between {
                low: "1.0".into(),
                low_inclusive: true,
                high: "2.0".into(),
                high_inclusive: false,
            }]]
        );
    }

    #[test]
    fn or_groups_and_and_terms_survive_decomposition() {
        let d = describe("(,1.0],[1.2,)", Maven);
        assert_eq!(d.alternatives.len(), 2);
        let f = describe(">=1.0.0 <2.0.0 || >=3.0.0", FabricPredicate);
        assert_eq!(f.alternatives.len(), 2);
        assert_eq!(f.alternatives[0].len(), 2);
        assert_eq!(f.alternatives[1].len(), 1);
    }

    #[test]
    fn caret_is_described_with_the_concrete_upper_bound() {
        // "up to the next major release" has no clean short Russian form, and
        // the evaluator already derives the exact bound.
        assert_eq!(
            describe("^0.5.11", FabricPredicate).alternatives,
            vec![vec![RangeClause::Between {
                low: "0.5.11".into(),
                low_inclusive: true,
                high: "0.6.0".into(),
                high_inclusive: false,
            }]]
        );
        assert_eq!(
            describe("~1.2.3", FabricPredicate).alternatives,
            vec![vec![RangeClause::Between {
                low: "1.2.3".into(),
                low_inclusive: true,
                high: "1.3.0".into(),
                high_inclusive: false,
            }]]
        );
    }

    #[test]
    fn fabric_bare_is_exact_and_quilt_bare_is_a_caret_span() {
        assert_eq!(
            describe("1.0.0", FabricPredicate).alternatives,
            vec![vec![RangeClause::Exact {
                version: "1.0.0".into()
            }]]
        );
        assert_eq!(
            describe("1.0.0", QuiltPredicate).alternatives,
            vec![vec![RangeClause::Between {
                low: "1.0.0".into(),
                low_inclusive: true,
                high: "2.0.0".into(),
                high_inclusive: false,
            }]]
        );
    }

    #[test]
    fn an_undecomposable_range_falls_back_to_the_raw_string() {
        let d = describe("1.2.x", FabricPredicate);
        assert!(d.unparseable);
        assert_eq!(d.raw, "1.2.x");
        assert!(d.alternatives.is_empty());

        let trailing = describe(">=1.0 ||", FabricPredicate);
        assert!(trailing.unparseable);
    }

    #[test]
    fn a_degenerate_maven_range_is_unparseable_not_invented() {
        // `[]` is the shape AsyncParticles ships; the evaluator returns Unknown
        // for it, so the description must not claim a bound either.
        assert!(describe("[]", Maven).unparseable);
        assert!(describe("junk", Maven).soft || !describe("junk", Maven).unparseable);
    }

    #[test]
    fn describe_agrees_with_the_evaluator_about_what_is_parseable() {
        // The two must not drift: a Maven range the evaluator can parse is a
        // range we can describe, and vice versa.
        for range in [
            "[1.0,)",
            "(,2.0]",
            "[1.0,2.0)",
            "[1.0]",
            "(,1.0],[1.2,)",
            "1.5",
            "",
            "*",
            "[]",
            "junk",
        ] {
            let d = describe(range, Maven);
            let evaluable = range.trim().is_empty()
                || range.trim() == "*"
                || is_bare_maven_spec(range)
                || parse_maven_restrictions(range.trim()).is_some_and(|r| !r.is_empty());
            assert_eq!(
                !d.unparseable, evaluable,
                "describe and the evaluator disagree about {range:?}"
            );
        }
    }
}
