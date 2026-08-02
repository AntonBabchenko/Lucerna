//! `network::loopback` bypasses the host allowlist by construction — it dials
//! 127.0.0.1, which is deliberately NOT an allowlist entry. That is safe only
//! while exactly one feature can reach it. This guard fails the build if any
//! other module learns to call it.

use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn loopback_is_confined_to_the_prefill_feature() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = [src.join("network"), src.join("l10n").join("prefill")];
    let mut files = Vec::new();
    rs_files(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        if allowed.iter().any(|a| file.starts_with(a)) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (i, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("loopback::") {
                violations.push(format!("{}:{}", file.display(), i + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "network::loopback must stay confined to l10n::prefill; found: {violations:?}"
    );
}
