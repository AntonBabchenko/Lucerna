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

#[cfg(test)]
mod tests {
    use super::*;

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
}
