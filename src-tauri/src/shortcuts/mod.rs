//! Desktop launch shortcuts.
//!
//! A shortcut is a local file carrying **argv**, not a `lucerna://` URL. That is
//! deliberate and load-bearing: argv is the trusted channel (a local actor able
//! to pass argv can already run any program), so a shortcut may launch the game
//! in one click, while a URL — reachable from any web page — must never be able
//! to. See the design spec §2 and the round-trip test at the bottom of this file,
//! which pins that what a shortcut writes is exactly what `cli::parse` reads
//! back.
//!
//! Windows writes a real `.lnk`; Linux writes a `.desktop` entry. macOS has no
//! v1 support (an `.app` alias with arguments is a different mechanism) and
//! reports that honestly instead of writing a file that would not work.

pub mod icon;

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

/// Longest shortcut file stem we will write. Well under any filesystem limit,
/// and keeps a pathological instance name from producing an unusable icon label.
const MAX_STEM_LEN: usize = 80;

/// What a shortcut launches. World/Server ride on the existing Quick Play
/// support, so they need MC 1.20+ — the UI gates on
/// `instance_quick_play_support` before offering them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShortcutTarget {
    Instance {
        instance_id: String,
    },
    World {
        instance_id: String,
        /// `saves/<folder>` segment of the world to open.
        folder: String,
    },
    Server {
        instance_id: String,
        /// `host` or `host:port`.
        address: String,
    },
}

impl ShortcutTarget {
    pub fn instance_id(&self) -> &str {
        match self {
            Self::Instance { instance_id }
            | Self::World { instance_id, .. }
            | Self::Server { instance_id, .. } => instance_id,
        }
    }

    /// Reject at creation time anything the launcher would refuse at launch
    /// time, so a shortcut can never encode an argument that fails on
    /// double-click. Reuses the launch path's own validators rather than a
    /// second copy of the rules.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Instance { .. } => Ok(()),
            Self::World { folder, .. } => crate::worlds::fs::validate_segment(folder),
            Self::Server { address, .. } => {
                crate::launch::quick_play::validate_server_address(address)
            }
        }
    }
}

/// argv the shortcut hands back to the launcher.
pub fn build_args(t: &ShortcutTarget) -> Vec<String> {
    let mut args = vec!["--launch".to_string(), t.instance_id().to_string()];
    match t {
        ShortcutTarget::Instance { .. } => {}
        ShortcutTarget::World { folder, .. } => {
            args.push("--world".into());
            args.push(folder.clone());
        }
        ShortcutTarget::Server { address, .. } => {
            args.push("--server".into());
            args.push(address.clone());
        }
    }
    args
}

/// Default file stem (no extension) for a target, sanitised.
pub fn default_file_stem(instance_name: &str, t: &ShortcutTarget) -> String {
    let raw = match t {
        ShortcutTarget::Instance { .. } => instance_name.to_string(),
        ShortcutTarget::World { folder, .. } => format!("{instance_name} - {folder}"),
        ShortcutTarget::Server { address, .. } => format!("{instance_name} - {address}"),
    };
    sanitize_stem(&raw)
}

/// Make `raw` safe as a file stem: replace filesystem-reserved characters and
/// controls with `-`, collapse runs, trim, bound the length. Falls back to
/// `"Lucerna"` when nothing usable survives (or the result is a reserved
/// Windows device name), so we always write *something* openable.
pub fn sanitize_stem(raw: &str) -> String {
    let replaced: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut collapsed = replaced;
    while collapsed.contains("--") {
        collapsed = collapsed.replace("--", "-");
    }
    let trimmed: String = collapsed
        .trim()
        .trim_matches('-')
        .trim()
        .chars()
        .take(MAX_STEM_LEN)
        .collect();
    let trimmed = trimmed.trim().to_string();
    if trimmed.is_empty() || crate::pathsafe::is_reserved_windows_name(&trimmed) {
        return "Lucerna".to_string();
    }
    trimmed
}

/// First free `<dir>/<stem>.<ext>`, adding ` (2)`, ` (3)`, … on collision, so
/// creating a second shortcut for the same instance never silently overwrites
/// the first.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let candidate = dir.join(format!("{stem} ({n}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // 998 shortcuts with the same name is not a real scenario; a uuid suffix
    // keeps the operation total rather than failing the user's click.
    dir.join(format!("{stem} ({}).{ext}", uuid::Uuid::new_v4()))
}

/// Shortcut file extension on this OS; `None` where shortcuts are unsupported.
pub fn shortcut_extension() -> Option<&'static str> {
    #[cfg(windows)]
    {
        Some("lnk")
    }
    #[cfg(target_os = "linux")]
    {
        Some("desktop")
    }
    #[cfg(target_os = "macos")]
    {
        None
    }
}

/// Whether this OS supports desktop shortcuts at all.
pub fn supported() -> bool {
    shortcut_extension().is_some()
}

fn unsupported_platform() -> Error {
    Error::UnsupportedPlatform {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

/// Encode one argument for a command-line string, following the
/// `CommandLineToArgvW` rules the OS uses to split it again: a token containing
/// a space or a quote is wrapped in `"`, each embedded `"` becomes `\"`, and any
/// run of backslashes immediately before a quote (embedded or closing) is
/// doubled.
///
/// Escaping rather than assuming clean input is deliberate. A world folder on
/// Linux, or a saved server address, CAN contain a `"` — the launch-path
/// validators reject whitespace and control characters, not quotes. An
/// unescaped quote would close the token early and the remainder would be
/// re-split into extra argv entries on double-click, so the shortcut would
/// launch with a corrupted target instead of the one the user picked.
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains(' ') && !arg.contains('"') && !arg.contains('\\') {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut pending_backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => pending_backslashes += 1,
            '"' => {
                // Double the backslashes that precede this quote, then escape it.
                out.push_str(&"\\".repeat(pending_backslashes * 2 + 1));
                pending_backslashes = 0;
                out.push('"');
            }
            _ => {
                out.push_str(&"\\".repeat(pending_backslashes));
                pending_backslashes = 0;
                out.push(c);
            }
        }
    }
    // Backslashes running into the closing quote must be doubled too.
    out.push_str(&"\\".repeat(pending_backslashes * 2));
    out.push('"');
    out
}

/// Join arguments into one command-line string (see [`quote_arg`]).
pub fn quote_args(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Create the shortcut file for `target` on the user's desktop, named after
/// `label`. Returns the created file's absolute path.
pub fn create(app: &tauri::AppHandle, target: &ShortcutTarget, label: &str) -> Result<String> {
    target.validate()?;
    let Some(ext) = shortcut_extension() else {
        return Err(unsupported_platform());
    };
    let exe = std::env::current_exe().map_err(|e| Error::io("<current_exe>", e))?;
    // The OS's real Desktop location — correct when the Desktop is redirected
    // (OneDrive), unlike a hand-built `%USERPROFILE%\Desktop`.
    let dir = tauri::Manager::path(app)
        .desktop_dir()
        .map_err(|e| Error::io("<desktop_dir>", e.to_string()))?;
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    let path = unique_path(&dir, &sanitize_stem(label), ext);
    write_shortcut(&exe, &build_args(target), label, &path)?;
    Ok(path.display().to_string())
}

#[cfg(windows)]
fn write_shortcut(exe: &Path, args: &[String], _label: &str, path: &Path) -> Result<()> {
    let mut link = mslnk::ShellLink::new(exe)
        .map_err(|e| Error::io(exe.display().to_string(), format!("shortcut target: {e}")))?;
    link.set_arguments(Some(quote_args(args)));
    if let Some(parent) = exe.parent() {
        link.set_working_dir(Some(parent.display().to_string()));
    }
    // Index 0 of the launcher exe: instance pictures are PNG, and a .lnk icon
    // must be an .ico or an exe resource. Per-instance icons are a follow-up.
    link.set_icon_location(Some(format!("{},0", exe.display())));
    link.create_lnk(path)
        .map_err(|e| Error::io(path.display().to_string(), format!("write .lnk: {e}")))
}

#[cfg(target_os = "linux")]
fn write_shortcut(exe: &Path, args: &[String], label: &str, path: &Path) -> Result<()> {
    // Plain text entry — no MIME database involved, so nothing to refresh.
    let body = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={label}\n\
         Exec={} {}\n\
         Icon=lucerna\n\
         Terminal=false\n\
         Categories=Game;\n",
        exe.display(),
        quote_args(args)
    );
    std::fs::write(path, body).map_err(|e| Error::io(path.display().to_string(), e))?;
    // A .desktop file is only offered as launchable by most file managers when
    // it carries the exec bit.
    crate::platform::set_executable(path).map_err(|e| Error::io(path.display().to_string(), e))
}

#[cfg(target_os = "macos")]
fn write_shortcut(_exe: &Path, _args: &[String], _label: &str, _path: &Path) -> Result<()> {
    // Unreachable in practice: `create` returns early because
    // `shortcut_extension()` is None on macOS. Kept as a real arm so adding
    // macOS support means writing this function, not finding a panic.
    Err(unsupported_platform())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_for_a_plain_instance() {
        assert_eq!(
            build_args(&ShortcutTarget::Instance {
                instance_id: "my-pack".into()
            }),
            vec!["--launch", "my-pack"]
        );
    }

    #[test]
    fn args_for_a_world_and_a_server() {
        assert_eq!(
            build_args(&ShortcutTarget::World {
                instance_id: "p".into(),
                folder: "New World".into()
            }),
            vec!["--launch", "p", "--world", "New World"]
        );
        assert_eq!(
            build_args(&ShortcutTarget::Server {
                instance_id: "p".into(),
                address: "mc.example.com:25565".into()
            }),
            vec!["--launch", "p", "--server", "mc.example.com:25565"]
        );
    }

    #[test]
    fn args_round_trip_through_the_cli_parser() {
        // The two halves of the feature must agree: what a shortcut writes is
        // exactly what the launcher parses back. A change to either side that
        // breaks the contract fails here rather than on a user's desktop.
        for (target, expected) in [
            (
                ShortcutTarget::Instance {
                    instance_id: "p".into(),
                },
                None,
            ),
            (
                ShortcutTarget::World {
                    instance_id: "p".into(),
                    folder: "New World".into(),
                },
                Some(crate::launch::QuickPlay::Singleplayer {
                    world: "New World".into(),
                }),
            ),
            (
                ShortcutTarget::Server {
                    instance_id: "p".into(),
                    address: "mc.example.com:25565".into(),
                },
                Some(crate::launch::QuickPlay::Multiplayer {
                    address: "mc.example.com:25565".into(),
                }),
            ),
        ] {
            let mut argv = vec!["lucerna.exe".to_string()];
            argv.extend(build_args(&target));
            assert_eq!(
                crate::cli::parse(argv),
                Some(crate::cli::LaunchIntent::Launch {
                    instance: "p".into(),
                    quick_play: expected
                }),
                "round-trip failed for {target:?}"
            );
        }
    }

    #[test]
    fn default_stem_composes_and_sanitises() {
        assert_eq!(
            default_file_stem(
                "My Pack",
                &ShortcutTarget::Server {
                    instance_id: "p".into(),
                    address: "mc.example.com:25565".into()
                }
            ),
            "My Pack - mc.example.com-25565"
        );
        assert_eq!(
            default_file_stem(
                "My Pack",
                &ShortcutTarget::Instance {
                    instance_id: "p".into()
                }
            ),
            "My Pack"
        );
    }

    #[test]
    fn sanitise_strips_separators_and_falls_back() {
        assert_eq!(sanitize_stem(r"a/b\c:d"), "a-b-c-d");
        assert_eq!(sanitize_stem("a***b"), "a-b");
        assert_eq!(sanitize_stem("   "), "Lucerna");
        assert_eq!(sanitize_stem("///"), "Lucerna");
        // A reserved Windows device name would produce an unopenable file.
        assert_eq!(sanitize_stem("CON"), "Lucerna");
        assert_eq!(
            sanitize_stem(&"x".repeat(200)).chars().count(),
            MAX_STEM_LEN
        );
    }

    #[test]
    fn invalid_targets_are_rejected_before_a_file_is_written() {
        assert!(ShortcutTarget::World {
            instance_id: "p".into(),
            folder: "../escape".into()
        }
        .validate()
        .is_err());
        assert!(ShortcutTarget::Server {
            instance_id: "p".into(),
            address: "has space".into()
        }
        .validate()
        .is_err());
        assert!(ShortcutTarget::Instance {
            instance_id: "p".into()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn quote_args_only_quotes_tokens_that_need_it() {
        assert_eq!(
            quote_args(&[
                "--launch".into(),
                "my-pack".into(),
                "--world".into(),
                "New World".into()
            ]),
            "--launch my-pack --world \"New World\""
        );
    }

    #[test]
    fn quote_args_escapes_quotes_and_trailing_backslashes() {
        // A world folder (Linux) or a saved server address CAN contain a quote —
        // the launch validators reject whitespace and control characters, not
        // quotes — so the encoder must escape rather than assume clean input.
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(r"trailing\"), r#""trailing\\""#);
        assert_eq!(quote_arg(r#"back\"slash"#), r#""back\\\"slash""#);
    }

    /// Split a command-line string the way `CommandLineToArgvW` does, so the
    /// round-trip test below exercises the REAL path: encode → OS split → parse.
    /// (argv[0]'s special casing is irrelevant here — only arguments are passed.)
    fn split_command_line(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        let mut started = false;
        let mut backslashes = 0usize;
        let flush_backslashes = |cur: &mut String, n: &mut usize| {
            cur.push_str(&"\\".repeat(*n));
            *n = 0;
        };
        for c in line.chars() {
            match c {
                '\\' => {
                    backslashes += 1;
                    started = true;
                }
                '"' => {
                    // 2n backslashes + `"` = n backslashes and a quote toggle;
                    // 2n+1 + `"` = n backslashes and a literal quote.
                    cur.push_str(&"\\".repeat(backslashes / 2));
                    if backslashes % 2 == 1 {
                        cur.push('"');
                    } else {
                        in_quotes = !in_quotes;
                    }
                    backslashes = 0;
                    started = true;
                }
                ' ' | '\t' if !in_quotes => {
                    flush_backslashes(&mut cur, &mut backslashes);
                    if started {
                        out.push(std::mem::take(&mut cur));
                        started = false;
                    }
                }
                _ => {
                    flush_backslashes(&mut cur, &mut backslashes);
                    cur.push(c);
                    started = true;
                }
            }
        }
        flush_backslashes(&mut cur, &mut backslashes);
        if started {
            out.push(cur);
        }
        out
    }

    #[test]
    fn encoded_args_survive_an_os_style_split_back_into_the_parser() {
        // Closes the gap the plain argv round-trip leaves: this one goes through
        // the ACTUAL file contents (one command-line string), splits it the way
        // Windows will, and parses that. A quote-mangling encoder shows up here
        // as a corrupted — or injected — launch target.
        for (target, expected) in [
            (
                ShortcutTarget::World {
                    instance_id: "p".into(),
                    folder: r#"weird" folder"#.into(),
                },
                crate::launch::QuickPlay::Singleplayer {
                    world: r#"weird" folder"#.into(),
                },
            ),
            (
                ShortcutTarget::Server {
                    instance_id: "p".into(),
                    address: r#"host" --launch "victim"#.into(),
                },
                crate::launch::QuickPlay::Multiplayer {
                    address: r#"host" --launch "victim"#.into(),
                },
            ),
        ] {
            let line = quote_args(&build_args(&target));
            let mut argv = vec!["lucerna.exe".to_string()];
            argv.extend(split_command_line(&line));
            assert_eq!(
                crate::cli::parse(argv),
                Some(crate::cli::LaunchIntent::Launch {
                    instance: "p".into(),
                    quick_play: Some(expected)
                }),
                "round-trip failed for command line: {line}"
            );
        }
    }

    #[test]
    fn unique_path_avoids_collisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = unique_path(dir.path(), "Pack", "lnk");
        assert_eq!(first.file_name().expect("name"), "Pack.lnk");
        std::fs::write(&first, b"x").expect("write");
        let second = unique_path(dir.path(), "Pack", "lnk");
        assert_eq!(second.file_name().expect("name"), "Pack (2).lnk");
        std::fs::write(&second, b"x").expect("write");
        let third = unique_path(dir.path(), "Pack", "lnk");
        assert_eq!(third.file_name().expect("name"), "Pack (3).lnk");
    }
}
