//! Minecraft's own verdict on a data pack's declared formats
//! (`PackCompatibility`), recomputed offline from what the pack's
//! `pack.mcmeta` declares ([`PackMcmeta`]) and the data-pack format the
//! instance's client jar reports ([`FormatVersion`]). Spec §1 C3; the rules
//! were read from the 1.20.1, 1.20.6, 1.21.1, 1.21.11 and 26.2 jars.
//!
//! The era follows the GAME's format:
//! * **A** (≤ 15, up to 1.20.1): only `description` and `pack_format` are
//!   read, both required; other fields are ignored.
//! * **B** (16–81, 1.20.2–1.21.8): `supported_formats` replaces
//!   `[pack_format]` when it contains `pack_format`.
//! * **C** (≥ 82, 1.21.9+): `min_format`/`max_format`, validated like 26.2's
//!   `PackFormat$IntermediaryFormat.validate(81, true, false)`; a failed
//!   validation is the game's "(Broken or incompatible)" — it still loads.
//!
//! Lucerna never claims more than the game does. It does not model Gson/DFU
//! coercions (48.5 → 48; a string rejected), so anything outside a JSON
//! integer, and any `pack.mcmeta` serde cannot parse, stays `Unknown` by
//! design — never `Broken` or `WontLoad` (§0.5 A16).
//!
//! Known gaps: 1.14–1.19.4, 1.20.2–1.20.5, 1.21.2–1.21.8, 1.21.9–1.21.10 and
//! 26.1.x are inferred from their verified neighbours. Forge ≤ 1.20.1's
//! `forge:server_data_pack_format` is not modelled; it moves only the
//! TooOld/TooNew labels, never whether a pack loads.

use crate::datapacks::format::{Bound, Fact, FormatVersion, PackDeclaration, PackMcmeta};
use crate::datapacks::{PackCompat, WontLoadReason};

/// `lastPreMinorVersion(SERVER_DATA)` in 26.2: the last data format without
/// a minor, and the boundary of era C's rules.
const LAST_PRE_MINOR: u32 = 81;
/// The last era-A game format (1.20.1).
const LAST_ERA_A: u32 = 15;
/// Era C's floor for a `pack_format` declared beside a range.
const MIN_RANGED_PACK_FORMAT: u32 = 15;

/// An inclusive declared range; `hi.minor == u32::MAX` means "any minor of
/// `hi.major`" — internal only, never printed (see [`range_label`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Range {
    lo: FormatVersion,
    hi: FormatVersion,
}

impl Range {
    fn majors(lo: u32, hi: u32) -> Self {
        Self {
            lo: FormatVersion::new(lo, 0),
            hi: FormatVersion::new(hi, u32::MAX),
        }
    }
}

enum Declared {
    Range(Range),
    Broken,
    Unknown,
    WontLoad(WontLoadReason),
}

/// The game's verdict for pack `m` on game format `game`. `None` on either
/// side is `Unknown` — the restrictive answer when the check could not run.
#[must_use]
pub fn verdict(m: Option<&PackMcmeta>, game: Option<FormatVersion>) -> PackCompat {
    let (Some(m), Some(game)) = (m, game) else {
        return PackCompat::Unknown;
    };
    let d = match m {
        PackMcmeta::Unreadable => return PackCompat::Unknown,
        PackMcmeta::Missing => return wont(WontLoadReason::NoPackMcmeta),
        PackMcmeta::NoPackSection => return wont(WontLoadReason::NoPackSection),
        PackMcmeta::Read(d) => d,
    };
    match d.description {
        Fact::Absent => return wont(WontLoadReason::NoDescription),
        Fact::Invalid => return PackCompat::Unknown,
        Fact::Present(()) => {}
    }
    let declared = if game.major <= LAST_ERA_A {
        era_a(d)
    } else if game.major <= LAST_PRE_MINOR {
        era_b(d)
    } else {
        era_c(d)
    };
    match declared {
        Declared::Range(r) => compare(r, game),
        Declared::Broken => PackCompat::Broken,
        Declared::Unknown => PackCompat::Unknown,
        Declared::WontLoad(reason) => wont(reason),
    }
}

/// Whose recorded `pack.mcmeta` speaks for a row (§0.5 A1): the library's
/// declaration describes the library's own bytes, so it applies to a row
/// whose name is not on the world's disk at all (the library copy IS the
/// pack), or to a world entry the scan vouched as the library's bytes.
/// `on_disk` is `None` when the world holds no entry of this name, else
/// `Some(vouched)`. A hand-dropped, folder, or differing entry gets `None`
/// — its verdict is Unknown until a world-entry probe exists (deferred, A1).
#[must_use]
pub fn library_declaration(
    on_disk: Option<bool>,
    recorded: Option<&PackMcmeta>,
) -> Option<&PackMcmeta> {
    match on_disk {
        None | Some(true) => recorded,
        Some(false) => None,
    }
}

fn wont(reason: WontLoadReason) -> PackCompat {
    PackCompat::WontLoad { reason }
}

/// Eras A and B require `pack_format`.
fn required_pack_format(d: &PackDeclaration) -> Result<u32, Declared> {
    match d.pack_format {
        Fact::Present(n) => Ok(n),
        Fact::Absent => Err(Declared::WontLoad(WontLoadReason::NoPackFormat)),
        Fact::Invalid => Err(Declared::Unknown),
    }
}

fn era_a(d: &PackDeclaration) -> Declared {
    match required_pack_format(d) {
        Ok(n) => Declared::Range(Range::majors(n, n)),
        Err(x) => x,
    }
}

fn era_b(d: &PackDeclaration) -> Declared {
    let n = match required_pack_format(d) {
        Ok(n) => n,
        Err(x) => return x,
    };
    match d.supported_formats {
        // The game ignores a malformed value, but ours may be one Gson/DFU
        // still accepts: could not tell.
        Fact::Invalid => Declared::Unknown,
        Fact::Present((a, b)) if a <= n && n <= b => Declared::Range(Range::majors(a, b)),
        Fact::Present(_) | Fact::Absent => Declared::Range(Range::majors(n, n)),
    }
}

fn era_c(d: &PackDeclaration) -> Declared {
    let (Some(pf), Some(sf), Some(min), Some(max)) = (
        known(&d.pack_format),
        known(&d.supported_formats),
        known(&d.min_format),
        known(&d.max_format),
    ) else {
        return Declared::Unknown;
    };
    validate_c(pf, sf, min, max).map_or(Declared::Broken, Declared::Range)
}

/// `Some(value-or-absent)` for a readable fact, `None` for `Invalid`.
fn known<T: Copy>(f: &Fact<T>) -> Option<Option<T>> {
    match f {
        Fact::Absent => Some(None),
        Fact::Present(v) => Some(Some(*v)),
        Fact::Invalid => None,
    }
}

/// 26.2's `IntermediaryFormat.validate(81, true, false)`; `None` = fails.
fn validate_c(
    pack_format: Option<u32>,
    supported: Option<(u32, u32)>,
    min: Option<Bound>,
    max: Option<Bound>,
) -> Option<Range> {
    match (min, max) {
        (Some(min), Some(max)) => {
            let (lo, hi) = (lower(min), upper(max));
            if lo > hi {
                return None;
            }
            let within = |n: u32| n >= lo.major && n <= hi.major && n >= MIN_RANGED_PACK_FORMAT;
            if lo.major > LAST_PRE_MINOR {
                if supported.is_some() {
                    return None;
                }
                if pack_format.is_some_and(|n| !within(n)) {
                    return None;
                }
            } else {
                let (s_min, s_max) = supported?;
                if s_min != lo.major || (s_max != hi.major && s_max != LAST_PRE_MINOR) {
                    return None;
                }
                if !within(pack_format?) {
                    return None;
                }
            }
            Some(Range { lo, hi })
        }
        (Some(_), None) | (None, Some(_)) => None,
        (None, None) => match (supported, pack_format) {
            (Some((s_min, s_max)), pf) => {
                if s_max > LAST_PRE_MINOR {
                    return None;
                }
                let n = pf?;
                if n < s_min || n > s_max || n < MIN_RANGED_PACK_FORMAT {
                    return None;
                }
                Some(Range::majors(s_min, s_max))
            }
            // No ≥ 15 rule here: an old pack_format alone is TooOld, not Broken.
            (None, Some(n)) => (n <= LAST_PRE_MINOR).then(|| Range::majors(n, n)),
            (None, None) => None,
        },
    }
}

fn lower(b: Bound) -> FormatVersion {
    match b {
        Bound::Major(n) => FormatVersion::new(n, 0),
        Bound::Exact(n, m) => FormatVersion::new(n, m),
    }
}

fn upper(b: Bound) -> FormatVersion {
    match b {
        Bound::Major(n) => FormatVersion::new(n, u32::MAX),
        Bound::Exact(n, m) => FormatVersion::new(n, m),
    }
}

/// `PackCompatibility.forVersion`: above the range is TooOld, below is TooNew.
fn compare(r: Range, game: FormatVersion) -> PackCompat {
    if r.hi < game {
        PackCompat::TooOld {
            made_for: range_label(r),
            game: game_label(game),
        }
    } else if game < r.lo {
        PackCompat::TooNew {
            made_for: range_label(r),
            game: game_label(game),
        }
    } else {
        PackCompat::Compatible
    }
}

/// "34–48", "94–101", "107.1", "48". A minor is printed only when it is not
/// 0 on `lo` and not the `u32::MAX` sentinel on `hi`, so the sentinel never
/// leaks; equal ends print once.
fn range_label(r: Range) -> String {
    if r.lo == r.hi {
        return game_label(r.lo);
    }
    let lo = if r.lo.minor == 0 {
        r.lo.major.to_string()
    } else {
        format!("{}.{}", r.lo.major, r.lo.minor)
    };
    let hi = if r.hi.minor == u32::MAX {
        r.hi.major.to_string()
    } else {
        format!("{}.{}", r.hi.major, r.hi.minor)
    };
    if lo == hi {
        lo
    } else {
        format!("{lo}–{hi}")
    }
}

fn game_label(g: FormatVersion) -> String {
    if g.minor == 0 {
        g.major.to_string()
    } else {
        format!("{}.{}", g.major, g.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::format::{read_mcmeta_bytes, samples::*};

    const G1_20_1: FormatVersion = FormatVersion::new(15, 0);
    const G1_20_6: FormatVersion = FormatVersion::new(41, 0);
    const G1_21_1: FormatVersion = FormatVersion::new(48, 0);
    const G1_21_11: FormatVersion = FormatVersion::new(94, 1);
    const G26_2: FormatVersion = FormatVersion::new(107, 1);
    const GAMES: [FormatVersion; 5] = [G1_20_1, G1_20_6, G1_21_1, G1_21_11, G26_2];

    fn mcmeta(body: &str) -> PackMcmeta {
        read_mcmeta_bytes(body.as_bytes()).mcmeta
    }
    fn too_old(made_for: &str, game: &str) -> PackCompat {
        PackCompat::TooOld {
            made_for: made_for.into(),
            game: game.into(),
        }
    }
    fn too_new(made_for: &str, game: &str) -> PackCompat {
        PackCompat::TooNew {
            made_for: made_for.into(),
            game: game.into(),
        }
    }
    fn no_pack_format() -> PackCompat {
        PackCompat::WontLoad {
            reason: WontLoadReason::NoPackFormat,
        }
    }

    #[test]
    fn verdict_matches_the_game_for_real_packs_and_jars() {
        use PackCompat::{Broken, Compatible};
        // §1 C3's table; cobblemarks is derived by the same rules.
        let table: Vec<(&str, &str, [PackCompat; 5])> = vec![
            (
                "dagger",
                DAGGER,
                [
                    Compatible,
                    too_old("15", "41"),
                    too_old("15", "48"),
                    too_old("15", "94.1"),
                    too_old("15", "107.1"),
                ],
            ),
            (
                "bettercaps",
                BETTERCAPS,
                [
                    too_new("48", "15"),
                    Compatible,
                    Compatible,
                    too_old("34–48", "94.1"),
                    too_old("34–48", "107.1"),
                ],
            ),
            (
                "witchhuts",
                WITCHHUTS,
                [too_new("26", "15"), Compatible, Compatible, Broken, Broken],
            ),
            (
                "guns_dp",
                GUNS_DP,
                [
                    too_new("101", "15"),
                    too_new("101", "41"),
                    too_new("101", "48"),
                    Compatible,
                    too_old("94–101", "107.1"),
                ],
            ),
            (
                "nullscape",
                NULLSCAPE,
                [
                    no_pack_format(),
                    no_pack_format(),
                    no_pack_format(),
                    too_new("107.1", "94.1"),
                    Compatible,
                ],
            ),
            (
                "tectonic",
                TECTONIC,
                [
                    no_pack_format(),
                    no_pack_format(),
                    no_pack_format(),
                    too_new("121", "94.1"),
                    too_new("121", "107.1"),
                ],
            ),
            (
                "hopo",
                HOPO,
                [
                    too_old("7", "15"),
                    too_old("7", "41"),
                    too_old("7", "48"),
                    too_old("7", "94.1"),
                    too_old("7", "107.1"),
                ],
            ),
            (
                "cobblemarks",
                COBBLEMARKS,
                [
                    too_new("34", "15"),
                    too_old("34", "41"),
                    too_old("34", "48"),
                    too_old("34", "94.1"),
                    too_old("34", "107.1"),
                ],
            ),
        ];
        for (name, body, want) in table {
            let m = mcmeta(body);
            for (game, want) in GAMES.iter().zip(want) {
                assert_eq!(verdict(Some(&m), Some(*game)), want, "{name} on {game:?}");
            }
        }
    }

    #[test]
    fn era_c_rules_mirror_the_26_2_validator() {
        let broken: &[(&str, &str)] = &[
            (
                "min without max",
                r#"{"pack":{"description":"d","min_format":90}}"#,
            ),
            (
                "max without min",
                r#"{"pack":{"description":"d","max_format":95}}"#,
            ),
            (
                "lo above hi",
                r#"{"pack":{"description":"d","min_format":95,"max_format":94}}"#,
            ),
            (
                "lo above hi by minor",
                r#"{"pack":{"description":"d","min_format":[94,2],"max_format":[94,1]}}"#,
            ),
            (
                "modern range with supported_formats",
                r#"{"pack":{"description":"d","min_format":90,"max_format":95,"supported_formats":[90,95]}}"#,
            ),
            (
                "modern range, pack_format above",
                r#"{"pack":{"description":"d","min_format":90,"max_format":95,"pack_format":96}}"#,
            ),
            (
                "modern range, pack_format below",
                r#"{"pack":{"description":"d","min_format":90,"max_format":95,"pack_format":89}}"#,
            ),
            (
                "bridging range, no supported_formats",
                r#"{"pack":{"description":"d","min_format":70,"max_format":90,"pack_format":70}}"#,
            ),
            (
                "bridging range, supported min differs",
                r#"{"pack":{"description":"d","min_format":70,"max_format":90,"supported_formats":[71,90],"pack_format":70}}"#,
            ),
            (
                "bridging range, supported max neither hi nor 81",
                r#"{"pack":{"description":"d","min_format":70,"max_format":90,"supported_formats":[70,85],"pack_format":70}}"#,
            ),
            (
                "bridging range, no pack_format",
                r#"{"pack":{"description":"d","min_format":70,"max_format":90,"supported_formats":[70,90]}}"#,
            ),
            (
                "bridging range, pack_format below 15",
                r#"{"pack":{"description":"d","min_format":10,"max_format":90,"supported_formats":[10,90],"pack_format":10}}"#,
            ),
            (
                "supported only, max above 81",
                r#"{"pack":{"description":"d","supported_formats":[40,90],"pack_format":48}}"#,
            ),
            (
                "supported only, no pack_format",
                r#"{"pack":{"description":"d","supported_formats":[34,48]}}"#,
            ),
            (
                "supported only, pack_format outside",
                r#"{"pack":{"description":"d","supported_formats":[34,48],"pack_format":50}}"#,
            ),
            (
                "supported only, pack_format below 15",
                r#"{"pack":{"description":"d","supported_formats":[10,20],"pack_format":12}}"#,
            ),
            (
                "pack_format alone above 81",
                r#"{"pack":{"description":"d","pack_format":90}}"#,
            ),
            ("nothing declared", r#"{"pack":{"description":"d"}}"#),
        ];
        for (label, body) in broken {
            assert_eq!(
                verdict(Some(&mcmeta(body)), Some(G1_21_11)),
                PackCompat::Broken,
                "{label}"
            );
        }
        let valid: &[(&str, &str, PackCompat)] = &[
            (
                "pack_format 7 alone is too old, not broken",
                r#"{"pack":{"description":"d","pack_format":7}}"#,
                too_old("7", "94.1"),
            ),
            (
                "bridging range to 81",
                r#"{"pack":{"description":"d","min_format":70,"max_format":90,"supported_formats":[70,81],"pack_format":70}}"#,
                too_old("70–90", "94.1"),
            ),
            (
                "bridging range to hi",
                r#"{"pack":{"description":"d","min_format":70,"max_format":90,"supported_formats":[70,90],"pack_format":70}}"#,
                too_old("70–90", "94.1"),
            ),
            (
                "modern range covering the game",
                r#"{"pack":{"description":"d","min_format":90,"max_format":95,"pack_format":94}}"#,
                PackCompat::Compatible,
            ),
            (
                "exact bounds on the game",
                r#"{"pack":{"description":"d","min_format":[94,1],"max_format":[94,1]}}"#,
                PackCompat::Compatible,
            ),
        ];
        for (label, body, want) in valid {
            assert_eq!(
                &verdict(Some(&mcmeta(body)), Some(G1_21_11)),
                want,
                "{label}"
            );
        }
    }

    #[test]
    fn a_field_lucerna_cannot_parse_is_unknown_never_broken_or_wontload() {
        let everywhere: &[(&str, &str)] = &[
            (
                "pack_format 48.5",
                r#"{"pack":{"description":"d","pack_format":48.5}}"#,
            ),
            (
                "pack_format \"48\"",
                r#"{"pack":{"description":"d","pack_format":"48"}}"#,
            ),
            (
                "pack_format null",
                r#"{"pack":{"description":"d","pack_format":null}}"#,
            ),
            (
                "trailing comma",
                r#"{"pack":{"description":"d","pack_format":48,}}"#,
            ),
            (
                "description a number",
                r#"{"pack":{"description":5,"pack_format":48}}"#,
            ),
        ];
        for (label, body) in everywhere {
            for game in GAMES {
                assert_eq!(
                    verdict(Some(&mcmeta(body)), Some(game)),
                    PackCompat::Unknown,
                    "{label} on {game:?}"
                );
            }
        }
        // An era reads only its own fields: an invalid field another era
        // ignores does not make that era's verdict Unknown.
        use PackCompat::{Compatible, Unknown};
        let scoped: [(&str, &str, [PackCompat; 5]); 2] = [
            (
                "supported_formats [1]",
                r#"{"pack":{"description":"d","pack_format":48,"supported_formats":[1]}}"#,
                [too_new("48", "15"), Unknown, Unknown, Unknown, Unknown],
            ),
            (
                "min_format \"90\"",
                r#"{"pack":{"description":"d","pack_format":48,"min_format":"90"}}"#,
                [
                    too_new("48", "15"),
                    too_new("48", "41"),
                    Compatible,
                    Unknown,
                    Unknown,
                ],
            ),
        ];
        for (label, body, want) in scoped {
            for (game, want) in GAMES.iter().zip(want) {
                assert_eq!(
                    verdict(Some(&mcmeta(body)), Some(*game)),
                    want,
                    "{label} on {game:?}"
                );
            }
        }
    }

    #[test]
    fn the_general_cases_come_before_any_era() {
        let m = mcmeta(DAGGER);
        assert_eq!(verdict(None, Some(G1_21_1)), PackCompat::Unknown);
        assert_eq!(verdict(Some(&m), None), PackCompat::Unknown);
        for game in GAMES {
            assert_eq!(
                verdict(Some(&PackMcmeta::Unreadable), Some(game)),
                PackCompat::Unknown
            );
            assert_eq!(
                verdict(Some(&PackMcmeta::Missing), Some(game)),
                PackCompat::WontLoad {
                    reason: WontLoadReason::NoPackMcmeta
                }
            );
            assert_eq!(
                verdict(Some(&PackMcmeta::NoPackSection), Some(game)),
                PackCompat::WontLoad {
                    reason: WontLoadReason::NoPackSection
                }
            );
            assert_eq!(
                verdict(Some(&mcmeta(r#"{"pack":{"pack_format":48}}"#)), Some(game)),
                PackCompat::WontLoad {
                    reason: WontLoadReason::NoDescription
                }
            );
        }
    }

    #[test]
    fn only_the_library_s_own_bytes_carry_its_declaration() {
        let m = mcmeta(DAGGER);
        assert_eq!(
            library_declaration(None, Some(&m)),
            Some(&m),
            "not in the world: the library copy is the pack"
        );
        assert_eq!(
            library_declaration(Some(true), Some(&m)),
            Some(&m),
            "a vouched link is the library's bytes"
        );
        assert_eq!(
            library_declaration(Some(false), Some(&m)),
            None,
            "a hand-dropped or differing copy is not"
        );
        assert_eq!(library_declaration(None, None), None);
    }

    #[test]
    fn labels_never_leak_the_max_minor_and_print_equal_ends_once() {
        let r = |lo: (u32, u32), hi: (u32, u32)| Range {
            lo: FormatVersion::new(lo.0, lo.1),
            hi: FormatVersion::new(hi.0, hi.1),
        };
        assert_eq!(range_label(Range::majors(34, 48)), "34–48");
        assert_eq!(range_label(Range::majors(94, 101)), "94–101");
        assert_eq!(range_label(Range::majors(48, 48)), "48");
        assert_eq!(range_label(r((107, 1), (107, 1))), "107.1");
        assert_eq!(range_label(r((94, 2), (101, u32::MAX))), "94.2–101");
        assert_eq!(range_label(r((90, 0), (95, 3))), "90–95.3");
        assert_eq!(game_label(G1_21_11), "94.1");
        assert_eq!(game_label(G1_21_1), "48");
    }
}
