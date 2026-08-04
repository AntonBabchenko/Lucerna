//! Human-readable directory slugs for instances and servers.
//!
//! Domain-neutral leaf module: both `instances::` and `servers_runtime::`
//! derive their on-disk directory names here, so neither depends on the other
//! for naming policy.
//!
//! The design deliberately uses a character **allowlist** (keep ASCII
//! letters/digits + `-`/`_`, replace everything else with `-`) rather than a
//! per-OS denylist. Keeping only the universally-safe intersection means the
//! result is a valid directory component on Windows, macOS, and Linux without
//! enumerating each platform's forbidden set.
//!
//! **ASCII specifically, not "safe Unicode".** The Windows JVM launcher decodes
//! its command line through the system ANSI code page, and we pass instance
//! paths as `--gameDir` and `-Djava.library.path`. A directory name holding
//! characters that code page cannot express therefore reaches `java.exe` with
//! `?` substituted — an illegal Windows path character — and the game dies with
//! `InvalidPathException` before Minecraft starts. Which characters are affected
//! depends on the user's locale, so this cannot be narrowed to "non-Latin".
//! Readability for non-Latin names is restored one level up by [`derive_base`],
//! never by widening this allowlist.
//!
//! Further consequences:
//!
//! - No dots survive → Windows "reserved-name-with-extension" (`nul.txt`) and
//!   `.`/`..` traversal are structurally impossible; the reserved-name check is
//!   a plain exact match.
//! - Control / format / bidi / zero-width chars, emoji, and shell-significant
//!   punctuation are all non-alphanumeric → replaced.
//! - Length is capped below the old 36-char UUID leaf, so nested `.minecraft`
//!   paths gain MAX_PATH headroom rather than losing it.
//!
//! Uniqueness is enforced structurally by [`reserve_unique_dir`]: a
//! case-insensitive collision (consistent across case-sensitive and
//! case-insensitive filesystems) appends `-2`, `-3`, … The reservation is
//! atomic (`std::fs::create_dir`, which fails if the directory exists), so it
//! is a real reserve, not a check-then-create.

use crate::error::{Error, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Cap on a slug's length in characters. Below the 36-char UUID it replaces, so
/// deep `instances/<slug>/.minecraft/...` paths stay further from Windows'
/// 260-char MAX_PATH than they were with UUID leaves.
const MAX_SLUG_LEN: usize = 32;

/// Upper bound on the numeric collision suffix before giving up. Far beyond any
/// realistic number of same-named instances; exists only so a pathological
/// filesystem can't spin the loop forever.
const MAX_SUFFIX: u32 = 10_000;

/// Turn a display name into a filesystem-safe directory slug.
///
/// Keeps ASCII alphanumerics plus `-`/`_`; every other character — including
/// Cyrillic and CJK letters — becomes `-`. Consecutive `-` collapse,
/// leading/trailing separators are stripped, the result is capped at
/// [`MAX_SLUG_LEN`] characters, and an empty result falls back to
/// `fallback_base` (e.g. `"instance"` / `"server"`).
///
/// Callers wanting a readable name for a non-Latin display name should use
/// [`derive_base`], which falls back to transliteration instead of to a bare
/// `"instance"`.
///
/// Original case is preserved (`All The Mods 10` → `All-The-Mods-10`).
pub fn slugify(name: &str, fallback_base: &str) -> String {
    let mut out = String::new();
    // Start "in a dash run" so leading separators are dropped entirely rather
    // than producing a leading `-` that we'd only trim later.
    let mut prev_dash = true;
    let mut kept = 0usize;
    for ch in name.trim().chars() {
        if kept >= MAX_SLUG_LEN {
            break;
        }
        let mapped = if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            ch
        } else {
            '-'
        };
        if mapped == '-' {
            if prev_dash {
                continue; // collapse runs and drop leading separators
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
        out.push(mapped);
        kept += 1;
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        fallback_base.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Minimum ASCII **letters** a candidate must carry to count as readable.
/// Digits deliberately excluded: "Сборка 1.20 с шейдерами" reduces to `1-20`,
/// which looks like a name and identifies nothing. Three, because no meaningful
/// folder name is two letters long and three already give a recognisable root.
const MIN_RESIDUE_LETTERS: usize = 3;

fn ascii_letter_count(s: &str) -> usize {
    s.chars().filter(|c| c.is_ascii_alphabetic()).count()
}

/// Replace every non-alphanumeric character with a space, ahead of
/// transliteration.
///
/// Unidecode-family transliteration does two different jobs: it romanises
/// scripts (`Моя` → `Moia`) **and** it spells symbols out as English words
/// (`🎮` → `video game`, `€` → `EUR`). Only the first is wanted here — without
/// this filter the name `🎮🎮🎮` produces the directory
/// `video-game-video-game-video-game`, which is both absurd and worse than the
/// `instance` fallback it replaced. Symbols are decorative; the display name
/// keeps them, the directory does not.
fn letters_and_digits_only(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect()
}

/// Derive a readable ASCII directory base from a display name.
///
/// Ladder, in this order:
/// 1. the ASCII residue of `display_name`, if it carries enough letters;
/// 2. `pack_slug`, when the instance is being installed from a modpack;
/// 3. transliteration of `display_name`;
/// 4. `fallback_base` (`"instance"` / `"server"`), which [`reserve_unique_dir`]
///    then suffixes as needed.
///
/// **The order is load-bearing in two places.** Transliterating before taking
/// the residue would turn `红石生电优化【Redstone Survival Optimization】` into
/// `Hong-Shi-Sheng-Dian-You-Hua-Reds` (truncated at the cap) instead of
/// `Redstone-Survival-Optimization`. And a pack's canonical slug is real English
/// matching what the user saw on Modrinth, so it beats romanisation whenever it
/// exists.
///
/// Passing `""` as `fallback_base` makes "nothing usable" observable (an empty
/// return) rather than silently becoming `instance` — the rename command relies
/// on that to reject a name outright.
pub fn derive_base(display_name: &str, pack_slug: Option<&str>, fallback_base: &str) -> String {
    let residue = slugify(display_name, "");
    if ascii_letter_count(&residue) >= MIN_RESIDUE_LETTERS {
        return residue;
    }
    if let Some(slug) = pack_slug {
        let from_slug = slugify(slug, "");
        if ascii_letter_count(&from_slug) >= MIN_RESIDUE_LETTERS {
            return from_slug;
        }
    }
    let translit = slugify(
        &deunicode::deunicode(&letters_and_digits_only(display_name)),
        "",
    );
    if ascii_letter_count(&translit) >= MIN_RESIDUE_LETTERS {
        return translit;
    }
    fallback_base.to_string()
}

/// True iff `name` is a Windows reserved device name (case-insensitive).
///
/// Because slugs never contain a `.`, matching the whole slug is sufficient
/// (the "reserved name with any extension is also reserved" rule can't apply).
/// Delegates to the shared list in `pathsafe` so the two gates can't drift.
pub fn is_reserved(name: &str) -> bool {
    crate::pathsafe::is_reserved_windows_name(name)
}

/// Ordered candidate stream: `base`, `base-2`, `base-3`, … up to [`MAX_SUFFIX`].
///
/// Probe-first (not `max(existing suffix)+1`): the caller tries each in turn
/// and takes the first free one, so a deleted middle suffix refills correctly.
fn candidates(base: &str) -> impl Iterator<Item = String> + '_ {
    std::iter::once(base.to_string()).chain((2..=MAX_SUFFIX).map(move |n| format!("{base}-{n}")))
}

/// Lowercased set of the directory names already present under `parent`.
/// A missing/unreadable `parent` yields an empty set (the atomic create still
/// guards against races).
fn existing_lowercased(parent: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                set.insert(name.to_lowercase());
            }
        }
    }
    set
}

/// Atomically reserve a unique sub-directory of `parent` for a new
/// instance/server named `name`.
///
/// Slugifies `name`, ensures `parent` exists, then creates the first free
/// candidate directory (`slug`, `slug-2`, …). Skips Windows reserved names and,
/// **case-insensitively**, any name already present — so behaviour is identical
/// on case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems.
///
/// Returns the final reserved name (the id) and its path. Callers MUST persist
/// this returned name as the instance/server id — never the pre-reservation
/// slug — and are responsible for removing the reserved directory if a later
/// step fails (orphan cleanup).
pub fn reserve_unique_dir(
    parent: &Path,
    name: &str,
    pack_slug: Option<&str>,
    fallback_base: &str,
) -> Result<(String, PathBuf)> {
    let base = derive_base(name, pack_slug, fallback_base);
    // Idempotent; guarantees the non-recursive create below won't hit NotFound
    // when `instances/` or `servers/` doesn't exist yet.
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    let taken = existing_lowercased(parent);
    for candidate in candidates(&base) {
        if is_reserved(&candidate) || taken.contains(&candidate.to_lowercase()) {
            continue;
        }
        let path = parent.join(&candidate);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok((candidate, path)),
            // Lost a race (or a case-insensitive collision the snapshot missed):
            // try the next candidate.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io(path.display().to_string(), e)),
        }
    }
    Err(Error::io(
        parent.join(&base).display().to_string(),
        "could not reserve a unique directory name",
    ))
}

/// RAII guard that removes a reserved directory on drop unless disarmed.
///
/// Create one right after [`reserve_unique_dir`]; any early return (`?`)
/// between the reservation and success then removes the partial directory, so a
/// failed create never leaks the slug (which would force every future
/// same-name create to climb `-2`, `-3`, …). Call [`DirCleanup::keep`] on
/// success to commit the directory. Also cleans up if an async future holding
/// the guard is cancelled mid-create.
pub struct DirCleanup {
    path: Option<PathBuf>,
}

impl DirCleanup {
    pub fn new(path: &Path) -> Self {
        Self {
            path: Some(path.to_path_buf()),
        }
    }

    /// Disarm the guard — the directory is now a committed instance/server.
    pub fn keep(mut self) {
        self.path = None;
    }
}

impl Drop for DirCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ---- slugify --------------------------------------------------------

    #[test]
    fn keeps_ascii_words_and_hyphenates_spaces() {
        assert_eq!(slugify("All The Mods 10", "instance"), "All-The-Mods-10");
    }

    #[test]
    fn preserves_original_case() {
        assert_eq!(slugify("MyPack", "instance"), "MyPack");
    }

    #[test]
    fn trims_and_collapses_whitespace() {
        assert_eq!(slugify("  Trim   Me  ", "instance"), "Trim-Me");
    }

    #[test]
    fn replaces_windows_forbidden_and_path_separators() {
        assert_eq!(slugify(r#"My/Bad\Name:*?"#, "instance"), "My-Bad-Name");
    }

    #[test]
    fn drops_cyrillic_letters() {
        // PRODUCT DECISION, not an incidental edit: this test previously asserted
        // `Мой-сервер` survived. Directory names must be ASCII because the Windows
        // JVM launcher decodes argv through the system ANSI code page, so a name
        // the code page cannot express reaches java.exe as `?` and kills the
        // launch. The readability job moves one level up, to `derive_base`.
        assert_eq!(slugify("Мой сервер", "server"), "server");
    }

    #[test]
    fn drops_cjk_and_keeps_the_latin_part() {
        assert_eq!(
            slugify("红石生电优化【Redstone Survival Optimization】", "instance"),
            "Redstone-Survival-Optimization"
        );
    }

    #[test]
    fn drops_precomposed_diacritics() {
        // Precomposed `é` has no ASCII form here; `derive_base` transliterates.
        assert_eq!(slugify("café", "instance"), "caf");
    }

    #[test]
    fn dots_become_dashes() {
        // No dots may survive (reserved-with-extension / `..` safety).
        assert_eq!(slugify("Fabric 1.20.4", "instance"), "Fabric-1-20-4");
    }

    #[test]
    fn keeps_underscores() {
        assert_eq!(slugify("My_Cool_Pack", "instance"), "My_Cool_Pack");
    }

    #[test]
    fn empty_and_punctuation_only_fall_back() {
        assert_eq!(slugify("", "instance"), "instance");
        assert_eq!(slugify("   ", "instance"), "instance");
        assert_eq!(slugify("...", "instance"), "instance");
        assert_eq!(slugify("@#$", "server"), "server");
    }

    #[test]
    fn emoji_only_falls_back() {
        assert_eq!(slugify("🎮🎮🎮", "instance"), "instance");
    }

    #[test]
    fn drops_decomposed_combining_marks() {
        // "café" as c a f e + U+0301 (combining acute) → the mark is dropped,
        // the base letter is kept. (Precomposed é would be kept as-is.)
        assert_eq!(slugify("cafe\u{0301}", "instance"), "cafe");
    }

    #[test]
    fn strips_zero_width_and_bidi_controls() {
        // ZWJ (U+200D) and RLO (U+202E) are format chars → not alphanumeric.
        assert_eq!(slugify("a\u{200D}\u{202E}b", "instance"), "a-b");
    }

    #[test]
    fn caps_at_max_len_without_trailing_dash() {
        let long = "a".repeat(40);
        let s = slugify(&long, "instance");
        assert_eq!(s.chars().count(), MAX_SLUG_LEN);
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn cap_that_lands_on_a_separator_is_trimmed() {
        // 31 'a's then spaces then more: the cut can land mid-separator-run.
        let name = format!("{} tail words here", "a".repeat(31));
        let s = slugify(&name, "instance");
        assert!(s.chars().count() <= MAX_SLUG_LEN);
        assert!(!s.starts_with('-') && !s.ends_with('-'));
    }

    // ---- derive_base ----------------------------------------------------

    #[test]
    fn latin_residue_wins_when_substantial() {
        assert_eq!(
            derive_base(
                "红石生电优化【Redstone Survival Optimization】",
                None,
                "instance"
            ),
            "Redstone-Survival-Optimization"
        );
    }

    #[test]
    fn digits_only_residue_is_not_substantial() {
        // "Сборка 1.20 с шейдерами" reduces to "1-20" — looks like a name, means
        // nothing. Three ASCII LETTERS are required; digits do not count.
        let got = derive_base("Сборка 1.20 с шейдерами", None, "instance");
        assert_ne!(got, "1-20");
        assert!(
            got.starts_with("Sborka"),
            "expected transliteration, got {got}"
        );
    }

    #[test]
    fn pack_slug_beats_transliteration() {
        assert_eq!(
            derive_base(
                "红石生电优化",
                Some("redstone-survival-optimization"),
                "instance"
            ),
            "redstone-survival-optimization"
        );
    }

    #[test]
    fn transliterates_when_no_residue_and_no_slug() {
        assert_eq!(derive_base("Моя сборка", None, "instance"), "Moia-sborka");
    }

    #[test]
    fn cjk_transliterates_when_no_slug() {
        let got = derive_base("红石生电优化", None, "instance");
        assert!(
            got.starts_with("Hong"),
            "expected pinyin-ish romanisation, got {got}"
        );
        assert!(got.is_ascii());
    }

    #[test]
    fn emoji_only_still_falls_back_to_base() {
        assert_eq!(derive_base("🎮🎮🎮", None, "instance"), "instance");
    }

    #[test]
    fn derive_base_output_is_always_ascii_and_capped() {
        for name in [
            "红石生电优化【Redstone Survival Optimization】",
            "Моя сборка",
            "🎮",
            "",
        ] {
            let got = derive_base(name, None, "instance");
            assert!(got.is_ascii(), "{name} produced non-ASCII: {got}");
            assert!(got.chars().count() <= MAX_SLUG_LEN);
            assert!(!got.starts_with('-') && !got.ends_with('-'));
        }
    }

    // ---- is_reserved ----------------------------------------------------

    #[test]
    fn reserved_device_names_detected_case_insensitive() {
        for n in [
            "con", "CON", "Con", "prn", "aux", "nul", "com1", "com9", "lpt1", "LPT9",
        ] {
            assert!(is_reserved(n), "{n} should be reserved");
        }
    }

    #[test]
    fn non_reserved_lookalikes_pass() {
        for n in [
            "con-old",
            "console",
            "com0",
            "lpt0",
            "com10",
            "comm",
            "nul2",
            "communism",
        ] {
            assert!(!is_reserved(n), "{n} should NOT be reserved");
        }
    }

    // ---- reserve_unique_dir --------------------------------------------

    #[test]
    fn reserves_base_then_suffixes_on_collision() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        let (a, pa) = reserve_unique_dir(&parent, "My Pack", None, "instance").unwrap();
        let (b, pb) = reserve_unique_dir(&parent, "My Pack", None, "instance").unwrap();
        assert_eq!(a, "My-Pack");
        assert_eq!(b, "My-Pack-2");
        assert!(pa.is_dir() && pb.is_dir());
    }

    #[test]
    fn creates_parent_when_missing() {
        let dir = tempdir().unwrap();
        // `instances/` deliberately does not exist yet.
        let parent = dir.path().join("instances");
        let (name, path) = reserve_unique_dir(&parent, "First", None, "instance").unwrap();
        assert_eq!(name, "First");
        assert!(path.is_dir());
    }

    #[test]
    fn collision_check_is_case_insensitive_even_on_linux() {
        // Linux CI is case-sensitive: without an explicit lowercase check,
        // create_dir("test") would succeed alongside "Test". Assert the
        // consistent cross-platform behaviour (second → suffixed).
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        let (a, _) = reserve_unique_dir(&parent, "Test", None, "instance").unwrap();
        let (b, _) = reserve_unique_dir(&parent, "test", None, "instance").unwrap();
        assert_eq!(a, "Test");
        assert_eq!(b, "test-2");
    }

    #[test]
    fn reserved_base_skips_to_suffix() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        let (name, _) = reserve_unique_dir(&parent, "CON", None, "instance").unwrap();
        assert_eq!(name, "CON-2");
    }

    #[test]
    fn probe_first_refills_a_deleted_middle_suffix() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        std::fs::create_dir_all(parent.join("Pack")).unwrap();
        std::fs::create_dir_all(parent.join("Pack-2")).unwrap();
        std::fs::create_dir_all(parent.join("Pack-4")).unwrap();
        // First free is 3, not 5 (probe-first, not max+1).
        let (name, _) = reserve_unique_dir(&parent, "Pack", None, "instance").unwrap();
        assert_eq!(name, "Pack-3");
    }

    #[test]
    fn user_named_pack_2_does_not_clobber_an_unrelated_suffix() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        let (p, _) = reserve_unique_dir(&parent, "Pack", None, "instance").unwrap();
        let (p2, _) = reserve_unique_dir(&parent, "Pack-2", None, "instance").unwrap();
        assert_eq!(p, "Pack");
        assert_eq!(p2, "Pack-2");
        // A second "Pack" must skip the occupied "Pack-2" and land on "Pack-3".
        let (p3, _) = reserve_unique_dir(&parent, "Pack", None, "instance").unwrap();
        assert_eq!(p3, "Pack-3");
    }

    #[test]
    fn all_emoji_names_share_fallback_base_and_suffix() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("instances");
        let (a, _) = reserve_unique_dir(&parent, "🎮", None, "instance").unwrap();
        let (b, _) = reserve_unique_dir(&parent, "🎲", None, "instance").unwrap();
        assert_eq!(a, "instance");
        assert_eq!(b, "instance-2");
    }

    #[test]
    fn returned_name_matches_created_directory() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("servers");
        let (name, path) = reserve_unique_dir(&parent, "My Server", None, "server").unwrap();
        assert_eq!(path, parent.join(&name));
        assert!(path.is_dir());
    }

    #[test]
    fn dir_cleanup_removes_directory_on_drop() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("instances/orphan");
        std::fs::create_dir_all(&target).unwrap();
        {
            let _guard = DirCleanup::new(&target);
            // dropped here without keep() → directory removed
        }
        assert!(!target.exists(), "un-kept guard must remove the directory");
    }

    #[test]
    fn dir_cleanup_keeps_directory_when_disarmed() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("instances/committed");
        std::fs::create_dir_all(&target).unwrap();
        let guard = DirCleanup::new(&target);
        guard.keep();
        assert!(target.exists(), "kept guard must retain the directory");
    }
}
