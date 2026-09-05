//! Structural guard: no raw file-write primitive under the three
//! hardlink-bearing trees — `src/mods/`, `src/datapacks/`, `src/worlds/` —
//! outside the modules that own writing there.
//!
//! ## Why these three trees, and no others
//!
//! Instance mod jars in `<instance>/.minecraft/mods/` are HARDLINKS to one
//! shared physical file in the mod cache; a world's datapack in
//! `saves/<world>/datapacks/` is a hardlink into the instance's datapack
//! library (see `datapacks::library`'s module doc); `src/worlds/` is the
//! module that writes into the very `saves/` tree those datapack links live
//! in (backup/restore/import). These three trees are the ones that write
//! hardlink-bearing content; every byte that lands there goes through
//! `mods::store` (temp file + rename, link or copy) or `mods::cache`
//! (temp file + rename, SHA-keyed).
//!
//! This is deliberately NOT widened to all of `src/`: `src/logs`,
//! `src/screenshots`, `src/instances`, `src/servers_runtime` and others
//! legitimately write files no other instance or world shares, so a
//! whole-tree scan would need a sprawling allowlist and the guard would stop
//! meaning anything. The guard's subject is hardlink-bearing instance
//! content, and exactly these three trees write there.
//!
//! `src/l10n/` was evaluated as a candidate fourth tree and rejected on the
//! same grounds. It writes into instance content (a generated translation
//! resource pack) and rewrites `options.txt`, but neither path is
//! hardlink-bearing:
//!
//! - the resource pack write happens in `commands::l10n::l10n_apply`, via
//!   `mods::asset_local::install_asset_local` -> `mods::store::place_bytes` —
//!   already inside the `mods` tree above, through the sanctioned sink — and
//!   that call site is not even under `src/l10n/` to begin with;
//! - `options.txt` is a plain per-instance file, never shared: it is already
//!   copied instance-to-instance with an ordinary `std::fs::copy` in
//!   `instances::clone` and `instances::import::pipeline::copy_category`,
//!   with no link concern, because there is no second name for any
//!   instance's `options.txt` to protect.
//!
//! `l10n`'s own on-disk writes (`l10n::store::save`,
//! `l10n::coverage::ScanCache::save`, `l10n::options_txt::update_atomically`)
//! already self-discipline with temp-then-rename on their own merits, but
//! none of them touch a path any other instance or world could be linked
//! to. Folding them in here would mean allowlisting three files against a
//! hardlink risk none of them carry — the same drift the paragraph above
//! already warns about, just at tree granularity instead of whole-`src/`
//! granularity. If `l10n` ever starts sharing physical bytes across
//! instances (the generated pack becoming a cached, linked file rather than
//! a per-instance `place_bytes` write; `options.txt` becoming linked rather
//! than copied), that changes this analysis and the tree belongs here.
//!
//! ## Why this is a build-breaking rule
//!
//! Opening a hardlinked path for writing — including `fs::copy`, whose
//! destination is opened with truncate — changes the bytes for EVERY name
//! sharing that physical file, and zeroes it before the first byte is
//! written. A plain `fs::copy` that would corrupt one instance or world
//! before dedup corrupts all of them after it. The only safe shape is
//! write-to-temp-then-rename, which replaces a directory entry and leaves
//! the other links intact.
//!
//! Removals are deliberately NOT covered: deleting one name cannot affect the
//! store entry or another instance's/world's link, so the uninstall/unlink
//! sites keep their plain `remove_file` / `remove_dir_all`.
//!
//! Guardrail, not a sandbox — same framing as `structural_no_raw_http.rs` and
//! `structural_no_raw_spawn.rs`. Its job is to make a NEW write into
//! hardlink-bearing content fail the build, so the author routes it through
//! `store` or justifies an addition to the allowlist below.

use std::fs;
use std::path::{Path, PathBuf};

/// The three trees scanned, relative to `src/`. See the module doc for why
/// exactly these three and no others.
const ROOTS: &[&str] = &["mods", "datapacks", "worlds"];

/// Files allowed to call write primitives directly, with the class of path
/// each one writes, relative to `src/`.
///
/// Adding a file here means stating which class of path it writes and why
/// that path can never be a hardlink shared between instances or worlds.
const ALLOWLIST: &[&str] = &[
    // The two owners of writing.
    "mods/store.rs", // instance side: temp + rename, link-or-copy
    "mods/cache.rs", // store side: temp + rename, SHA-keyed
    // Metadata and caches — JSON sidecars, never content bytes.
    "mods/installed.rs", // .lucerna/installed-mods.json (already temp + rename)
    // `{instance}/lucerna/jar-hashes.json` and its `json.tmp.<pid>.<seq>`
    // sibling — the per-instance `(mtime, size) -> sha1` memo that makes the
    // first installed-list after a launcher start stat-only. Writes no content
    // bytes: the path it builds is `installed::registry_dir(instance_root)`,
    // the instance's `lucerna/` METADATA directory, a SIBLING of `.minecraft/`,
    // so it can never resolve inside `.minecraft/mods/` or a world's
    // `datapacks/` and can never be one of the hardlinked names this guard
    // protects. Same class, same temp-then-rename shape, and the same per-write
    // sequence number as `mods/installed.rs` above.
    "mods/hash_cache.rs",
    "mods/summary_cache.rs", // mod summary JSON cache
    "mods/assets.rs",        // installed-assets registry JSON (temp + rename)
    // `<app_data>/mods-cache/jar-scans.json` and its `tmp.<pid>` sibling —
    // a global, cross-instance cache of PARSED DESCRIPTORS (families, provided
    // ids, declared deps), keyed by the jar's SHA-1. It writes no content
    // bytes: the path it builds comes from `paths::jar_scan_cache_file`, which
    // is rooted at the app-data dir, so it can never resolve inside any
    // instance's `.minecraft/mods/` or a world's `datapacks/` and can never be
    // one of the hardlinked names this guard protects. Same class and same
    // temp-then-rename shape as `mods/summary_cache.rs` above; it lives under
    // `src/mods/` only because the data it caches is a mod descriptor.
    "mods/jar_scan_cache.rs",
    // Paths outside any instance's content directories.
    "mods/dep_resolve.rs", // server mods dirs — out of scope, see the session spec
    "mods/modpack/export/assembly.rs", // the export zip being assembled
    "mods/modpack/source/stage.rs", // downloaded pack archive staging
    // `src/worlds/` — writes into the `saves/` tree, but never in place onto
    // a live world's own files or a datapack hardlink living inside it.
    //
    // `File::create` writes only: (1) a freshly timestamped backup `.zip`
    // under `backups/<world>/` (`zip_dir`, called from
    // `worlds::backup::backup_world` via `pick_unused_filename`, which
    // guarantees an unused destination name), and (2) an entry path inside
    // `extract_zip_capped`'s `dest_dir` — every production caller
    // (`worlds::restore`'s `restore_replace` / `restore_as_copy`,
    // `worlds::import`'s `import_from_zip`, and the unrelated
    // `servers_runtime` backup/import paths) extracts into a freshly
    // created staging or runtime directory and only `rename`s the verified
    // result into place afterward — never straight onto a live world or its
    // `datapacks/` folder. Verified by reading every call site.
    "worlds/zip.rs",
    // `std::fs::copy` in `copy_tree` has two callers, and both write only
    // into a directory that did not exist before the caller itself created
    // it:
    //
    // (1) `worlds::import::place_world` — `dest = saves.join(&chosen)`,
    //     where `chosen` is a name `pick_free_world_name` proved did NOT
    //     already exist on disk, so every path written is a fresh name
    //     under a brand-new world folder;
    // (2) `worlds::migrate` (the world-migration copy path) — a hidden
    //     stage `saves/.tmp-migrate-<world>-<n>` that migrate claimed with
    //     `create_dir` (`AlreadyExists` ⇒ next `n`, the `restore::claim_stage`
    //     shape) and later `rename`s to its final name; the stage is a
    //     directory only migrate has ever written into.
    //
    // Neither destination can be a pre-existing, possibly-hardlinked
    // datapack: this guard's subject is a write ONTO an existing link, and a
    // directory created moments earlier by the writer holds none. `copy_tree`
    // never follows symlinks, so it cannot be steered onto one either. A
    // third caller must keep the same discipline — a destination it created
    // itself — or this entry is no longer justified.
    "worlds/import.rs",
];

const PRIMITIVES: &[&str] = &[
    "fs::copy(",
    "fs::write(",
    "File::create(",
    "File::options",
    "OpenOptions",
];

/// `PRIMITIVES` matches call-site text, so it only sees the qualified form. A
/// plain `use tokio::fs::copy;` would let `copy(&cached, &out)` slip past with
/// no `fs::copy(` substring anywhere — not an adversarial trick, just an
/// ordinary import style. So importing one of these names unqualified is itself
/// a violation: keep the `fs::` prefix at the call site and the guard keeps
/// working.
fn imports_a_primitive_unqualified(line: &str) -> bool {
    let t = line.trim_start();
    if !t.starts_with("use ") || !t.contains("fs::") {
        return false;
    }
    // The item list is whatever follows the last `fs::` — `use std::fs::copy;`
    // or `use tokio::fs::{self, copy, write};`. A bare `use tokio::fs;` has no
    // `fs::` and never reaches here.
    let tail = t.rsplit("fs::").next().unwrap_or("");
    ["copy", "write", "File", "OpenOptions"].iter().any(|item| {
        tail.split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == *item)
    })
}

/// True for each line index inside a `#[cfg(test)] mod .. { }` region. The
/// detector is the attribute + `mod` PAIR with brace tracking — NOT "break at
/// the first `#[cfg(test)]`", which misclassifies the rest of any file whose
/// attribute sits on a helper fn or const (`mods/curseforge/keyring.rs:25` is
/// a live example), and NOT "skip to end of file", because production code
/// follows a mid-file test module in `mods/modpack/export/manifest.rs` and
/// `datapacks/compat.rs`. Residual risk, accepted and named: an unpaired `{`
/// or `}` inside a string literal inside a test module would end the region
/// early or late. No such line exists today.
fn test_region_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        let is_pair = t.starts_with("#[cfg(test)]")
            && lines
                .get(i + 1)
                .map(|n| {
                    let n = n.trim_start();
                    n.starts_with("mod ") || n.starts_with("pub mod ")
                })
                .unwrap_or(false);
        if !is_pair {
            i += 1;
            continue;
        }
        mask[i] = true;
        let mut depth = 0usize;
        let mut opened = false;
        let mut j = i + 1;
        while j < lines.len() {
            mask[j] = true;
            for c in lines[j].chars() {
                match c {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            if opened && depth == 0 {
                break;
            }
            j += 1;
        }
        i = j + 1;
    }
    mask
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

#[test]
fn no_raw_write_primitive_under_mods_datapacks_worlds_outside_the_owners() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for root in ROOTS {
        rust_files(&src_dir.join(root), &mut files);
    }

    let mut violations = Vec::new();
    for file in files {
        // Path relative to src/, normalised to forward slashes so the
        // allowlist matches on every platform.
        let rel = file
            .strip_prefix(&src_dir)
            .expect("file under src/")
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWLIST.contains(&rel.as_str()) {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        let lines: Vec<&str> = content.lines().collect();
        // Test-module interiors are exempt — their fixture writes are not
        // production writes — but production code before, between, or AFTER
        // them stays scanned. See `test_region_mask` for why this is a
        // region mask and not a break-at-first-`#[cfg(test)]`.
        let in_test = test_region_mask(&lines);
        for (i, line) in lines.iter().enumerate() {
            if in_test[i] {
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // doc comments naming a primitive are fine
            }
            if PRIMITIVES.iter().any(|p| line.contains(p)) || imports_a_primitive_unqualified(line)
            {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "raw write primitive under a hardlink-bearing tree (src/mods, src/datapacks, \
         src/worlds) outside mods::store / mods::cache.\n\
         Instance mod jars and world datapack links are hardlinks to ONE shared physical \
         file — writing such a path in place corrupts every instance or world sharing it, \
         and truncates it to zero before the first byte lands. Route the write through \
         `mods::store::materialize` / `mods::store::place_bytes`, or add the file to \
         ALLOWLIST stating the class of path it writes:\n{}",
        violations.join("\n"),
    );
}

#[test]
fn the_test_region_mask_survives_the_shapes_that_break_naive_detectors() {
    // A cfg(test) HELPER (the `mods/curseforge/keyring.rs:25` shape):
    // production code after the attribute must stay scanned. The old
    // first-`#[cfg(test)]`-line break got this wrong.
    let helper = [
        "#[cfg(test)]",
        "fn append() {}",
        "fn production(p: &std::path::Path) -> bool { p.exists() }",
    ];
    let mask = test_region_mask(&helper);
    assert!(
        !mask[2],
        "production after a cfg(test) helper must stay scanned"
    );

    // A mid-file test MODULE with production after it (the
    // `mods/modpack/export/manifest.rs:98` / `datapacks/compat.rs:58`
    // shape): skip-to-end-of-file would leave the tail unscanned.
    let midfile = [
        "#[cfg(test)]",
        "mod mrpack_tests {",
        "    #[test]",
        "    fn t() { fs::write(p, b\"x\").unwrap(); }",
        "}",
        "pub fn production() {}",
    ];
    let mask = test_region_mask(&midfile);
    assert!(mask[3], "code inside the test module is exempt");
    assert!(
        !mask[5],
        "production after the test module must stay scanned"
    );
}
