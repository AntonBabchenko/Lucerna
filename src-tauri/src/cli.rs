//! Launch-argument parsing.
//!
//! Trust model (design spec `2026-07-29-desktop-integration-design.md` §2):
//! argv is a **trusted** channel — anything that can pass argv to `lucerna.exe`
//! can already start arbitrary programs, so a `--launch` flag grants an attacker
//! nothing new and may start the game with no prompt. That is what a desktop
//! shortcut uses. A `lucerna://` URL is **untrusted** — any web page can trigger
//! the scheme handler with one navigation — so it is only *carried* here and can
//! never express "launch"; the frontend takes it to the import confirmation UI.
//!
//! `--export-bindings` and `--uninstall-cleanup` are handled before Tauri init
//! (see `run()` and `uninstall_cleanup::maybe_run_from_args`) and never reach
//! this parser.

use crate::launch::QuickPlay;
use serde::Serialize;
use specta::Type;

/// The URL scheme Lucerna owns. Registered with the OS only on explicit opt-in
/// (Settings → General). Deliberately not `curseforge://` or `modrinth://` —
/// those belong to those vendors' apps.
pub const URL_SCHEME: &str = "lucerna";

/// Something the launcher was asked to do at startup, or by a second launch
/// whose argv the single-instance guard forwarded.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchIntent {
    /// From argv (trusted): launch this instance, optionally straight into a
    /// world or onto a server.
    Launch {
        instance: String,
        quick_play: Option<QuickPlay>,
    },
    /// From a `lucerna://` URL (untrusted): open the import confirmation UI.
    OpenUrl { url: String },
}

/// Parse launcher arguments into at most one intent.
///
/// Unknown flags are ignored on purpose: `tauri dev`, the OS, and the webview
/// runtime all append arguments we do not own, and refusing to start over an
/// unrecognised flag would be worse than ignoring it. `--world` together with
/// `--server` is a real conflict (two different Quick Play targets), so it
/// yields `None` rather than letting one silently win.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Option<LaunchIntent> {
    let args: Vec<String> = args.into_iter().collect();
    // Value of `--flag <value>`; a following token that itself looks like a flag
    // means the value is missing rather than being the next flag's name.
    let value_after = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .filter(|v| !v.starts_with("--"))
            .cloned()
    };

    if args.iter().any(|a| a == "--launch") {
        let instance = value_after("--launch")?;
        if instance.is_empty() {
            return None;
        }
        let quick_play = match (value_after("--world"), value_after("--server")) {
            (Some(_), Some(_)) => {
                crate::diag!("[cli] --world and --server are mutually exclusive; ignoring argv");
                return None;
            }
            (Some(world), None) => Some(QuickPlay::Singleplayer { world }),
            (None, Some(address)) => Some(QuickPlay::Multiplayer { address }),
            (None, None) => None,
        };
        return Some(LaunchIntent::Launch {
            instance,
            quick_play,
        });
    }

    // A scheme URL arrives as a bare positional argument — Windows and Linux
    // both hand it to a freshly spawned process that way. `skip(1)` drops
    // argv[0] (the exe path), which must never be read as a payload.
    let prefix = format!("{URL_SCHEME}://");
    args.iter()
        .skip(1)
        .find(|a| a.to_ascii_lowercase().starts_with(&prefix))
        .map(|url| LaunchIntent::OpenUrl { url: url.clone() })
}

/// The single slot an inbound intent lands in, whether it came from this
/// process's own argv (cold start) or from a second process via the
/// single-instance guard. The frontend drains it with `take_pending_intent`.
///
/// Pulling, rather than only emitting an event, removes the cold-start race
/// where the event would fire before the webview has a listener attached.
#[derive(Default)]
pub struct PendingIntent(std::sync::Mutex<Option<LaunchIntent>>);

impl PendingIntent {
    pub fn set(&self, intent: LaunchIntent) {
        // A poisoned lock can only mean a previous holder panicked mid-write.
        // The slot is a plain Option, so recovering the guard and overwriting is
        // safe — and better than turning a stale panic into a failed startup.
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(intent);
    }

    /// Take-once: a second caller gets `None`, so an intent cannot be acted on
    /// twice if both the mount drain and an event drain race.
    pub fn take(&self) -> Option<LaunchIntent> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        slot.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("lucerna.exe".to_string())
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn no_arguments_is_no_intent() {
        assert_eq!(parse(argv(&[])), None);
        assert_eq!(parse(Vec::<String>::new()), None);
    }

    #[test]
    fn launch_instance_only() {
        assert_eq!(
            parse(argv(&["--launch", "my-pack"])),
            Some(LaunchIntent::Launch {
                instance: "my-pack".into(),
                quick_play: None
            })
        );
    }

    #[test]
    fn launch_with_world() {
        assert_eq!(
            parse(argv(&["--launch", "my-pack", "--world", "New World"])),
            Some(LaunchIntent::Launch {
                instance: "my-pack".into(),
                quick_play: Some(QuickPlay::Singleplayer {
                    world: "New World".into()
                })
            })
        );
    }

    #[test]
    fn launch_with_server() {
        assert_eq!(
            parse(argv(&["--launch", "p", "--server", "mc.example.com:25565"])),
            Some(LaunchIntent::Launch {
                instance: "p".into(),
                quick_play: Some(QuickPlay::Multiplayer {
                    address: "mc.example.com:25565".into()
                })
            })
        );
    }

    #[test]
    fn world_and_server_together_is_rejected() {
        assert_eq!(
            parse(argv(&["--launch", "p", "--world", "w", "--server", "h"])),
            None
        );
    }

    #[test]
    fn launch_without_value_is_rejected() {
        assert_eq!(parse(argv(&["--launch"])), None);
        assert_eq!(parse(argv(&["--launch", "--world", "w"])), None);
    }

    #[test]
    fn unknown_flags_are_ignored() {
        assert_eq!(
            parse(argv(&["--no-sandbox", "--launch", "p", "--devtools"])),
            Some(LaunchIntent::Launch {
                instance: "p".into(),
                quick_play: None
            })
        );
    }

    #[test]
    fn scheme_url_positional_is_an_open_url_intent() {
        assert_eq!(
            parse(argv(&["lucerna://modpack?source=modrinth&project=x"])),
            Some(LaunchIntent::OpenUrl {
                url: "lucerna://modpack?source=modrinth&project=x".into()
            })
        );
    }

    #[test]
    fn scheme_matching_is_case_insensitive() {
        assert!(matches!(
            parse(argv(&["LUCERNA://modpack?project=x"])),
            Some(LaunchIntent::OpenUrl { .. })
        ));
    }

    #[test]
    fn foreign_positionals_are_ignored() {
        assert_eq!(parse(argv(&["https://modrinth.com/modpack/x"])), None);
        assert_eq!(parse(argv(&["C:\\some\\file.mrpack"])), None);
    }

    #[test]
    fn argv0_is_never_treated_as_a_url() {
        assert_eq!(parse(vec!["lucerna://oops".to_string()]), None);
    }

    #[test]
    fn launch_wins_over_a_url_positional() {
        assert_eq!(
            parse(argv(&["lucerna://modpack?project=x", "--launch", "p"])),
            Some(LaunchIntent::Launch {
                instance: "p".into(),
                quick_play: None
            })
        );
    }

    #[test]
    fn pending_intent_take_is_once() {
        let slot = PendingIntent::default();
        assert!(slot.take().is_none());
        slot.set(LaunchIntent::OpenUrl { url: "u".into() });
        assert!(slot.take().is_some());
        assert!(slot.take().is_none());
    }
}
