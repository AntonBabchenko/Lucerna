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

/// Split a version into tokens on `.` and `-`, then compare position by
/// position. Confident only while both tokens are all-ASCII-digits; the first
/// position where either side is non-numeric and they are not byte-equal
/// yields `Unknown`. Trailing all-zero / missing tokens compare equal.
fn compare_numeric(a: &str, b: &str) -> Cmp {
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
        return cmp_to_sat(compare_numeric(installed, range), Bound::AtLeast);
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

enum Bound {
    AtLeast,
}

fn cmp_to_sat(c: Cmp, _b: Bound) -> Satisfaction {
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

// predicate_satisfies is a stub here — implemented in Task 3.
fn predicate_satisfies(_installed: &str, _pred: &str, _is_quilt: bool) -> Satisfaction {
    Satisfaction::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use RangeFamily::Maven;

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
}
