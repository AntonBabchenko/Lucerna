//! Rewriting `.minecraft/options.txt` to enable or disable Lucerna's
//! generated translation resource pack.
//!
//! This is the single riskiest write in the launcher: `options.txt` holds
//! EVERY one of the user's game settings — keybinds, video, audio, language,
//! all of it — and nothing in this project has ever written it before; it
//! has only ever been copied whole. A careless rewrite here silently
//! destroys a user's entire configuration, and they would not notice until
//! they next launched the game. The rewrite algorithm below mirrors
//! `I18nUpdateMod`'s `GameConfig.addResourcePack` (seven million installs
//! behind it) rather than inventing a fresh one.
//!
//! # Priority and the `file/` prefix
//!
//! `FallbackResourceManager.getResource()` iterates
//! `for (int i = size() - 1; i >= 0; --i)` and returns the first hit, so the
//! LAST entry in `resourcePacks` wins. `GuiScreenResourcePacks` builds the
//! "Selected" panel from `Lists.reverse(...)` and reverses again on save, so
//! the in-game list shows the highest-priority pack at the TOP even though
//! the file stores it LAST — `"vanilla"`, always the file's first element, is
//! the anchor that proves the direction. Our own entry is therefore always
//! APPENDED, never prepended.
//!
//! MC ≥ 1.13 additionally prefixes user pack ids with `file/` (a real
//! instance reads `["vanilla","fabric","mod_resources","file/Foo.zip"]`);
//! older versions do not. `file_prefix` selects which convention to write.
//!
//! # Self-healing drift, and the one direction it does not heal
//!
//! The game silently drops `resourcePacks` entries that fail to resolve and
//! rewrites the list on its own next save, so a dangling reference left
//! behind by (say) a deleted pack file is not something this module needs to
//! defend against. `mods/modpack/overrides.rs` can cause drift in the OTHER
//! direction, though: a modpack update extracts the pack's own
//! `overrides/options.txt` verbatim over this file, which can wipe our
//! `resourcePacks` entry while leaving the generated pack file itself on
//! disk. [`pack_state`] exists to detect exactly that split so the UI can
//! offer to re-enable.
//!
//! # Ownership
//!
//! Minecraft rewrites `options.txt` wholesale on exit, so a write from here
//! while the instance is running is guaranteed to be discarded at best and
//! racing the game's own write at worst — [`update_atomically`] refuses
//! while the instance is running (see its doc comment for why it takes a
//! plain flag rather than an instance id).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use specta::Type;

/// Filename prefix identifying a pack Lucerna generated. Matching the whole
/// prefix (not one exact filename) is what makes switching target language
/// correct: the stale ru_ru entry must go when a de_de pack replaces it.
pub const PACK_PREFIX: &str = "lucerna-translation-";

/// MC ≥ 1.13's prefix for user-supplied resource pack ids in `resourcePacks`.
const FILE_PREFIX: &str = "file/";

/// The one key this module ever reads or writes.
const RESOURCE_PACKS_KEY: &str = "resourcePacks";

// ---------------------------------------------------------------------
// Lossless line-oriented parsing
// ---------------------------------------------------------------------

/// Which line terminator `options.txt` uses. Minecraft's `PrintWriter` picks
/// one separator (`System.lineSeparator()`) for the entire file and uses it
/// consistently — CRLF on Windows, LF on Linux/macOS — never mixed within a
/// single write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

/// One physical line, preserved losslessly. Splitting on the FIRST `:` only
/// (via `str::split_once`) is what keeps a value containing its own `:`
/// (e.g. `lastServer:127.0.0.1:25565`) intact rather than truncated at the
/// second colon.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Pair(String, String),
    /// No colon found (a blank line, or a corrupted one). Reproduced
    /// byte-for-byte on render — this is what makes "preserve every other
    /// line verbatim, including keys we do not understand" hold even for
    /// lines this module cannot interpret at all, not just ones it chooses
    /// not to touch.
    Other(String),
}

impl Line {
    fn render(&self) -> String {
        match self {
            Line::Pair(k, v) => format!("{k}:{v}"),
            Line::Other(s) => s.clone(),
        }
    }
}

/// A parsed `options.txt`: ordered lines plus enough information to
/// reproduce the original byte stream when nothing changes.
///
/// Line-ending strategy: split on bare `'\n'` and strip a trailing `'\r'`
/// from each resulting segment, then pick ONE ending for the whole document
/// — CRLF if any segment carried a `'\r'`, LF otherwise. For any file with a
/// single consistent terminator (100% of files Minecraft itself writes) this
/// round-trips byte-for-byte: `render(parse(src)) == src` whenever no
/// transform touches a line (see the `parsing_with_no_transform_is_the_identity_for_*`
/// tests below). A hand-edited file with genuinely MIXED endings is
/// normalised to one style rather than reproduced exactly — a defensible,
/// non-corrupting best effort for a case Minecraft's own writer can never
/// produce, and strictly better than the alternative bug this design avoids:
/// an earlier draft that split on `"\r\n"` as a literal separator would, on
/// a mixed file, silently fold a bare trailing `'\n'` into the last line's
/// VALUE and then append a spurious extra terminator on render.
struct ParsedOptions {
    lines: Vec<Line>,
    ending: LineEnding,
    trailing_newline: bool,
}

impl ParsedOptions {
    fn parse(src: &str) -> Self {
        let trailing_newline = src.ends_with('\n');
        let body = src.strip_suffix('\n').unwrap_or(src);
        let raw_lines: Vec<&str> = if body.is_empty() && !trailing_newline {
            Vec::new()
        } else {
            body.split('\n').collect()
        };

        let ending = if raw_lines.iter().any(|l| l.ends_with('\r')) {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };

        let lines = raw_lines
            .into_iter()
            .map(|raw| {
                let raw = raw.strip_suffix('\r').unwrap_or(raw);
                match raw.split_once(':') {
                    Some((k, v)) => Line::Pair(k.to_string(), v.to_string()),
                    None => Line::Other(raw.to_string()),
                }
            })
            .collect();

        Self {
            lines,
            ending,
            trailing_newline,
        }
    }

    fn render(&self) -> String {
        let sep = self.ending.as_str();
        let mut out = self
            .lines
            .iter()
            .map(Line::render)
            .collect::<Vec<_>>()
            .join(sep);
        if self.trailing_newline {
            out.push_str(sep);
        }
        out
    }

    /// The value of the first line whose key matches, if any.
    fn get_first(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| match l {
            Line::Pair(k, v) if k.as_str() == key => Some(v.as_str()),
            _ => None,
        })
    }

    /// Overwrite the first matching key's value in place, or append a new
    /// `key:value` line at the end when no line has that key yet.
    fn set_or_append(&mut self, key: &str, value: String) {
        let existing = self.lines.iter_mut().find_map(|l| match l {
            Line::Pair(k, v) if k.as_str() == key => Some(v),
            _ => None,
        });
        match existing {
            Some(slot) => *slot = value,
            None => self.lines.push(Line::Pair(key.to_string(), value)),
        }
    }

    /// `resourcePacks`, defaulting to empty when the key is absent OR its
    /// value fails to parse as a JSON array of strings — a malformed value
    /// is treated the same as a missing one, never propagated.
    fn resource_packs(&self) -> Vec<String> {
        self.get_first(RESOURCE_PACKS_KEY)
            .and_then(|v| serde_json::from_str::<Vec<String>>(v).ok())
            .unwrap_or_default()
    }

    fn set_resource_packs(&mut self, packs: &[String]) {
        // A `Vec<String>` cannot fail to serialize (no floats, no non-UTF8
        // content possible in a Rust `String`); the fallback exists only so
        // this stays total rather than needing an `unwrap()` justification.
        let json = serde_json::to_string(packs).unwrap_or_else(|_| "[]".to_string());
        self.set_or_append(RESOURCE_PACKS_KEY, json);
    }
}

/// Whether `entry` (a raw `resourcePacks` array element, with or without the
/// MC ≥ 1.13 `file/` prefix) names a pack Lucerna generated.
fn is_ours(entry: &str) -> bool {
    entry
        .strip_prefix(FILE_PREFIX)
        .unwrap_or(entry)
        .starts_with(PACK_PREFIX)
}

// ---------------------------------------------------------------------
// Public transforms
// ---------------------------------------------------------------------

/// Rewrite `options.txt` TEXT with our pack enabled, returning the new text.
/// `file_prefix` is true for MC >= 1.13, which stores user pack ids as
/// `file/<name>` rather than a bare filename.
///
/// Returns a plain `String`, not `Option<String>`: parsing here is total — a
/// missing or malformed `resourcePacks` value defaults to an empty array
/// rather than failing, and any line without a colon is preserved verbatim
/// rather than rejected — so this function has no failure path. An `Option`
/// would only ever be `Some`, forcing every caller to unwrap a case that can
/// never be `None`.
pub fn with_pack_enabled(src: &str, pack_filename: &str, file_prefix: bool) -> String {
    let mut doc = ParsedOptions::parse(src);
    let mut packs = doc.resource_packs();
    packs.retain(|p| !is_ours(p.as_str()));
    packs.push(if file_prefix {
        format!("{FILE_PREFIX}{pack_filename}")
    } else {
        pack_filename.to_string()
    });
    doc.set_resource_packs(&packs);
    doc.render()
}

/// Rewrite with every Lucerna-generated pack removed, returning the new
/// text. A no-op — byte-identical output — when `resourcePacks` never
/// existed in `src`: disabling must not conjure a key the file never had.
/// When the key DOES exist (well-formed or not), it is always rewritten in
/// valid form, which is also how a malformed value gets healed rather than
/// propagated.
pub fn with_pack_disabled(src: &str) -> String {
    let mut doc = ParsedOptions::parse(src);
    if doc.get_first(RESOURCE_PACKS_KEY).is_none() {
        return doc.render();
    }
    let mut packs = doc.resource_packs();
    packs.retain(|p| !is_ours(p.as_str()));
    doc.set_resource_packs(&packs);
    doc.render()
}

/// Whether a Lucerna-generated pack is currently listed (in any position) in
/// `resourcePacks`.
pub fn is_pack_enabled(src: &str) -> bool {
    ParsedOptions::parse(src)
        .resource_packs()
        .iter()
        .any(|p| is_ours(p.as_str()))
}

// ---------------------------------------------------------------------
// State
// ---------------------------------------------------------------------

/// Whether the generated pack is on disk and whether the game will load it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PackState {
    /// No generated pack in the instance.
    NotApplied,
    /// Pack file exists but options.txt does not list it — the state a
    /// modpack update leaves behind. The UI offers to re-enable.
    PresentNotEnabled,
    /// Pack file exists and there is no `options.txt` at all: the instance has
    /// never been launched, so Minecraft has not written one yet.
    ///
    /// A separate variant rather than a flavour of `PresentNotEnabled`,
    /// because the two differ in the only way that matters to the user — the
    /// remedy. Re-applying fixes a wiped entry; it can do NOTHING here, since
    /// `update_atomically` returns `Ok(false)` on a missing file and never
    /// creates one. Collapsing them made the UI offer a button that could not
    /// succeed, under a message blaming a modpack update that never happened.
    PresentAwaitingLaunch,
    Enabled,
}

/// Classify pack state from the two independent facts: whether the generated
/// pack file exists on disk, and what `options.txt` currently says. Absence
/// of the file always wins to `NotApplied` regardless of what a stale
/// `options.txt` claims — there is nothing to load either way, so reporting
/// anything else would describe a pack that is not there.
///
/// `Option` rather than a string the caller defaulted to empty: "no file" and
/// "a file that does not list the pack" give `is_pack_enabled` the same answer
/// and need different remedies, so the distinction has to survive to here.
pub fn pack_state(pack_on_disk: bool, options_txt: Option<&str>) -> PackState {
    if !pack_on_disk {
        return PackState::NotApplied;
    }
    let Some(options_txt) = options_txt else {
        return PackState::PresentAwaitingLaunch;
    };
    if is_pack_enabled(options_txt) {
        PackState::Enabled
    } else {
        PackState::PresentNotEnabled
    }
}

// ---------------------------------------------------------------------
// Atomic, guarded write
// ---------------------------------------------------------------------

/// Pure predicate over a snapshot the caller already has — mirrors
/// `datapacks::guard::datapack_write_allowed`. `mc_dir` alone does not carry
/// an instance id (there is no cheap, reliable way to walk a `.minecraft`
/// path back to the id `launch`'s process registry is keyed by), and
/// `launch::registry()` is private with no cross-module seam, so this
/// decision can only ever be exercised end-to-end, never faked in a unit
/// test — exactly like the datapack guard. Kept as its own function (rather
/// than inlined into `update_atomically`) purely so that decision is
/// independently testable without touching a filesystem.
fn options_write_allowed(is_running: bool) -> Result<(), crate::error::Error> {
    if is_running {
        return Err(crate::error::Error::InstanceBusy);
    }
    Ok(())
}

/// Monotonic per-process sequence, combined with the pid, to make every
/// `update_atomically` temp file name unique. A bare `tmp.<pid>` (what
/// `l10n::coverage::ScanCache::save` uses) is only safe because
/// `ScanCache::update` holds `CACHE_DISK_LOCK` for the whole
/// read-modify-write cycle, serialising every call in this process onto one
/// path. Nothing plays that role here, and a mutex that would is the wrong
/// shape for this problem: a single global lock would serialise every
/// instance's options.txt writes against every other instance's for no
/// reason, and a lock keyed per-`mc_dir` is just this sequence number with
/// extra steps. `mods/installed.rs` hit precisely the gap a bare pid leaves
/// open: two concurrent writers shared one `tmp.<pid>` name, and the second's
/// rename failed with "cannot find the file" because the first had already
/// consumed it. A unique suffix per call removes the collision outright —
/// see `concurrent_updates_do_not_race_on_the_temp_file_name` below.
static OPTIONS_WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Read, transform and write `<mc_dir>/options.txt` atomically. Refuses
/// while the instance is running. `Ok(false)` when the file does not exist —
/// activation is deferred rather than the launcher inventing the game's
/// settings file — or when `transform` returns `None`, meaning it decided
/// there was nothing to change; `Ok(true)` iff a write actually happened.
///
/// Takes `is_running` as a plain flag, not an instance id, and folds the
/// guard directly into this signature rather than leaving it as a separate
/// pre-flight call a future caller could forget to make: `options.txt` is
/// the single highest-consequence file this launcher ever touches (see the
/// module doc comment), so the one function that writes it should not
/// compile without an answer to "is the instance running". The caller — the
/// command layer, which already has both the instance id and `mc_dir` — is
/// expected to compute `launch::spawn::is_running(instance_id)` and pass the
/// snapshot in: the snapshot-in shape `datapacks::guard::datapack_write_allowed`
/// also takes. (The world and datapack commands themselves now open with
/// `instances::maintenance::write_allowed(id)`, which resolves the snapshot.)
pub fn update_atomically<F>(
    mc_dir: &Path,
    is_running: bool,
    transform: F,
) -> Result<bool, crate::error::Error>
where
    F: FnOnce(&str) -> Option<String>,
{
    options_write_allowed(is_running)?;

    let path = mc_dir.join("options.txt");
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(crate::error::Error::io(path.display().to_string(), e)),
    };

    let Some(new_content) = transform(&src) else {
        return Ok(false);
    };

    let seq = OPTIONS_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("txt.tmp.{}.{seq}", std::process::id()));
    std::fs::write(&tmp, new_content.as_bytes())
        .map_err(|e| crate::error::Error::io(tmp.display().to_string(), e))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        // Don't leave a stale options.txt.tmp.* behind on a failed rename.
        let _ = std::fs::remove_file(&tmp);
        return Err(crate::error::Error::io(path.display().to_string(), e));
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A representative real `options.txt` slice: several ordinary keys,
    /// one (`lastServer`) whose value itself contains a colon, and a
    /// `resourcePacks` array with just `"vanilla"`.
    const SAMPLE: &str = concat!(
        "version:3465\n",
        "sound_category.master:1.0\n",
        "invertYMouse:false\n",
        "fullscreen:false\n",
        "lang:ru_ru\n",
        "resourcePacks:[\"vanilla\"]\n",
        "guiScale:0\n",
        "lastServer:127.0.0.1:25565\n",
    );

    fn packs_of(src: &str) -> Vec<String> {
        let doc = ParsedOptions::parse(src);
        doc.resource_packs()
    }

    // -----------------------------------------------------------------
    // with_pack_enabled
    // -----------------------------------------------------------------

    #[test]
    fn appends_the_pack_last_and_leaves_vanilla_first() {
        let out = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", false);
        let packs = packs_of(&out);
        assert_eq!(packs[0], "vanilla");
        assert_eq!(packs.last().unwrap(), "lucerna-translation-ru_ru.zip");
    }

    #[test]
    fn every_other_key_and_its_order_is_preserved() {
        let out = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        let out_lines: Vec<&str> = out.lines().collect();
        let sample_lines: Vec<&str> = SAMPLE.lines().collect();
        assert_eq!(out_lines.len(), sample_lines.len());
        for (o, s) in out_lines.iter().zip(sample_lines.iter()) {
            if s.starts_with("resourcePacks:") {
                assert!(o.starts_with("resourcePacks:"));
            } else {
                // Every non-resourcePacks line must be byte-identical,
                // including its position.
                assert_eq!(o, s);
            }
        }
    }

    #[test]
    fn a_value_containing_a_colon_is_not_split_twice() {
        // lastServer:127.0.0.1:25565 must survive completely unchanged —
        // a naive split-on-every-colon would truncate it at the first one.
        let out = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", false);
        assert!(out.contains("lastServer:127.0.0.1:25565"));
    }

    #[test]
    fn applying_twice_is_idempotent() {
        let once = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        let twice = with_pack_enabled(&once, "lucerna-translation-ru_ru.zip", true);
        assert_eq!(once, twice);
    }

    #[test]
    fn switching_target_language_replaces_rather_than_accumulates() {
        let ru = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        let de = with_pack_enabled(&ru, "lucerna-translation-de_de.zip", true);
        let packs = packs_of(&de);
        let ours: Vec<&String> = packs.iter().filter(|p| is_ours(p.as_str())).collect();
        assert_eq!(ours, vec!["file/lucerna-translation-de_de.zip"]);
    }

    #[test]
    fn file_prefix_is_applied_only_when_requested() {
        let with_prefix = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        assert!(packs_of(&with_prefix)
            .iter()
            .any(|p| p == "file/lucerna-translation-ru_ru.zip"));

        let without_prefix = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", false);
        assert!(packs_of(&without_prefix)
            .iter()
            .any(|p| p == "lucerna-translation-ru_ru.zip"));
    }

    #[test]
    fn a_missing_resource_packs_key_is_created() {
        let src = "version:3465\nlang:en_us\n";
        let out = with_pack_enabled(src, "lucerna-translation-ru_ru.zip", false);
        assert_eq!(packs_of(&out), vec!["lucerna-translation-ru_ru.zip"]);
        // The rest of the file is untouched, and the new key lands after it.
        assert!(out.starts_with("version:3465\nlang:en_us\n"));
    }

    #[test]
    fn a_malformed_resource_packs_value_is_replaced_not_propagated() {
        let src = "version:3465\nresourcePacks:not valid json\nlang:en_us\n";
        let out = with_pack_enabled(src, "lucerna-translation-ru_ru.zip", false);
        assert_eq!(packs_of(&out), vec!["lucerna-translation-ru_ru.zip"]);
        assert!(!out.contains("not valid json"));
    }

    // -----------------------------------------------------------------
    // with_pack_disabled
    // -----------------------------------------------------------------

    #[test]
    fn disabling_drops_only_our_entry_and_keeps_the_rest_in_order() {
        let src = concat!(
            "resourcePacks:[\"vanilla\",\"fabric\",",
            "\"file/lucerna-translation-ru_ru.zip\",\"mod_resources\"]\n"
        );
        let out = with_pack_disabled(src);
        assert_eq!(
            packs_of(&out),
            vec![
                "vanilla".to_string(),
                "fabric".to_string(),
                "mod_resources".to_string(),
            ]
        );
    }

    #[test]
    fn disabling_with_no_lucerna_entry_present_is_a_true_no_op() {
        let out = with_pack_disabled(SAMPLE);
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn disabling_when_resource_packs_never_existed_does_not_create_it() {
        let src = "version:3465\nlang:en_us\n";
        let out = with_pack_disabled(src);
        assert_eq!(out, src);
    }

    #[test]
    fn disable_after_enable_removes_the_entry_entirely() {
        let enabled = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        let disabled = with_pack_disabled(&enabled);
        assert_eq!(packs_of(&disabled), vec!["vanilla".to_string()]);
    }

    // -----------------------------------------------------------------
    // is_ours / is_pack_enabled
    // -----------------------------------------------------------------

    #[test]
    fn is_ours_matches_both_a_bare_and_a_file_prefixed_entry() {
        assert!(is_ours("lucerna-translation-ru_ru.zip"));
        assert!(is_ours("file/lucerna-translation-ru_ru.zip"));
    }

    #[test]
    fn is_ours_rejects_an_unrelated_entry_even_with_the_file_prefix() {
        assert!(!is_ours("file/FreshAnimations_v1.10.4.zip"));
        assert!(!is_ours("vanilla"));
    }

    #[test]
    fn is_pack_enabled_true_and_false_cases() {
        assert!(!is_pack_enabled(SAMPLE));
        let enabled = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        assert!(is_pack_enabled(&enabled));
        let disabled = with_pack_disabled(&enabled);
        assert!(!is_pack_enabled(&disabled));
    }

    // -----------------------------------------------------------------
    // line endings and trailing newline (Verify-1)
    // -----------------------------------------------------------------

    #[test]
    fn parsing_with_no_transform_is_the_identity_for_lf() {
        assert_eq!(ParsedOptions::parse(SAMPLE).render(), SAMPLE);
    }

    #[test]
    fn parsing_with_no_transform_is_the_identity_for_crlf() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        assert_eq!(ParsedOptions::parse(&crlf).render(), crlf);
    }

    #[test]
    fn parsing_with_no_transform_is_the_identity_without_a_trailing_newline() {
        let no_trailing = SAMPLE.trim_end_matches('\n');
        assert_eq!(ParsedOptions::parse(no_trailing).render(), no_trailing);
    }

    #[test]
    fn crlf_files_stay_crlf_after_enabling_the_pack() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        let out = with_pack_enabled(&crlf, "lucerna-translation-ru_ru.zip", true);
        // Every '\n' in the output is part of a "\r\n" pair — i.e. no bare
        // LF crept in — and the file still ends with one terminator (SAMPLE
        // has a trailing newline).
        assert_eq!(out.matches('\n').count(), out.matches("\r\n").count());
        assert!(out.ends_with("\r\n"));
    }

    #[test]
    fn lf_files_stay_lf_after_enabling_the_pack() {
        let out = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        assert!(!out.contains('\r'), "no CR should appear: {out}");
    }

    #[test]
    fn a_mixed_ending_file_normalises_without_dropping_or_duplicating_lines() {
        // Not a shape Minecraft's own writer can ever produce (one
        // PrintWriter, one separator, for the whole file) but defensively
        // exercised anyway: every logical line must survive, exactly once,
        // even though byte-for-byte preservation is not guaranteed here.
        let mixed = "a:1\r\nb:2\nc:3\r\n";
        let doc = ParsedOptions::parse(mixed);
        assert_eq!(doc.lines.len(), 3);
        let out = doc.render();
        assert!(out.contains("a:1"));
        assert!(out.contains("b:2"));
        assert!(out.contains("c:3"));
    }

    #[test]
    fn an_empty_options_txt_parses_and_renders_as_empty() {
        assert_eq!(ParsedOptions::parse("").render(), "");
    }

    // -----------------------------------------------------------------
    // pack_state (Verify-3 target)
    // -----------------------------------------------------------------

    #[test]
    fn pack_state_not_applied_when_the_file_is_missing_from_disk() {
        // Even if options.txt claims it's enabled, no file on disk means
        // there is nothing to load.
        let enabled = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        assert_eq!(pack_state(false, Some(&enabled)), PackState::NotApplied);
    }

    #[test]
    fn pack_state_present_not_enabled_when_disk_has_it_but_options_txt_does_not() {
        assert_eq!(pack_state(true, Some(SAMPLE)), PackState::PresentNotEnabled);
    }

    #[test]
    fn pack_state_awaiting_launch_when_there_is_no_options_txt_at_all() {
        // The instance has never been launched, so Minecraft has not written
        // options.txt yet. This is NOT the modpack-wipe state: re-applying
        // cannot fix it, because `update_atomically` never creates the file.
        assert_eq!(pack_state(true, None), PackState::PresentAwaitingLaunch);
    }

    #[test]
    fn pack_state_enabled_when_both_agree() {
        let enabled = with_pack_enabled(SAMPLE, "lucerna-translation-ru_ru.zip", true);
        assert_eq!(pack_state(true, Some(&enabled)), PackState::Enabled);
    }

    // -----------------------------------------------------------------
    // options_write_allowed (Verify-2 target)
    // -----------------------------------------------------------------

    #[test]
    fn a_stopped_instance_is_allowed_to_write() {
        assert!(options_write_allowed(false).is_ok());
    }

    #[test]
    fn a_running_instance_is_rejected() {
        assert!(matches!(
            options_write_allowed(true).unwrap_err(),
            crate::error::Error::InstanceBusy
        ));
    }

    // -----------------------------------------------------------------
    // update_atomically
    // -----------------------------------------------------------------

    #[test]
    fn returns_false_and_creates_nothing_when_options_txt_is_missing() {
        let dir = tempdir().unwrap();
        let mc_dir = dir.path().join(".minecraft");
        std::fs::create_dir_all(&mc_dir).unwrap();

        let wrote = update_atomically(&mc_dir, false, |s| {
            Some(with_pack_enabled(s, "lucerna-translation-ru_ru.zip", true))
        })
        .unwrap();

        assert!(!wrote);
        assert!(!mc_dir.join("options.txt").exists());
    }

    #[test]
    fn refuses_while_running_and_leaves_the_file_byte_identical() {
        let dir = tempdir().unwrap();
        let mc_dir = dir.path().join(".minecraft");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("options.txt"), SAMPLE).unwrap();

        let err = update_atomically(&mc_dir, true, |s| {
            Some(with_pack_enabled(s, "lucerna-translation-ru_ru.zip", true))
        })
        .unwrap_err();

        assert!(matches!(err, crate::error::Error::InstanceBusy));
        let on_disk = std::fs::read_to_string(mc_dir.join("options.txt")).unwrap();
        assert_eq!(on_disk, SAMPLE);
        // No leftover temp file from a guard that fired before ever
        // touching the filesystem.
        let leftover = std::fs::read_dir(&mc_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
        assert!(!leftover);
    }

    #[test]
    fn writes_the_transformed_content_and_reports_true() {
        let dir = tempdir().unwrap();
        let mc_dir = dir.path().join(".minecraft");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("options.txt"), SAMPLE).unwrap();

        let wrote = update_atomically(&mc_dir, false, |s| {
            Some(with_pack_enabled(s, "lucerna-translation-ru_ru.zip", true))
        })
        .unwrap();

        assert!(wrote);
        let on_disk = std::fs::read_to_string(mc_dir.join("options.txt")).unwrap();
        assert!(is_pack_enabled(&on_disk));
        assert!(on_disk.contains("lastServer:127.0.0.1:25565"));

        let leftover = std::fs::read_dir(&mc_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
        assert!(!leftover);
    }

    #[test]
    fn a_transform_that_declines_leaves_the_file_untouched_and_reports_false() {
        let dir = tempdir().unwrap();
        let mc_dir = dir.path().join(".minecraft");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("options.txt"), SAMPLE).unwrap();

        let wrote = update_atomically(&mc_dir, false, |_s| None).unwrap();

        assert!(!wrote);
        let on_disk = std::fs::read_to_string(mc_dir.join("options.txt")).unwrap();
        assert_eq!(on_disk, SAMPLE);
    }

    #[test]
    fn concurrent_updates_do_not_race_on_the_temp_file_name() {
        // Mirrors `mods::installed::concurrent_list_migration_does_not_race_
        // on_temp_file`: several threads writing the SAME options.txt at
        // once must not collide on the temp path and fail the rename. Every
        // call either succeeds or reports a real error — the process must
        // never panic and the file must never end up missing entirely.
        let dir = tempdir().unwrap();
        let mc_dir = dir.path().join(".minecraft");
        std::fs::create_dir_all(&mc_dir).unwrap();
        std::fs::write(mc_dir.join("options.txt"), SAMPLE).unwrap();

        let handles: Vec<_> = (0..16)
            .map(|i| {
                let thread_mc_dir = mc_dir.clone();
                std::thread::spawn(move || {
                    update_atomically(&thread_mc_dir, false, move |s| {
                        Some(with_pack_enabled(
                            s,
                            &format!("lucerna-translation-lang{i}.zip"),
                            true,
                        ))
                    })
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap().unwrap();
        }

        // The file must still exist and be a coherent options.txt with a
        // valid, parseable resourcePacks array — not truncated or missing.
        let on_disk = std::fs::read_to_string(mc_dir.join("options.txt")).unwrap();
        assert!(is_pack_enabled(&on_disk));
        assert!(on_disk.contains("lastServer:127.0.0.1:25565"));

        let leftover = std::fs::read_dir(&mc_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
        assert!(!leftover);
    }
}
