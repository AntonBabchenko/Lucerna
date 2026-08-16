//! Structural guard: the global cargo-deny license allowlist must stay free of
//! the OpenSSL license.
//!
//! Lucerna ships as a GPL-3.0-or-later binary. The OpenSSL license is
//! GPL-incompatible (advertising clause), so an OpenSSL-licensed dependency
//! may only ever be admitted as a per-crate, per-decision `exceptions` entry
//! in deny.toml — with the legal question resolved in writing — never through
//! the global `allow` list, which would let any new transitive crate link
//! GPL-incompatible code into the binary without review.

use std::fs;
use std::path::Path;

#[test]
fn license_allowlist_has_no_global_openssl() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let deny = fs::read_to_string(Path::new(manifest_dir).join("deny.toml"))
        .expect("src-tauri/deny.toml must exist");

    // Take the [licenses] section, then the `allow = [` array inside it, and
    // assert "OpenSSL" is not an entry. Comments mentioning OpenSSL (the
    // rationale block sits AFTER the array's closing bracket) are fine; strip
    // in-array comments before matching anyway.
    let licenses_section = deny
        .split("[licenses]")
        .nth(1)
        .expect("deny.toml must have a [licenses] section");
    let allow_array = licenses_section
        .split("allow = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("[licenses] must have an `allow = [` array");
    let has_openssl_entry = allow_array
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .any(|code| code.contains("\"OpenSSL\""));
    assert!(
        !has_openssl_entry,
        "deny.toml [licenses] allow contains a global \"OpenSSL\" entry. The \
         OpenSSL license is GPL-incompatible; admit an OpenSSL-licensed crate \
         only via a per-crate `exceptions` entry with a written justification \
         (see the comment block in deny.toml)."
    );
}
