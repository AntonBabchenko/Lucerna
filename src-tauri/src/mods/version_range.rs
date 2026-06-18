//! Pure version-range evaluator. No network, no disk. Two families: Maven
//! (Forge/NeoForge) and a semver predicate (Fabric/Quilt). Conservative:
//! when a comparison cannot be made confidently it returns `Unknown`, which
//! callers treat as "do not flag".

/// Result of comparing two version strings. `Unknown` when a confident
/// numeric comparison is impossible (a qualifier in a decisive position).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmp {
    Less,
    Equal,
    Greater,
    Unknown,
}

/// If `v` is `<mcver>-<modver>` where mcver is `1.<minor>[.patch]` (all numeric)
/// and modver starts with a digit, return modver. Else `None`. Used to drop the
/// Minecraft-version prefix that Forge/NeoForge mods embed in their versions
/// (e.g. `1.19.2-5.1.3.0`), which otherwise poisons a token-wise comparison.
fn strip_mc_prefix(v: &str) -> Option<&str> {
    let (mc, rest) = v.split_once('-')?;
    let mut parts = mc.split('.');
    if parts.next()? != "1" {
        return None; // modern Minecraft is 1.x
    }
    if parts.next()?.parse::<u32>().is_err() {
        return None; // minor must be numeric
    }
    if let Some(patch) = parts.next() {
        if patch.parse::<u32>().is_err() {
            return None;
        }
    }
    if parts.next().is_some() {
        return None; // more than 3 components — not an MC version
    }
    rest.starts_with(|c: char| c.is_ascii_digit())
        .then_some(rest)
}

/// Split a version into tokens on `.` and `-`, then compare position by
/// position. Confident only while both tokens are all-ASCII-digits; the first
/// position where either side is non-numeric and they are not byte-equal
/// yields `Unknown`. Trailing all-zero / missing tokens compare equal.
fn compare_tokens(a: &str, b: &str) -> Cmp {
    let split = |s: &str| -> Vec<String> { s.split(['.', '-']).map(|t| t.to_string()).collect() };
    let at = split(a);
    let bt = split(b);
    let n = at.len().max(bt.len());
    for i in 0..n {
        let x = at.get(i).map(String::as_str).unwrap_or("0");
        let y = bt.get(i).map(String::as_str).unwrap_or("0");
        let xn = x.parse::<u64>();
        let yn = y.parse::<u64>();
        match (xn, yn) {
            (Ok(xv), Ok(yv)) if xv != yv => {
                return if xv < yv { Cmp::Less } else { Cmp::Greater };
            }
            (Ok(_), Ok(_)) => continue, // equal numerically, keep going
            _ => {
                if x == y {
                    continue; // identical qualifier token — not decisive
                }
                return Cmp::Unknown; // qualifier decides — bail conservatively
            }
        }
    }
    Cmp::Equal
}

/// Token-wise version comparison. When BOTH sides carry an MC-version prefix
/// (`<mc>-<modver>`), compares only the mod-version part — the MC segments can
/// differ (e.g. `1.19.2` vs `1.19`) and would otherwise misalign the tokens.
fn compare_numeric(a: &str, b: &str) -> Cmp {
    if let (Some(am), Some(bm)) = (strip_mc_prefix(a), strip_mc_prefix(b)) {
        return compare_tokens(am, bm);
    }
    compare_tokens(a, b)
}

/// Which grammar a raw range string uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeFamily {
    Maven,
    FabricPredicate,
    QuiltPredicate,
}

/// Whether an installed version satisfies a declared range. `Unknown` is the
/// conservative "cannot decide — do not flag" outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Satisfaction {
    Satisfied,
    Violated,
    Unknown,
}

/// Dev sentinel Forge emits when `${file.jarVersion}` cannot be resolved; the
/// loader treats such a dependency as satisfied regardless of range.
const DEV_SENTINEL: &str = "0.0NONE";

pub fn satisfies(installed: &str, range: &str, family: RangeFamily) -> Satisfaction {
    if installed.trim().is_empty() || installed == DEV_SENTINEL {
        return Satisfaction::Unknown;
    }
    match family {
        RangeFamily::Maven => maven_satisfies(installed, range),
        RangeFamily::FabricPredicate => predicate_satisfies(installed, range, false),
        RangeFamily::QuiltPredicate => predicate_satisfies(installed, range, true),
    }
}

/// One Maven restriction: lower/upper bounds with inclusivity. `None` bound =
/// unbounded on that side.
struct Restriction {
    lower: Option<String>,
    lower_inclusive: bool,
    upper: Option<String>,
    upper_inclusive: bool,
}

fn maven_satisfies(installed: &str, range: &str) -> Satisfaction {
    let range = range.trim();
    // Empty range = any version (Forge doc).
    if range.is_empty() || range == "*" {
        return Satisfaction::Satisfied;
    }
    // No brackets => bare version, redefined as a minimum (>=).
    if !range.starts_with('[') && !range.starts_with('(') {
        return cmp_to_sat(compare_numeric(installed, range));
    }
    let restrictions = match parse_maven_restrictions(range) {
        Some(r) if !r.is_empty() => r,
        _ => return Satisfaction::Unknown, // unparseable bracketed range
    };
    // OR across comma-separated bracket groups: satisfied if ANY group holds.
    let mut any_unknown = false;
    for r in &restrictions {
        match restriction_holds(installed, r) {
            Satisfaction::Satisfied => return Satisfaction::Satisfied,
            Satisfaction::Unknown => any_unknown = true,
            Satisfaction::Violated => {}
        }
    }
    if any_unknown {
        Satisfaction::Unknown
    } else {
        Satisfaction::Violated
    }
}

fn cmp_to_sat(c: Cmp) -> Satisfaction {
    match c {
        Cmp::Greater | Cmp::Equal => Satisfaction::Satisfied, // installed >= bare
        Cmp::Less => Satisfaction::Violated,
        Cmp::Unknown => Satisfaction::Unknown,
    }
}

fn restriction_holds(installed: &str, r: &Restriction) -> Satisfaction {
    if let Some(lo) = &r.lower {
        match compare_numeric(installed, lo) {
            Cmp::Less => return Satisfaction::Violated,
            Cmp::Equal if !r.lower_inclusive => return Satisfaction::Violated,
            Cmp::Unknown => return Satisfaction::Unknown,
            _ => {}
        }
    }
    if let Some(hi) = &r.upper {
        match compare_numeric(installed, hi) {
            Cmp::Greater => return Satisfaction::Violated,
            Cmp::Equal if !r.upper_inclusive => return Satisfaction::Violated,
            Cmp::Unknown => return Satisfaction::Unknown,
            _ => {}
        }
    }
    Satisfaction::Satisfied
}

/// Parse `[a,b]`, `(a,b)`, `[a]`, `[a,)`, `(,b]`, and comma-separated groups of
/// these. Returns `None` on malformed input.
fn parse_maven_restrictions(range: &str) -> Option<Vec<Restriction>> {
    let mut out = Vec::new();
    let bytes = range.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'(' => {
                let lower_inclusive = bytes[i] == b'[';
                let close = range[i..].find([']', ')'])? + i;
                let upper_inclusive = bytes[close] == b']';
                let inner = &range[i + 1..close];
                let (lower, upper) = match inner.split_once(',') {
                    Some((lo, hi)) => (opt(lo), opt(hi)),
                    None => (opt(inner), opt(inner)), // [a] exact
                };
                out.push(Restriction {
                    lower,
                    lower_inclusive,
                    upper,
                    upper_inclusive,
                });
                i = close + 1;
            }
            b',' | b' ' => i += 1,
            _ => return None,
        }
    }
    Some(out)
}

fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Fabric/Quilt semver predicate. `is_quilt` flips the bare-version rule:
/// Fabric bare `1.0.0` = exact; Quilt bare `1.0.0` = caret (`>=1.0.0 <2.0.0`).
/// Top-level OR is split on a synthetic ` || ` the reader inserts for arrays;
/// within one alternative, space-separated terms are AND-ed.
fn predicate_satisfies(installed: &str, pred: &str, is_quilt: bool) -> Satisfaction {
    let pred = pred.trim();
    if pred.is_empty() || pred == "*" {
        return Satisfaction::Satisfied;
    }
    // OR across alternatives. A malformed (empty) alternative makes the whole
    // predicate un-evaluable — Unknown propagates and prevents a Satisfied result,
    // keeping the conservative contract.
    let mut any_satisfied = false;
    let mut any_unknown = false;
    for alt in pred.split("||") {
        match alternative_holds(installed, alt.trim(), is_quilt) {
            Satisfaction::Satisfied => any_satisfied = true,
            Satisfaction::Unknown => any_unknown = true,
            Satisfaction::Violated => {}
        }
    }
    if any_unknown {
        Satisfaction::Unknown
    } else if any_satisfied {
        Satisfaction::Satisfied
    } else {
        Satisfaction::Violated
    }
}

fn alternative_holds(installed: &str, alt: &str, is_quilt: bool) -> Satisfaction {
    // An empty/whitespace-only alternative (e.g. trailing `||`) cannot be
    // evaluated — treat conservatively as Unknown rather than Satisfied.
    if alt.trim().is_empty() {
        return Satisfaction::Unknown;
    }
    let mut any_unknown = false;
    for term in alt.split_whitespace() {
        match term_holds(installed, term, is_quilt) {
            Satisfaction::Satisfied => {}
            Satisfaction::Violated => return Satisfaction::Violated,
            Satisfaction::Unknown => any_unknown = true,
        }
    }
    if any_unknown {
        Satisfaction::Unknown
    } else {
        Satisfaction::Satisfied
    }
}

fn term_holds(installed: &str, term: &str, is_quilt: bool) -> Satisfaction {
    let (op, ver) = if let Some(v) = term.strip_prefix(">=") {
        (">=", v)
    } else if let Some(v) = term.strip_prefix("<=") {
        ("<=", v)
    } else if let Some(v) = term.strip_prefix('>') {
        (">", v)
    } else if let Some(v) = term.strip_prefix('<') {
        ("<", v)
    } else if let Some(v) = term.strip_prefix('=') {
        ("=", v)
    } else if let Some(v) = term.strip_prefix('^') {
        ("^", v)
    } else if let Some(v) = term.strip_prefix('~') {
        ("~", v)
    } else {
        // bare
        if is_quilt {
            ("^", term) // Quilt bare = caret
        } else {
            ("=", term) // Fabric bare = exact
        }
    };
    // x-range (e.g. 1.2.x) — treat as caret on the fixed prefix; conservative
    // fallback to Unknown if it contains a wildcard we don't model precisely.
    if ver.contains('x') || ver.contains('X') || ver.contains('*') {
        return Satisfaction::Unknown;
    }
    let c = compare_numeric(installed, ver);
    if c == Cmp::Unknown {
        return Satisfaction::Unknown;
    }
    match op {
        "=" => bool_sat(c == Cmp::Equal),
        ">=" => bool_sat(c != Cmp::Less),
        ">" => bool_sat(c == Cmp::Greater),
        "<=" => bool_sat(c != Cmp::Greater),
        "<" => bool_sat(c == Cmp::Less),
        "^" => caret_or_tilde(installed, ver, true),
        "~" => caret_or_tilde(installed, ver, false),
        _ => Satisfaction::Unknown,
    }
}

fn bool_sat(b: bool) -> Satisfaction {
    if b {
        Satisfaction::Satisfied
    } else {
        Satisfaction::Violated
    }
}

/// Upper bound for `^`/`~` ranges, following semver:
///   - `^0.x.y` = `>=0.x.y <0.(x+1).0`  (zero-major: minor is the breaking digit)
///   - `^M.x.y` (M >= 1) = `>=M.x.y <(M+1).0.0`
///   - `~a.b.c` = `>=a.b.c <a.(b+1).0`  (minor bump, regardless of major)
fn caret_or_tilde(installed: &str, base: &str, caret: bool) -> Satisfaction {
    if compare_numeric(installed, base) == Cmp::Less {
        return Satisfaction::Violated;
    }
    let parts: Vec<&str> = base.split('.').collect();
    let major = parts.first().and_then(|s| s.parse::<u64>().ok());
    let minor = parts.get(1).and_then(|s| s.parse::<u64>().ok());
    let upper = match (caret, major, minor) {
        // ^0.x.y — zero-major: upper = 0.(x+1).0
        (true, Some(0), Some(n)) => {
            let n1 = match n.checked_add(1) {
                Some(v) => v,
                None => return Satisfaction::Unknown,
            };
            format!("0.{n1}.0")
        }
        // ^M.x.y (M >= 1) — upper = (M+1).0.0
        (true, Some(m), _) => {
            let m1 = match m.checked_add(1) {
                Some(v) => v,
                None => return Satisfaction::Unknown,
            };
            format!("{m1}.0.0")
        }
        // ~M.x.y — upper = M.(x+1).0
        (false, Some(m), Some(n)) => {
            let n1 = match n.checked_add(1) {
                Some(v) => v,
                None => return Satisfaction::Unknown,
            };
            format!("{m}.{n1}.0")
        }
        _ => return Satisfaction::Unknown,
    };
    match compare_numeric(installed, &upper) {
        Cmp::Less => Satisfaction::Satisfied,
        Cmp::Unknown => Satisfaction::Unknown,
        _ => Satisfaction::Violated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RangeFamily::{FabricPredicate, Maven, QuiltPredicate};

    #[test]
    fn numeric_compare_orders_the_real_bug_case() {
        assert_eq!(compare_numeric("1.3.50.2005", "1.3.51"), Cmp::Less);
        assert_eq!(compare_numeric("1.3.51", "1.3.51"), Cmp::Equal);
        assert_eq!(compare_numeric("1.3.52", "1.3.51"), Cmp::Greater);
    }

    #[test]
    fn numeric_compare_trims_trailing_zeros() {
        assert_eq!(compare_numeric("1.0", "1.0.0"), Cmp::Equal);
        assert_eq!(compare_numeric("1", "1.0.0"), Cmp::Equal);
    }

    #[test]
    fn numeric_compare_is_unknown_on_decisive_qualifier() {
        assert_eq!(compare_numeric("1.0.0-beta", "1.0.0"), Cmp::Unknown);
    }

    #[test]
    fn maven_bug_case_too_low_is_violated() {
        assert_eq!(
            satisfies("1.3.50.2005", "[1.3.51,)", Maven),
            Satisfaction::Violated
        );
        assert_eq!(
            satisfies("1.3.51", "[1.3.51,)", Maven),
            Satisfaction::Satisfied
        );
    }

    #[test]
    fn maven_bracket_inclusivity() {
        assert_eq!(satisfies("2.0", "[1.0,2.0)", Maven), Satisfaction::Violated); // upper excl
        assert_eq!(
            satisfies("2.0", "[1.0,2.0]", Maven),
            Satisfaction::Satisfied
        ); // upper incl
        assert_eq!(satisfies("1.0", "(1.0,)", Maven), Satisfaction::Violated); // lower excl
        assert_eq!(satisfies("1.5", "[1.0]", Maven), Satisfaction::Violated); // exact pin
        assert_eq!(satisfies("1.0", "[1.0]", Maven), Satisfaction::Satisfied);
    }

    #[test]
    fn maven_bare_is_minimum_and_empty_is_any() {
        assert_eq!(satisfies("0.9", "1.0", Maven), Satisfaction::Violated); // bare = >=
        assert_eq!(satisfies("1.5", "1.0", Maven), Satisfaction::Satisfied);
        assert_eq!(satisfies("0.1", "", Maven), Satisfaction::Satisfied); // empty = any
    }

    #[test]
    fn maven_or_sets() {
        // x <= 1.0 OR x >= 1.2
        assert_eq!(
            satisfies("1.1", "(,1.0],[1.2,)", Maven),
            Satisfaction::Violated
        );
        assert_eq!(
            satisfies("1.3", "(,1.0],[1.2,)", Maven),
            Satisfaction::Satisfied
        );
    }

    #[test]
    fn maven_dev_sentinel_and_qualifier_are_unknown() {
        assert_eq!(
            satisfies("0.0NONE", "[1.3.51,)", Maven),
            Satisfaction::Unknown
        );
        assert_eq!(
            satisfies("1.0.0-beta", "[1.0.0,)", Maven),
            Satisfaction::Unknown
        );
    }

    #[test]
    fn fabric_bare_is_exact_quilt_bare_is_caret() {
        // Same input string "1.0.0", different families:
        assert_eq!(
            satisfies("1.4.0", "1.0.0", FabricPredicate),
            Satisfaction::Violated
        ); // exact
        assert_eq!(
            satisfies("1.4.0", "1.0.0", QuiltPredicate),
            Satisfaction::Satisfied
        ); // caret <2.0.0
        assert_eq!(
            satisfies("2.0.0", "1.0.0", QuiltPredicate),
            Satisfaction::Violated
        ); // caret upper
    }

    #[test]
    fn fabric_operators_and_star() {
        assert_eq!(
            satisfies("0.15.0", ">=0.15", FabricPredicate),
            Satisfaction::Satisfied
        );
        assert_eq!(
            satisfies("0.14.0", ">=0.15", FabricPredicate),
            Satisfaction::Violated
        );
        assert_eq!(
            satisfies("9.9.9", "*", FabricPredicate),
            Satisfaction::Satisfied
        );
    }

    #[test]
    fn predicate_or_and_xrange_unknown() {
        assert_eq!(
            satisfies("3.0.0", ">=1.0.0 <2.0.0 || >=3.0.0", FabricPredicate),
            Satisfaction::Satisfied
        );
        assert_eq!(
            satisfies("1.2.5", "1.2.x", FabricPredicate),
            Satisfaction::Unknown
        ); // x-range → silent
    }

    // Fix 1 + Fix 2: zero-major caret (^0.x.y = >=0.x.y <0.(x+1).0)
    #[test]
    fn caret_zero_major_rejects_version_past_next_minor() {
        // ^0.92.0: upper = 0.93.0; 0.99.0 is above that → Violated
        assert_eq!(
            satisfies("0.99.0", "^0.92.0", FabricPredicate),
            Satisfaction::Violated
        );
        // 0.92.5 is within [0.92.0, 0.93.0) → Satisfied
        assert_eq!(
            satisfies("0.92.5", "^0.92.0", FabricPredicate),
            Satisfaction::Satisfied
        );
        // 0.93.0 is exactly the upper bound (exclusive) → Violated
        assert_eq!(
            satisfies("0.93.0", "^0.92.0", FabricPredicate),
            Satisfaction::Violated
        );
    }

    // Tilde: ~M.x.y = >=M.x.y <M.(x+1).0
    #[test]
    fn tilde_allows_patch_bumps_not_minor_bumps() {
        assert_eq!(
            satisfies("1.2.9", "~1.2.3", FabricPredicate),
            Satisfaction::Satisfied
        );
        assert_eq!(
            satisfies("1.3.0", "~1.2.3", FabricPredicate),
            Satisfaction::Violated
        );
        // base itself is satisfied
        assert_eq!(
            satisfies("1.2.3", "~1.2.3", FabricPredicate),
            Satisfaction::Satisfied
        );
    }

    // <= operator
    #[test]
    fn lte_operator() {
        assert_eq!(
            satisfies("1.0.0", "<=1.0.0", FabricPredicate),
            Satisfaction::Satisfied
        );
        assert_eq!(
            satisfies("1.0.1", "<=1.0.0", FabricPredicate),
            Satisfaction::Violated
        );
    }

    // < operator
    #[test]
    fn lt_operator() {
        assert_eq!(
            satisfies("0.9.0", "<1.0.0", FabricPredicate),
            Satisfaction::Satisfied
        );
        assert_eq!(
            satisfies("1.0.0", "<1.0.0", FabricPredicate),
            Satisfaction::Violated
        );
    }

    // Fix 4: trailing || produces an empty alternative → Unknown (conservative).
    // An Unknown alternative poisons the whole OR expression so the result is
    // Unknown even when another alternative is Satisfied — preserving the
    // "cannot be confident → do not flag" contract.
    #[test]
    fn trailing_or_alternative_is_unknown_not_satisfied() {
        // ">=1.0 ||" — first alt Satisfied, second alt empty (Unknown).
        // Unknown wins → result is Unknown, not Satisfied.
        assert_eq!(
            satisfies("1.0.0", ">=1.0 ||", FabricPredicate),
            Satisfaction::Unknown
        );
        // ">=2.0 ||" — first alt Violated, second empty (Unknown) → Unknown.
        assert_eq!(
            satisfies("1.0.0", ">=2.0 ||", FabricPredicate),
            Satisfaction::Unknown
        );
    }

    #[test]
    fn strip_mc_prefix_detects_mc_prefixed_versions() {
        assert_eq!(strip_mc_prefix("1.19.2-5.1.3.0"), Some("5.1.3.0"));
        assert_eq!(strip_mc_prefix("1.21-2.29.0"), Some("2.29.0"));
        assert_eq!(strip_mc_prefix("1.19-77"), Some("77"));
        assert_eq!(strip_mc_prefix("1.5.2-pre"), None); // rest non-numeric (qualifier)
        assert_eq!(strip_mc_prefix("1.3.50.2005"), None); // no dash
        assert_eq!(strip_mc_prefix("5.0.7.1"), None); // no dash
        assert_eq!(strip_mc_prefix("1.19.2"), None); // no dash
        assert_eq!(strip_mc_prefix("2.0.0-1.0.0"), None); // mc must start with "1."
        assert_eq!(strip_mc_prefix("1.19.2.3-5.0"), None); // >3 mc components
    }

    #[test]
    fn mc_prefixed_mod_versions_compare_by_mod_version() {
        assert_eq!(
            satisfies("1.19.2-5.1.3.0", "[1.19-5.0.7.1,]", Maven),
            Satisfaction::Satisfied
        ); // curios
        assert_eq!(
            satisfies("1.21.1-3.0.9", "[1.21-2.29.0,]", Maven),
            Satisfaction::Satisfied
        ); // moonlight
        assert_eq!(
            satisfies("1.19.2-77", "[1.19-77,]", Maven),
            Satisfaction::Satisfied
        ); // patchouli (equal, inclusive)
        assert_eq!(
            satisfies("1.21.1-3.0.9", "[1.21-2.30.0,]", Maven),
            Satisfaction::Satisfied
        ); // supplementaries
    }

    #[test]
    fn mc_prefix_fix_does_not_create_false_negatives() {
        assert_eq!(
            satisfies("1.3.50.2005", "[1.3.51,)", Maven),
            Satisfaction::Violated
        ); // sophisticatedcore (real)
        assert_eq!(
            satisfies("1.19.2-1.0.0", "[1.19-2.0.0,]", Maven),
            Satisfaction::Violated
        ); // genuine out-of-range
        assert_eq!(
            satisfies("1.19.2-5.0.0", "(1.19-5.0.0,]", Maven),
            Satisfaction::Violated
        ); // equal, exclusive lower
    }
}
