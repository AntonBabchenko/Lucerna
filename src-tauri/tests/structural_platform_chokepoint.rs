//! Structural guard: OS-divergent primitives are confined to `platform::`.
//! `PermissionsExt` (exec bits), `WaitForInputIdle` (window detect),
//! `libc::kill` (process signal), and the registry-mutating `Reg*` call
//! families (GPU-preference writes, `lucerna://` scheme registration) must
//! not appear outside `src/platform/`, so
//! adding macOS later means editing one module — not hunting the codebase.
//! Subprocess spawning is governed separately by structural_no_raw_spawn.rs.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// OS-divergent primitives, matched as plain substrings. The `Reg*` entries
/// are PREFIX FAMILIES, not exact API names: Win32 spells each registry
/// mutation several ways (`RegCreateKeyW`, `RegCreateKeyExW`,
/// `RegCreateKeyTransactedW`, ...), and an exact-name list lets every variant
/// it forgot slip through — `contains()` on `RegCreateKeyW` does not match
/// `RegCreateKeyExW`. Only the MUTATING families are listed; the read side
/// (`RegOpenKeyExW`, `RegQueryValueExW`, `RegCloseKey`, `RegEnumKeyExW`)
/// stays out of scope per the module doc, which names the writes.
const NEEDLES: &[&str] = &[
    "PermissionsExt",
    "WaitForInputIdle",
    "libc::kill",
    "RegCreateKey",
    "RegSetValue",
    // `RegSetKeyValueW` does NOT contain the substring `RegSetValue` — the
    // `Key` in the middle breaks it — so this family needs its own entry.
    "RegSetKeyValue",
    // Also covers `RegDeleteKeyValueW` by substring.
    "RegDeleteKey",
    "RegDeleteValue",
    // URL-scheme registration (platform::protocol) deletes its whole key
    // tree; listed so a future caller can't move that write out of platform::.
    "RegDeleteTree",
];

/// Which platform primitive family, if any, `line` names.
fn primitive_on(line: &str) -> Option<&'static str> {
    NEEDLES.iter().copied().find(|n| line.contains(n))
}

#[test]
fn platform_primitives_confined_to_platform_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let platform_dir = src.join("platform");

    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        if file.starts_with(&platform_dir) {
            continue; // the chokepoint module is allowed these primitives
        }
        let content = fs::read_to_string(&file).expect("read rust file");
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // skip comments
            }
            if let Some(needle) = primitive_on(line) {
                violations.push(format!("{}:{} ({needle})", file.display(), i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "OS-divergent primitive used outside platform::\n{}",
        violations.join("\n"),
    );
}

/// The matchers, pinned directly. No Ex/Transacted/KeyValue registry variant
/// exists in the tree (platform:: uses the plain `W` names), so the scan
/// alone cannot prove the families match. Same rationale as the `matchers`
/// module in `structural_no_blind_err_swallow.rs`.
#[cfg(test)]
mod matchers {
    use super::*;

    #[test]
    fn ex_and_keyvalue_registry_variants_are_matched() {
        assert_eq!(
            primitive_on(
                "        let rc = RegCreateKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), &mut hkey);"
            ),
            Some("RegCreateKey"),
        );
        assert_eq!(
            primitive_on(
                "        let rc = RegSetKeyValueW(hkey, std::ptr::null(), name.as_ptr(), REG_SZ, data, len);"
            ),
            Some("RegSetKeyValue"),
        );
        assert_eq!(
            primitive_on("        let rc = RegDeleteKeyW(HKEY_CURRENT_USER, subkey.as_ptr());"),
            Some("RegDeleteKey"),
        );
    }

    #[test]
    fn the_exact_w_names_still_match_their_family() {
        // The four spellings the old list caught must stay caught.
        assert_eq!(
            primitive_on("        let rc = RegSetValueExW("),
            Some("RegSetValue")
        );
        assert_eq!(
            primitive_on("            let rc = RegDeleteValueW(hkey, name.as_ptr());"),
            Some("RegDeleteValue"),
        );
        assert_eq!(
            primitive_on(
                "        let rc = RegCreateKeyW(HKEY_CURRENT_USER, subkey.as_ptr(), &mut hkey);"
            ),
            Some("RegCreateKey"),
        );
        assert_eq!(
            primitive_on(
                "    let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, subkey.as_ptr()) };"
            ),
            Some("RegDeleteTree"),
        );
    }

    #[test]
    fn registry_reads_stay_out_of_scope() {
        // The module doc scopes the registry clause to MUTATIONS; the read
        // side must not start flagging.
        assert_eq!(
            primitive_on(
                "        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, KEY_READ, &mut hkey)"
            ),
            None,
        );
        assert_eq!(primitive_on("        let rc = RegQueryValueExW("), None);
        assert_eq!(primitive_on("            RegCloseKey(hkey);"), None);
        assert_eq!(
            primitive_on("                let rc = RegEnumKeyExW("),
            None
        );
    }
}
