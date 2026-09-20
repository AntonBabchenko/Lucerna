//! Structural guard: every own-server command that writes into — or ships a
//! copy of — `runtime/` consults the server maintenance gate.
//!
//! A backup restore (`server_backup_restore`) holds a
//! `servers_runtime::maintenance` claim while it does `remove_dir_all(runtime/)`
//! plus a re-extract: minutes on a GB-scale server. `runtime/` is where the mod
//! jars, the plugin jars, the world (`level.dat` and `datapacks/`),
//! `server.properties` and the `.lucerna-installed.json` sidecar all live.
//!
//! Until this guard existed the claim had a writer and two readers and nothing
//! in between: only `server_start` / `server_restart` ever read it, and every
//! content command checked `is_running` alone. Both directions were open, and
//! both were reachable from the UI with one click:
//!
//!   - FORWARD. Opening the Add-ons tab auto-fires `server_enrich_mods`
//!     whenever a listed jar has no provenance — which is the normal state for
//!     an imported server. Mid-restore that wrote the sidecar straight into the
//!     tree being replaced. `server_enrich_mods` had no gate at all.
//!   - REVERSE. An install started from Add-ons → Browse keeps downloading
//!     after the user switches to Backups; the tab bar is never disabled and
//!     Restore is disabled only on the live process. The restore claimed
//!     unopposed, because a writer in flight registered nothing anywhere.
//!
//! A third one stayed open after that, for the same reason the REVERSE one had
//! been: the commands whose PRODUCT is a copy of the tree — upload, export,
//! backup, the upload preflight, a client instance built from the server's
//! mods, and the backup scheduler, which took no gate at all — opened with the
//! snapshot and then held nothing, for seconds to hours. A restore started
//! meanwhile replaced the tree under them and the copy came out torn and
//! reported as a success. They take a shared READ claim now; rule 3 covers it.
//!
//! Why a test and not a comment: the identical rule on the CLIENT side lived as
//! prose in `datapacks::guard`'s module doc before
//! `structural_maintenance_gate.rs` pinned it — and this module is where that
//! client gate was COPIED FROM (world-migration spec §4.0/A5: "`servers_runtime::maintenance`
//! is the model"). The copy grew a full gate and a guard; the original did not.
//! A comment is not a guard, and an un-guarded original is how the drift
//! happened the first time.
//!
//! Three rules:
//!
//!   1. LISTED WRITERS. Every `(file, fn)` in [`GATED`] carries its required
//!      spelling on a code line of its body. A listed fn that no longer exists
//!      fails too, so the list cannot rot.
//!   2. RATCHET. In every [`RATCHETED`] file, every `#[tauri::command]` either
//!      carries a gate spelling or is declared in [`READ_ONLY`] with a reason.
//!      A new command must choose, in code — silence fails the build. A
//!      `READ_ONLY` entry that gains a gate, or names a fn that is gone, is
//!      reported as stale.
//!   3. CLAIM HELD. A shared claim must be HELD for the write: the first code
//!      line carrying it binds a named local (`let write = …` / `let _write =
//!      …`, never `let _ = …`, which drops the guard — and releases the server
//!      — on the spot), and it comes before the body's first `.await`. The
//!      plausible regression is shrinking the protected region "so the restore
//!      works during the download", which re-opens the exact window this
//!      change closed.
//!
//! Which gate a command takes is a decision this guard pins per [`GATED`]
//! entry, not one it can derive. A per-item writer that spans `.await`s takes
//! the SHARED claim — the server Mods browser deliberately runs two installs at
//! once ("installs can overlap without clobbering each other's busy state"), so
//! an exclusive claim would refuse the second and change shipped behaviour —
//! while a single short write takes the snapshot. Moving an entry between
//! spellings changes user-visible behaviour; do it in the same change as the
//! reason.
//!
//! Guardrail, not a static analyzer — same framing as
//! `structural_maintenance_gate.rs`. Named gaps:
//!
//!   - ORDER. The scan is lexical: it proves the gate is CALLED, not that it
//!     runs before the first write. Review owns ordering — except for a shared
//!     claim, which rule 3 pins ahead of the body's first `.await`. A claim
//!     moved below a SYNC read is still invisible.
//!   - RUNNING POLICY IS NOT CHECKED HERE. Whether a command also refuses a
//!     running or starting server is per-command and deliberately untouched by
//!     this guard: `server.properties` is editable while the server runs, mods
//!     are not. This guard pins the MAINTENANCE term only. In particular it
//!     does NOT pin that a mod writer checks `is_starting` — the mod and plugin
//!     writers still check `is_running` alone, which `datapacks::guard`'s own
//!     doc explains is not enough. That is a separate, known gap.
//!   - START vs CONTENT WRITE. `maintenance_is_active` is the exclusive half
//!     only, so Start during a mod install stays admitted, exactly as it ships.
//!     Whether it should is a separate question and is not pinned here.
//!   - A gate reached through a helper this file does not name is invisible;
//!     add the helper's call-site spelling to [`GATE_SPELLINGS`] together with
//!     the helper, and list the helper itself in [`GATED`] (as `gate` is).
//!   - Whole-line `//` comments are exempt; a body whose only mention of the
//!     gate is prose therefore FAILS — which is the point.
//!   - A fn is located by a COLUMN-0 signature and its body ends at the first
//!     `}` in column 0 at or after it, so an indented test fn of the same name
//!     is never mistaken for it. Sound for rustfmt'd top-level fns, which is
//!     all this tree contains.

use std::fs;
use std::path::{Path, PathBuf};

/// The snapshot term, for a command performing ONE short write:
/// `crate::servers_runtime::maintenance::not_under_maintenance(id)`.
const SNAPSHOT_GATE: &str = "maintenance::not_under_maintenance(";

/// A per-item content writer's SHARED claim:
/// `crate::servers_runtime::maintenance::claim_shared_write(id)` — admitted
/// alongside other item writers and while the server runs, refused under an
/// exclusive claim, and refusing every exclusive claim while held.
const SHARED_WRITE_GATE: &str = "maintenance::claim_shared_write(";

/// A long reader's SHARED READ claim:
/// `crate::servers_runtime::maintenance::claim_shared_read(id)` — for an
/// operation whose PRODUCT is a copy of `runtime/` (upload, export, backup, the
/// preflight walk, a client instance built from the server's mods). Same matrix
/// row as the write claim; a kind of its own so a refused restore can say it was
/// a copy in flight and not an add-on install. It replaces the snapshot these
/// commands used to open with, which saw a restore already running and was
/// invisible to one that started later.
const SHARED_READ_GATE: &str = "maintenance::claim_shared_read(";

/// The exclusive claim, in its cause-reporting spelling:
/// `crate::servers_runtime::maintenance::try_begin(id)` — the restore's.
const TRY_BEGIN_GATE: &str = "maintenance::try_begin(";

/// The exclusive claim, in its plain spelling: the two import paths, which
/// claim a freshly reserved id nothing else can hold yet.
const CLAIM_GATE: &str = "maintenance::maintenance_begin(";

/// The claim-only read: Start/Restart's half.
const ACTIVE_GATE: &str = "maintenance_is_active(";

/// `commands/server_datapacks.rs` routes its running/starting/maintenance check
/// through `datapacks::guard::gate`, itself listed in [`GATED`].
const DATAPACKS_DELEGATE: &str = "guard::gate(&";

/// Spellings a command body may carry to count as gated. Every entry is a
/// call-site prefix, so a bare mention in prose never counts and a helper with
/// a different name never matches by accident.
const GATE_SPELLINGS: &[&str] = &[
    SNAPSHOT_GATE,
    SHARED_WRITE_GATE,
    SHARED_READ_GATE,
    TRY_BEGIN_GATE,
    CLAIM_GATE,
    ACTIVE_GATE,
    DATAPACKS_DELEGATE,
];

/// The spellings that TAKE a claim — the entries rule 3 applies to.
const CLAIM_SPELLINGS: &[&str] = &[
    SHARED_WRITE_GATE,
    SHARED_READ_GATE,
    TRY_BEGIN_GATE,
    CLAIM_GATE,
];

/// `(file, fn, why)` — claim takers excused from rule 3's "before the first
/// `.await`" half, and only that half: the claim must still be bound. A timer
/// loop sleeps first by construction and claims once per tick; what has to hold
/// for it is that the claim precedes that TICK's first read of `runtime/`, which
/// is an ordering question this lexical scan cannot answer and review owns.
const CLAIM_AFTER_AWAIT_OK: &[(&str, &str, &str)] = &[(
    "commands/servers_runtime.rs",
    "spawn_backup_scheduler",
    "a timer loop: `sleep(interval).await` opens every iteration, and the claim is taken \
     per tick, before `pause_saves_for_backup` and the zip",
)];

/// `(path relative to src/, fn name, required spelling, why it needs the gate)`.
const GATED: &[(&str, &str, &str, &str)] = &[
    // --- the registry itself: both halves must see each other --------------
    (
        "servers_runtime/maintenance.rs",
        "try_begin",
        "sharing.contains_key(",
        "the exclusive claim must refuse while a per-item writer is in flight — the \
         reverse direction, and the one a snapshot on the writer's side cannot give",
    ),
    (
        "servers_runtime/maintenance.rs",
        "try_begin",
        "reading.contains_key(",
        "the exclusive claim must refuse while a long reader is in flight — a restore that \
         replaces the tree under an upload, an export or a backup tears the copy being made",
    ),
    (
        "servers_runtime/maintenance.rs",
        "claim_shared_write",
        "held.contains(",
        "a per-item writer must refuse while a restore or import holds the server",
    ),
    (
        "servers_runtime/maintenance.rs",
        "claim_shared_read",
        "held.contains(",
        "a long reader must refuse while a restore or import holds the server — a copy of a \
         half-restored tree is worse than a refusal",
    ),
    (
        "servers_runtime/maintenance.rs",
        "not_under_maintenance",
        "maintenance_is_active(",
        "the snapshot term must read the exclusive half",
    ),
    (
        "servers_runtime/datapacks/guard.rs",
        "gate",
        ACTIVE_GATE,
        "the delegate every datapack writer calls — the maintenance term's one spelling there",
    ),
    // --- launch: unchanged, pinned so it stays the exclusive half only ------
    (
        "commands/servers_runtime.rs",
        "server_start",
        ACTIVE_GATE,
        "must not launch a JVM over a half-restored tree; reads the exclusive half only, \
         so a mod install does not block Start (as it ships)",
    ),
    (
        "commands/servers_runtime.rs",
        "server_restart",
        ACTIVE_GATE,
        "the Start twin",
    ),
    // --- the exclusive claimants -------------------------------------------
    (
        "commands/servers_runtime.rs",
        "server_backup_restore",
        TRY_BEGIN_GATE,
        "remove_dir_all(runtime/) + re-extract; takes the claim and must be told WHICH of \
         a rival restore or an in-flight content write blocked it, so the copy is honest",
    ),
    (
        "commands/servers_runtime.rs",
        "server_import_commit",
        CLAIM_GATE,
        "lays down runtime/ for a freshly reserved id while the server is already listable",
    ),
    (
        "servers_runtime/import/mod.rs",
        "commit_preserve",
        CLAIM_GATE,
        "the non-AppHandle import path, same reason",
    ),
    // --- per-item content writers: shared claim -----------------------------
    (
        "commands/servers_runtime.rs",
        "server_install_mod",
        SHARED_WRITE_GATE,
        "downloads a jar and its dependency closure into runtime/mods/ and writes the sidecar",
    ),
    (
        "commands/servers_runtime.rs",
        "server_install_plugin",
        SHARED_WRITE_GATE,
        "the plugin twin, into runtime/plugins/",
    ),
    (
        "commands/servers_runtime.rs",
        "server_install_local",
        SHARED_WRITE_GATE,
        "writes a picked jar into runtime/mods/",
    ),
    (
        "commands/servers_runtime.rs",
        "server_install_plugin_local",
        SHARED_WRITE_GATE,
        "the plugin twin",
    ),
    (
        "commands/servers_runtime.rs",
        "server_update_one",
        SHARED_WRITE_GATE,
        "installs the new jar, removes the old one and swaps the sidecar rows",
    ),
    (
        "commands/servers_runtime.rs",
        "server_update_plugin_one",
        SHARED_WRITE_GATE,
        "the plugin twin",
    ),
    (
        "commands/servers_runtime.rs",
        "server_enrich_mods",
        SHARED_WRITE_GATE,
        "hashes every jar and writes .lucerna-installed.json — and the Add-ons tab fires it \
         automatically on mount, which is how a restore gets written into with no click at all",
    ),
    (
        "commands/servers_runtime.rs",
        "server_enrich_plugins",
        SHARED_WRITE_GATE,
        "the plugin twin",
    ),
    (
        "commands/servers_runtime.rs",
        "server_install_missing_dep",
        SHARED_WRITE_GATE,
        "downloads a missing dependency jar into runtime/mods/",
    ),
    (
        "commands/servers_runtime.rs",
        "server_disable_mods",
        SHARED_WRITE_GATE,
        "renames a batch of jars to *.disabled",
    ),
    (
        "commands/servers_runtime.rs",
        "server_remove_mods",
        SHARED_WRITE_GATE,
        "deletes a batch of jars from runtime/mods/",
    ),
    (
        "commands/servers_runtime.rs",
        "server_quarantine_client_mods",
        SHARED_WRITE_GATE,
        "reads each jar's metadata and sets aside the client-only ones",
    ),
    (
        "commands/servers_runtime.rs",
        "server_redownload_jar",
        SHARED_WRITE_GATE,
        "re-downloads the server jar into runtime/",
    ),
    (
        "commands/servers_runtime.rs",
        "server_switch_core",
        SHARED_WRITE_GATE,
        "downloads and swaps the server core jar in runtime/",
    ),
    (
        "commands/server_datapacks.rs",
        "server_install_datapack",
        SHARED_WRITE_GATE,
        "writes a zip into the world's datapacks/ and its level.dat row",
    ),
    (
        "commands/server_datapacks.rs",
        "server_install_datapack_version",
        SHARED_WRITE_GATE,
        "downloads a catalog pack into the world",
    ),
    (
        "commands/server_datapacks.rs",
        "server_remove_datapack",
        SHARED_WRITE_GATE,
        "removes the entry, its level.dat name and its sidecar row",
    ),
    (
        "commands/server_datapacks.rs",
        "server_set_datapack_enabled",
        SHARED_WRITE_GATE,
        "a level.dat read-modify-write",
    ),
    (
        "commands/server_datapacks.rs",
        "server_update_datapack_one",
        SHARED_WRITE_GATE,
        "a network download plus a level.dat read-modify-write; its UpdateGuard is a \
         DIFFERENT registry that only Start reads, so it does not make a restore refuse",
    ),
    // --- short writers: the snapshot ----------------------------------------
    (
        "commands/servers_runtime.rs",
        "server_enable_mod",
        SNAPSHOT_GATE,
        "renames one jar out of *.disabled",
    ),
    (
        "commands/servers_runtime.rs",
        "server_disable_mod",
        SNAPSHOT_GATE,
        "renames one jar into *.disabled",
    ),
    (
        "commands/servers_runtime.rs",
        "server_delete_mod",
        SNAPSHOT_GATE,
        "deletes one jar from runtime/mods/",
    ),
    (
        "commands/servers_runtime.rs",
        "server_enable_plugin",
        SNAPSHOT_GATE,
        "the plugin twin",
    ),
    (
        "commands/servers_runtime.rs",
        "server_disable_plugin",
        SNAPSHOT_GATE,
        "the plugin twin",
    ),
    (
        "commands/servers_runtime.rs",
        "server_delete_plugin",
        SNAPSHOT_GATE,
        "the plugin twin",
    ),
    (
        "commands/servers_runtime.rs",
        "server_accept_eula",
        SNAPSHOT_GATE,
        "writes runtime/eula.txt",
    ),
    (
        "commands/servers_runtime.rs",
        "server_change_port",
        SNAPSHOT_GATE,
        "rewrites runtime/server.properties",
    ),
    (
        "commands/servers_runtime.rs",
        "server_write_properties",
        SNAPSHOT_GATE,
        "rewrites runtime/server.properties — and create_dir_all's runtime/ back into \
         existence, which a restore is in the middle of removing",
    ),
    (
        "commands/servers_runtime.rs",
        "server_whitelist_add",
        SNAPSHOT_GATE,
        "rewrites runtime/whitelist.json",
    ),
    (
        "commands/servers_runtime.rs",
        "server_whitelist_remove",
        SNAPSHOT_GATE,
        "rewrites runtime/whitelist.json",
    ),
    (
        "commands/servers_runtime.rs",
        "server_ops_add",
        SNAPSHOT_GATE,
        "rewrites runtime/ops.json",
    ),
    (
        "commands/servers_runtime.rs",
        "server_ops_remove",
        SNAPSHOT_GATE,
        "rewrites runtime/ops.json",
    ),
    (
        "commands/servers_runtime.rs",
        "server_delete",
        SNAPSHOT_GATE,
        "remove_dir_all of the server root a restore is rewriting inside",
    ),
    // --- reads whose PRODUCT is a copy of the tree ---------------------------
    // Each reason below is the FORWARD half (the reader starts mid-restore) and
    // is why the claim refuses under an exclusive one. The reverse half is the
    // same for all of them and is why it is a CLAIM and not the snapshot it used
    // to be: they walk `runtime/` for seconds to hours, and a restore that
    // starts meanwhile must be able to see them.
    (
        "commands/servers_runtime.rs",
        "spawn_backup_scheduler",
        SHARED_READ_GATE,
        "the timer twin of server_backup_create — not a #[tauri::command], so the ratchet \
         cannot see it, and until it was listed here it took no gate at all: a scheduled zip \
         outlives a Stop click, after which Restore is admitted by its running check",
    ),
    (
        "commands/servers_runtime.rs",
        "server_backup_create",
        SHARED_READ_GATE,
        "zips runtime/ — mid-restore it would snapshot a torn tree AND, through the \
         keep-N prune, evict a good snapshot in favour of it",
    ),
    (
        "commands/servers_runtime.rs",
        "server_backup_delete",
        SNAPSHOT_GATE,
        "deletes a zip from the set the running restore is reading its source from",
    ),
    (
        "commands/servers_runtime.rs",
        "server_export_zip",
        SHARED_READ_GATE,
        "hands the user a zip of the tree; mid-restore that zip is torn and says nothing about it",
    ),
    (
        "commands/servers_runtime.rs",
        "server_upload",
        SHARED_READ_GATE,
        "ships runtime/ to the user's host — uploading a half-restored tree is worse than refusing",
    ),
    (
        "commands/servers_runtime.rs",
        "server_upload_preflight",
        SHARED_READ_GATE,
        "walks runtime/ to build the upload plan the upload then trusts",
    ),
    (
        "commands/servers_runtime.rs",
        "server_create_client_instance",
        SHARED_READ_GATE,
        "copies the server's mod set into a new client instance",
    ),
];

/// Commands in the ratcheted files that touch nothing a restore replaces:
/// `(path relative to src/, fn name, why it needs no gate)`.
const READ_ONLY: &[(&str, &str, &str)] = &[
    // -- pure reads over runtime/ ------------------------------------------
    ("commands/servers_runtime.rs", "server_list", "lists servers from server.json + the live-process map"),
    ("commands/servers_runtime.rs", "server_list_mods", "lists jar names in runtime/mods/"),
    ("commands/servers_runtime.rs", "server_list_plugins", "lists jar names in runtime/plugins/"),
    ("commands/servers_runtime.rs", "server_read_properties", "reads runtime/server.properties"),
    ("commands/servers_runtime.rs", "server_list_logs", "lists runtime/logs/"),
    ("commands/servers_runtime.rs", "server_read_log", "reads one log file"),
    ("commands/servers_runtime.rs", "server_backup_list", "lists the backup set; the restore's own source stays listed"),
    ("commands/servers_runtime.rs", "server_backup_policy_get", "reads the policy from server.json"),
    ("commands/servers_runtime.rs", "server_whitelist_list", "reads runtime/whitelist.json"),
    ("commands/servers_runtime.rs", "server_ops_list", "reads runtime/ops.json"),
    ("commands/servers_runtime.rs", "server_diagnose", "reads the log and classifies it"),
    ("commands/servers_runtime.rs", "server_import_inspect", "reads a source tree outside any server"),
    ("commands/server_datapacks.rs", "server_list_datapacks", "reads level.dat and the world's datapacks/"),
    // -- reads that persist launcher-owned registry metadata ----------------
    // Declared, not gated, and the distinction is deliberate: these reconcile
    // the sidecar against what is on disk. Mid-restore the reconcile sees a
    // tree in motion and rewrites rows to match it — a stale list, not a torn
    // install, and the next list after the restore reconciles it back. Gating
    // them would refuse merely LOOKING at the Add-ons tab during a restore.
    ("commands/servers_runtime.rs", "server_list_mods_enriched", "reconciles the sidecar against disk while listing; self-correcting (see note)"),
    ("commands/servers_runtime.rs", "server_list_plugins_enriched", "the plugin twin (see note)"),
    ("commands/servers_runtime.rs", "server_check_mod_updates", "reconcile + a network query per mod (see note)"),
    ("commands/servers_runtime.rs", "server_check_plugin_updates", "the plugin twin (see note)"),
    ("commands/server_datapacks.rs", "server_check_datapack_updates", "the datapack twin (see note)"),
    // -- writes OUTSIDE runtime/ (server.json, the keyring, OS state) -------
    ("commands/servers_runtime.rs", "server_create", "creates a new server; nothing else can hold its id yet"),
    ("commands/servers_runtime.rs", "server_rename", "renames the display name in server.json"),
    ("commands/servers_runtime.rs", "server_update_runtime_config", "writes heap/flags to server.json"),
    ("commands/servers_runtime.rs", "server_raise_heap", "writes the heap ceiling to server.json"),
    ("commands/servers_runtime.rs", "server_lower_heap", "writes the heap ceiling to server.json"),
    ("commands/servers_runtime.rs", "server_backup_policy_set", "writes the policy to server.json and rearms the scheduler"),
    ("commands/servers_runtime.rs", "server_set_upload_config", "writes the SFTP config to server.json and the password to the keyring"),
    ("commands/servers_runtime.rs", "server_get_upload_auth", "reads the keyring"),
    ("commands/servers_runtime.rs", "server_set_upload_auth", "writes the keyring"),
    ("commands/servers_runtime.rs", "server_upload_resume_state", "reads the resume marker"),
    ("commands/servers_runtime.rs", "server_cancel_upload", "flips an in-memory cancel flag"),
    ("commands/servers_runtime.rs", "server_import_cancel", "flips an in-memory cancel flag for a pending import"),
    ("commands/servers_runtime.rs", "server_host_key_preview", "a network probe of the SFTP host"),
    ("commands/servers_runtime.rs", "server_firewall_status", "queries the OS firewall"),
    ("commands/servers_runtime.rs", "server_firewall_add_rule", "adds an OS firewall rule"),
    ("commands/servers_runtime.rs", "server_connectivity", "a network probe"),
    ("commands/servers_runtime.rs", "server_public_address", "a network probe"),
    ("commands/servers_runtime.rs", "server_core_versions", "a network query of the core's version list"),
    // -- process lifecycle and shell-outs -----------------------------------
    ("commands/servers_runtime.rs", "server_stop", "stops a running server; a restore requires it stopped already"),
    ("commands/servers_runtime.rs", "server_kill", "kills a running server, same"),
    ("commands/servers_runtime.rs", "server_send_command", "writes to a running server's stdin"),
    ("commands/servers_runtime.rs", "server_stop_orphan", "kills a stray JVM and clears runtime/server.pid — a recovery action that must stay available, and any restore replaces that file anyway"),
    ("commands/servers_runtime.rs", "server_open_folder", "opens a file manager"),
    ("commands/servers_runtime.rs", "server_open_logs_folder", "opens a file manager"),
    ("commands/servers_runtime.rs", "server_open_mods_folder", "opens a file manager"),
    ("commands/servers_runtime.rs", "server_open_plugins_folder", "opens a file manager"),
];

/// Files whose every `#[tauri::command]` must be gated or declared read-only.
const RATCHETED: &[&str] = &[
    "commands/servers_runtime.rs",
    "commands/server_datapacks.rs",
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn read_lines(rel: &str) -> Vec<String> {
    let path = src_dir().join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The name of the fn a COLUMN-0 signature line defines. `None` for anything
/// else — including an indented signature (a test fn, a nested helper), so a
/// same-named fn inside `mod tests` is never the one found.
fn top_level_fn_name(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let t = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub(super) "))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    let t = t.strip_prefix("async ").unwrap_or(t);
    let t = t.strip_prefix("fn ")?;
    let end = t.find(|c: char| c == '(' || c == '<')?;
    Some(&t[..end])
}

/// Index of the line closing the top-level fn opened at `sig`: the first `}` in
/// column 0 at or after it.
fn body_end(lines: &[String], sig: usize) -> usize {
    lines[sig..]
        .iter()
        .position(|l| l == "}")
        .map(|off| sig + off)
        .unwrap_or(lines.len() - 1)
}

/// `(signature index, body end)` of the top-level fn named `name`.
fn locate_fn(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let sig = lines
        .iter()
        .position(|l| top_level_fn_name(l) == Some(name))?;
    Some((sig, body_end(lines, sig)))
}

fn is_command_attr(line: &str) -> bool {
    line.trim().starts_with("#[tauri::command")
}

/// Every `#[tauri::command]` fn in `lines`: `(name, signature index, body end)`.
fn commands_in(lines: &[String]) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !is_command_attr(line) {
            continue;
        }
        let mut j = i + 1;
        let found = loop {
            if j >= lines.len() {
                break None;
            }
            if let Some(name) = top_level_fn_name(&lines[j]) {
                break Some((name.to_string(), j));
            }
            j += 1;
        };
        let Some((name, sig)) = found else { continue };
        out.push((name, sig, body_end(lines, sig)));
    }
    out
}

/// Indices of the CODE lines of `lines[sig..=end]` — signature and whole-line
/// `//` comments excluded — containing `needle`, in order.
fn code_lines_with(lines: &[String], sig: usize, end: usize, needle: &str) -> Vec<usize> {
    (sig + 1..=end)
        .filter(|&i| {
            let l = &lines[i];
            !l.trim_start().starts_with("//") && l.contains(needle)
        })
        .collect()
}

/// True when a CODE line of the body contains `needle`.
fn body_carries(lines: &[String], sig: usize, end: usize, needle: &str) -> bool {
    !code_lines_with(lines, sig, end, needle).is_empty()
}

/// The pattern a `let` on `line` binds, when it is not the bare `_` — i.e. the
/// value lives in a binding until the end of its scope. `let _ = claim` drops
/// the guard on the spot (`None`); `let _write`, `let write`, `let mut write`
/// and `let Some(write) = … else` all hold it.
fn let_binding(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("let ")?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let pattern_end = rest
        .find(|c: char| c.is_whitespace() || c == '=' || c == ':')
        .unwrap_or(rest.len());
    let pattern = &rest[..pattern_end];
    (!pattern.is_empty() && pattern != "_").then_some(pattern)
}

// ---------------------------------------------------------------------------
// Rule 1 — LISTED WRITERS
// ---------------------------------------------------------------------------

#[test]
fn every_listed_server_writer_opens_with_the_maintenance_gate() {
    let mut problems: Vec<String> = Vec::new();
    for (file, name, spelling, why) in GATED {
        let lines = read_lines(file);
        let Some((sig, end)) = locate_fn(&lines, name) else {
            problems.push(format!(
                "{file}: `{name}` is listed in GATED but no longer exists as a top-level fn. \
                 Renamed or removed? Update the list in the same change."
            ));
            continue;
        };
        if !body_carries(&lines, sig, end, spelling) {
            problems.push(format!(
                "{file}:{} `{name}` must carry `{spelling}…` on a code line of its body — {why}",
                sig + 1
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "server maintenance gate missing.\n\n{}\n\nEvery own-server command that writes into — \
         or ships a copy of — `runtime/` consults \
         `crate::servers_runtime::maintenance`: a short write takes \
         `not_under_maintenance(&id)?`, a writer that spans `.await`s takes \
         `let _write = …claim_shared_write(&id)?`, and a wholesale rewrite takes the claim \
         itself. A backup restore does `remove_dir_all(runtime/)` + re-extract for minutes; \
         anything admitted meanwhile writes into, or copies, a tree that is being replaced.",
        problems.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Rule 2 — RATCHET
// ---------------------------------------------------------------------------

#[test]
fn every_server_command_is_gated_or_declared_read_only() {
    let mut problems: Vec<String> = Vec::new();
    for file in RATCHETED {
        let lines = read_lines(file);
        for (name, sig, end) in commands_in(&lines) {
            let gated = GATE_SPELLINGS
                .iter()
                .any(|s| body_carries(&lines, sig, end, s));
            let declared = READ_ONLY.iter().any(|(f, n, _)| f == file && *n == name);
            match (gated, declared) {
                (false, false) => problems.push(format!(
                    "{file}:{} `{name}` is a #[tauri::command] with no maintenance gate and no \
                     READ_ONLY declaration. Add the gate, or declare it read-only WITH A REASON \
                     that is true.",
                    sig + 1
                )),
                (true, true) => problems.push(format!(
                    "{file}:{} `{name}` is declared READ_ONLY but now carries a gate — the \
                     declaration is stale, move it to GATED.",
                    sig + 1
                )),
                _ => {}
            }
        }
    }
    // A READ_ONLY entry naming a fn that is gone is rot of the same kind.
    for (file, name, _) in READ_ONLY {
        let lines = read_lines(file);
        if locate_fn(&lines, name).is_none() {
            problems.push(format!(
                "{file}: `{name}` is declared READ_ONLY but no longer exists — drop the entry."
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "server command ratchet.\n\n{}\n\nEvery `#[tauri::command]` in {RATCHETED:?} must \
         CHOOSE, in code: carry a maintenance gate, or be declared in READ_ONLY with a reason. \
         Silence is what let `server_enrich_mods` ship with no gate at all.",
        problems.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Rule 3 — CLAIM HELD
// ---------------------------------------------------------------------------

#[test]
fn a_claim_is_bound_to_a_named_local_before_the_first_await() {
    let mut problems: Vec<String> = Vec::new();
    for (file, name, spelling, _) in GATED {
        if !CLAIM_SPELLINGS.contains(spelling) {
            continue;
        }
        let lines = read_lines(file);
        let Some((sig, end)) = locate_fn(&lines, name) else {
            continue; // rule 1 reports the missing fn
        };
        let Some(&claim_line) = code_lines_with(&lines, sig, end, spelling).first() else {
            continue; // rule 1 reports the missing gate
        };

        // (a) the claim must be BOUND — `let _ = claim_shared_write(…)` drops
        //     the guard on the spot and protects nothing.
        if let_binding(&lines[claim_line]).is_none() {
            problems.push(format!(
                "{file}:{} `{name}` takes the claim without binding it. `let _ = …` drops the \
                 guard immediately — bind it (`let _write = …`) so it lives for the write.",
                claim_line + 1
            ));
        }

        // (b) it must come before the body's first `.await`: a claim taken after
        //     the download has already started protects only the tail. A timer
        //     loop is excused from THIS half only — see `CLAIM_AFTER_AWAIT_OK`.
        if CLAIM_AFTER_AWAIT_OK
            .iter()
            .any(|(f, n, _)| f == file && n == name)
        {
            continue;
        }
        let first_await = code_lines_with(&lines, sig, end, ".await")
            .first()
            .copied()
            .unwrap_or(usize::MAX);
        if claim_line > first_await {
            problems.push(format!(
                "{file}:{} `{name}` takes the claim AFTER its first `.await` (line {}). \
                 Everything before it runs unprotected.",
                claim_line + 1,
                first_await + 1
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "claim not held for the write.\n\n{}\n\nThe guard IS the protection: it is what a \
         restore starting later can see. Bind it to a named local at the top of the command \
         and let it drop at the end of scope.",
        problems.join("\n")
    );
}

/// An exemption nobody needs any more is a hole waiting for the next fn of that
/// name: every [`CLAIM_AFTER_AWAIT_OK`] entry must still be a listed claim taker
/// whose claim really does sit below its first `.await`.
#[test]
fn the_await_exemption_list_is_not_stale() {
    let mut problems: Vec<String> = Vec::new();
    for (file, name, _) in CLAIM_AFTER_AWAIT_OK {
        let Some((_, _, spelling, _)) = GATED
            .iter()
            .find(|(f, n, s, _)| f == file && n == name && CLAIM_SPELLINGS.contains(s))
        else {
            problems.push(format!(
                "{file} `{name}` is excused but is not a listed claim taker"
            ));
            continue;
        };
        let lines = read_lines(file);
        let Some((sig, end)) = locate_fn(&lines, name) else {
            problems.push(format!("{file} `{name}` is excused but no longer exists"));
            continue;
        };
        let claim = code_lines_with(&lines, sig, end, spelling).first().copied();
        let first_await = code_lines_with(&lines, sig, end, ".await").first().copied();
        if !matches!((claim, first_await), (Some(c), Some(a)) if c > a) {
            problems.push(format!(
                "{file} `{name}` no longer takes its claim after an `.await` — drop the \
                 exemption so rule 3 applies to it in full"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "stale await exemption.\n\n{}",
        problems.join("\n")
    );
}
