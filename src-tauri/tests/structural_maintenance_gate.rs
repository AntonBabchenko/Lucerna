//! Structural guard: every instance, world and datapack writer opens with the
//! maintenance gate.
//!
//! A world migration (`worlds::migrate`) holds `instances::maintenance` claims
//! on BOTH instances for the whole operation — minutes, on the copy path —
//! and relies on every other writer refusing with `InstanceBusy` while the
//! claim is held (world-migration spec §4.0, amendment A5). Nothing in the
//! frontend queue protects that: the world commands are direct IPC calls that
//! know nothing of the task registry's serial lane. The gate has ONE
//! definition, `instances::maintenance::write_allowed(id)` —
//! `is_running || is_starting || maintenance_is_active` — and this guard pins
//! that every writer actually calls it.
//!
//! Two deliberate exceptions, both checked here under their own spelling:
//!
//!   - LAUNCH must not refuse on `is_running` at command entry (a second
//!     launch of the SAME id is refused inside `launch::start` by
//!     `claim_start`), so `launch_instance` checks `maintenance_is_active`
//!     alone — and `start` checks it again right after `claim_start`,
//!     Dekker-paired with the migration's claim-then-check.
//!   - `delete_backup` only touches `<instance>/backups/`, never the world
//!     tree the JVM holds, and has always been allowed while the game runs; a
//!     Move migration relocates that set file by file, so it refuses for the
//!     maintenance claim alone.
//!
//! Why a test and not a comment: the rule "every world writer opens with the
//! gate" lived as prose in `datapacks::guard`'s module doc, listing four
//! world commands by name. `recover_stranded_world` was added later WITH the
//! gate, but nothing would have failed had it been added without — a comment
//! is not a guard. Three rules:
//!
//!   1. LISTED WRITERS. Every `(file, fn)` in `GATED` carries its required
//!      spelling on a code line of its body. A listed fn that no longer
//!      exists fails too, so the list cannot rot.
//!   2. RATCHET. In every `RATCHETED` file — `commands/worlds.rs`,
//!      `commands/datapacks.rs`, `commands/mods.rs`, `commands/modpack_cmds.rs`,
//!      `commands/assets.rs` — every `#[tauri::command]` either carries a
//!      `GATE_SPELLINGS` entry or is declared read-only in `READ_ONLY` with a
//!      reason. A new command in any of them must choose, in code — silence
//!      fails the build. A `READ_ONLY` entry that gains a gate, or names a fn
//!      that is gone, is reported as stale. A command that TAKES a claim itself
//!      (`maintenance_begin(`, the migration command; `claim_write(`, the long
//!      writers; `claim_shared_write(`, the per-item mod, asset and pack-file
//!      writers) is gated by construction: the claims refuse each other.
//!   3. CLAIM HELD. A listed writer that takes a claim — exclusive or shared —
//!      must HOLD it for the write: the first code line carrying the claim
//!      binds a named local (`let claim = …`, never `let _ = …`, which drops
//!      the guard — and releases the instance — on the spot); it comes before
//!      the body's first `.await`; and an explicit `drop(<name>)` on the fn's
//!      main path comes after the body's LAST `.await`. The ordering halves
//!      exist because the plausible regression is shrinking the protected
//!      region "so Play works during a modpack update's downloads" — by
//!      claiming late or releasing early — which leaves the diff and the
//!      carry-disabled snapshot describing a tree a concurrent writer may
//!      already have changed. For a shared claim the same shrink ("only the
//!      commit needs it") re-opens the window an install admitted before a pack
//!      update was supposed to close.
//!
//! Which claim a writer takes is a decision this guard pins per `GATED` entry,
//! not one it can derive: a per-item writer (one mod or asset, one pack file,
//! one overlay row) takes the SHARED claim — the Mods browser runs two installs
//! on one instance at once, and none of these writers has ever been refused
//! while the game runs — while a rewrite of the instance's pack or tree takes
//! the exclusive one. Moving an entry between spellings changes user-visible
//! behaviour; do it in the same change as the reason.
//!
//! Guardrail, not a static analyzer — same framing as
//! `structural_no_heavy_sync_command.rs`. Named gaps:
//!
//!   - ORDER. The scan is lexical: it proves the gate is CALLED, not that it
//!     runs before the first write or `.await`. Review owns ordering — except
//!     for a claim, which rule 3 pins to span the body's awaits. A claim moved
//!     below a SYNC read, or released before a SYNC write, is still invisible.
//!   - `commands/instances.rs` and `commands/logs.rs` are NOT ratcheted — most
//!     of their commands edit `instance.json` fields no claimer reads, or read
//!     logs — so a writer there that must refuse under a claim, or take one, is
//!     added to `GATED` by hand (`detach_instance_pack`, `set_instance_loader`,
//!     `clone_instance`, `execute_repair`).
//!   - `READ_ONLY` means "writes nothing a claim protects". Several of those
//!     reads persist launcher-owned registry metadata (the `installed::list`
//!     reconcile, display-name and identity backfills); that is declared, not
//!     gated — see the note above the `commands/mods.rs` entries.
//!   - A gate reached through a helper this file does not name is invisible;
//!     add the helper's call-site spelling to `GATE_SPELLINGS` together with
//!     the helper, and list the helper itself in `GATED` (as `guard` is).
//!   - Whole-line `//` comments are exempt; a body whose only mention of the
//!     gate is prose therefore FAILS — which is the point.
//!   - A fn is located by a COLUMN-0 signature and its body ends at the first
//!     `}` in column 0 at or after it, so an indented test fn of the same name
//!     is never mistaken for it. Sound for rustfmt'd top-level fns, which is
//!     all this tree contains.

use std::fs;
use std::path::{Path, PathBuf};

/// The single gate: `crate::instances::maintenance::write_allowed(id)`.
const WRITE_GATE: &str = "maintenance::write_allowed(";

/// The claim-only spelling: `crate::instances::maintenance::maintenance_is_active(id)`.
/// Launch's half of the Dekker pairing, and `delete_backup`'s gate.
const ACTIVE_GATE: &str = "maintenance_is_active(";

/// A command that takes the claim itself is gated by construction — the
/// migration command's spelling, `crate::instances::maintenance::maintenance_begin(id)`.
const CLAIM_GATE: &str = "maintenance_begin(";

/// The long writer's claim: `crate::instances::maintenance::claim_write(id)` —
/// claim first, then refuse while running or starting. Listed in `GATED` as a
/// helper (`claim_write_with` must take the claim, `claim_write` must pass the
/// real running/starting predicate).
const CLAIM_WRITE_GATE: &str = "maintenance::claim_write(";

/// A per-item content writer's SHARED claim:
/// `crate::instances::maintenance::claim_shared_write(id)` — admitted alongside
/// other item writers and while the game runs, refused under an exclusive
/// claim, and refusing every exclusive claim while held.
const SHARED_WRITE_GATE: &str = "maintenance::claim_shared_write(";

/// `commands/datapacks.rs` routes its commands through a file-local
/// `fn guard(instance_id)`, itself listed in `GATED` as a `WRITE_GATE` site.
const DATAPACKS_DELEGATE: &str = "guard(&";

/// Spellings a command body may carry to count as gated. Every entry is a
/// call-site prefix, so a bare mention in prose (a comment line) never counts
/// and a helper with a different name never matches by accident.
const GATE_SPELLINGS: &[&str] = &[
    WRITE_GATE,
    ACTIVE_GATE,
    CLAIM_GATE,
    CLAIM_WRITE_GATE,
    SHARED_WRITE_GATE,
    DATAPACKS_DELEGATE,
];

/// The spellings that TAKE a claim — the `GATED` entries rule 3 (CLAIM HELD)
/// applies to.
const CLAIM_SPELLINGS: &[&str] = &[CLAIM_GATE, CLAIM_WRITE_GATE, SHARED_WRITE_GATE];

/// `(path relative to src/, fn name, required spelling, why it is a writer)`.
const GATED: &[(&str, &str, &str, &str)] = &[
    (
        "commands/worlds.rs",
        "backup_world",
        WRITE_GATE,
        "zips the world tree a migration may be renaming or copying",
    ),
    (
        "commands/worlds.rs",
        "restore_backup",
        WRITE_GATE,
        "swaps or copies a world into saves/ — the directory a migration stages in",
    ),
    (
        "commands/worlds.rs",
        "delete_world",
        WRITE_GATE,
        "removes the world tree and its backup set — both of which a Move relocates",
    ),
    (
        "commands/worlds.rs",
        "world_import",
        WRITE_GATE,
        "places a new world in saves/ and may take the name a migration is about to pick",
    ),
    (
        "commands/worlds.rs",
        "recover_stranded_world",
        WRITE_GATE,
        "renames a .tmp-* stage — a migration's own live stage included — into saves/",
    ),
    (
        "commands/worlds.rs",
        "delete_backup",
        ACTIVE_GATE,
        "deletes one zip from a backup set `move_set_at` may be renaming file by file; \
         claim-only, so deleting a backup while the game runs stays allowed as before",
    ),
    (
        "commands/datapacks.rs",
        "guard",
        WRITE_GATE,
        "the file-local delegate every datapack writer calls — the gate's one spelling there",
    ),
    (
        "commands/instances.rs",
        "delete_instance",
        WRITE_GATE,
        "remove_dir_all of the instance a migration is reading from or writing into",
    ),
    (
        "commands/instances.rs",
        "rename_instance_dir",
        WRITE_GATE,
        "renames the instance root — every path a running migration holds goes stale",
    ),
    (
        "commands/instances.rs",
        "change_instance_mc",
        WRITE_GATE,
        "changes the version the migration plan's verdict was computed against",
    ),
    (
        "commands/instances.rs",
        "clone_instance",
        CLAIM_WRITE_GATE,
        "copies saves/ and mods/ wholesale for minutes — a half-staged world would be \
         cloned mid-copy, and a pack update, mod migration or world migration that \
         starts DURING the copy must be refused, which an entry-only check cannot do",
    ),
    (
        "commands/worlds.rs",
        "world_migrate",
        CLAIM_GATE,
        "holds both instances for the whole move or copy; listed so rule 3 pins that \
         its claims are held, not only taken",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_apply_update",
        CLAIM_WRITE_GATE,
        "swaps pack files in mods/ and rewrites pack_origin and instance.json over \
         minutes of downloads and installs — Update, Switch version and the migration \
         dialog's platform restore all run through it",
    ),
    (
        "commands/mods.rs",
        "mods_apply_mc_migration",
        CLAIM_WRITE_GATE,
        "replaces, installs, disables and removes jars in mods/ row by row — a pack \
         update or a launch between two rows sees a half-migrated mod set",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_reimport_overrides",
        CLAIM_WRITE_GATE,
        "overwrites overrides/ (config, options, jars, packs) across the instance and \
         rewrites pack_origin — a pack-level rewrite like the update; exclusive, and \
         refused while the game runs, which rewrites those config files on exit",
    ),
    (
        "commands/mods.rs",
        "mods_install_with_deps",
        SHARED_WRITE_GATE,
        "resolves, downloads and commits a mod and its dependency closure into mods/ and \
         the registry — a pack update or mod migration starting meanwhile must refuse; \
         shared, because Browse runs two installs on one instance at once",
    ),
    (
        "commands/mods.rs",
        "mods_update_one",
        SHARED_WRITE_GATE,
        "removes the old jar and installs the new one plus its dependencies",
    ),
    (
        "commands/mods.rs",
        "mods_install_missing_required",
        SHARED_WRITE_GATE,
        "resolves, downloads and installs one dependency jar",
    ),
    (
        "commands/mods.rs",
        "mods_install_local",
        SHARED_WRITE_GATE,
        "places a dropped jar in mods/ and adds its registry row",
    ),
    (
        "commands/mods.rs",
        "mods_disable",
        SHARED_WRITE_GATE,
        "renames a jar in mods/ and flips its registry row",
    ),
    (
        "commands/mods.rs",
        "mods_enable",
        SHARED_WRITE_GATE,
        "renames a jar in mods/ and flips its registry row",
    ),
    (
        "commands/mods.rs",
        "mods_uninstall",
        SHARED_WRITE_GATE,
        "removes a jar from mods/ and its registry row",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_restore_file",
        SHARED_WRITE_GATE,
        "re-installs one pack file into mods/ or an asset folder",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_resolve_missing_with",
        SHARED_WRITE_GATE,
        "read-modify-writes pack_origin — overlapping a pack update it writes the old \
         origin back over the new one",
    ),
    (
        "commands/assets.rs",
        "asset_install",
        SHARED_WRITE_GATE,
        "downloads a resource or shader pack into the instance and its registry — the \
         folders a pack update swaps and a clone copies",
    ),
    (
        "commands/assets.rs",
        "asset_install_local",
        SHARED_WRITE_GATE,
        "places a dropped resource or shader pack and adds its registry row",
    ),
    (
        "commands/assets.rs",
        "asset_update_one",
        SHARED_WRITE_GATE,
        "installs the new pack version and removes the superseded file and row",
    ),
    (
        "commands/assets.rs",
        "asset_uninstall",
        SHARED_WRITE_GATE,
        "removes a resource or shader pack file and its registry row",
    ),
    (
        "commands/logs.rs",
        "execute_repair",
        SHARED_WRITE_GATE,
        "a log-diagnosis repair disables, removes or reinstalls a mod — the same writes \
         as the Mods view's toggle and install",
    ),
    (
        "commands/instances.rs",
        "detach_instance_pack",
        WRITE_GATE,
        "clears the instance's pack identity, the fields a pack update writes last — \
         overlapping it, the detach is silently undone",
    ),
    (
        "commands/instances.rs",
        "set_instance_loader",
        WRITE_GATE,
        "changes the loader a pack update, a mod migration and every install resolve \
         against — the sibling of change_instance_mc",
    ),
    (
        "instances/maintenance.rs",
        "maintenance_begin",
        "sharing.contains_key(",
        "the exclusive claim must refuse while a per-item writer is in flight — the \
         half an entry check on the item writer's side cannot give",
    ),
    (
        "instances/maintenance.rs",
        "claim_shared_write",
        "held.contains(",
        "a per-item writer must refuse while a long operation holds the instance",
    ),
    (
        "instances/maintenance.rs",
        "claim_write_with",
        CLAIM_GATE,
        "the long writers' helper: it must take the claim itself, before the \
         running/starting check (the Dekker pairing with launch::start)",
    ),
    (
        "instances/maintenance.rs",
        "claim_write",
        "is_running(",
        "the long writers' helper must refuse a running instance",
    ),
    (
        "instances/maintenance.rs",
        "claim_write",
        "is_starting(",
        "the long writers' helper must refuse mid-launch too — is_running stays false \
         for the whole spawn pipeline, which is the half the old mod-migration check lacked",
    ),
    (
        "commands/instances.rs",
        "launch_instance",
        ACTIVE_GATE,
        "must not refuse on is_running (claim_start refuses a same-id relaunch inside \
         start); the maintenance claim alone is checked at command entry",
    ),
    (
        "launch/spawn.rs",
        "start",
        ACTIVE_GATE,
        "the re-check after claim_start — the launch half of the Dekker pairing with \
         the migration's claim-then-check",
    ),
];

/// Commands in the ratcheted files that touch nothing a migration moves:
/// `(path relative to src/, fn name, why it needs no gate)`.
const READ_ONLY: &[(&str, &str, &str)] = &[
    (
        "commands/worlds.rs",
        "list_worlds",
        "stats and lists; a .tmp-* stage is dot-hidden from it",
    ),
    (
        "commands/worlds.rs",
        "list_world_names",
        "sidebar listing, folder names only",
    ),
    ("commands/worlds.rs", "list_backups", "lists zips, touches none"),
    (
        "commands/worlds.rs",
        "open_saves_folder",
        "create_dir_all of saves/ then the OS opener — creates nothing a migration can lose",
    ),
    (
        "commands/worlds.rs",
        "open_backups_folder",
        "create_dir_all of backups/<world>/ then the OS opener; an empty set dir is nothing to lose",
    ),
    (
        "commands/worlds.rs",
        "list_orphaned_backup_worlds",
        "scan only",
    ),
    (
        "commands/worlds.rs",
        "list_stranded_worlds",
        "scan only — it is how a stranded stage is FOUND; recovery is the gated command",
    ),
    (
        "commands/worlds.rs",
        "world_migration_plan",
        "reads level.dat, the target jar and both installed-mod lists; writes nothing a migration could race, and must stay available while an instance runs",
    ),
    (
        "commands/datapacks.rs",
        "datapacks_list_library",
        "read-only view; the registry reconcile it may persist is launcher-owned metadata \
         the game never reads (see the file's module doc)",
    ),
    (
        "commands/datapacks.rs",
        "datapacks_list_for_world",
        "read-only view, same reconcile caveat",
    ),
    (
        "commands/datapacks.rs",
        "datapacks_check_updates",
        "network query over the library listing; installs nothing",
    ),
    // commands/mods.rs. "Reconcile" below means `installed::list` (or a
    // display-name backfill) persisting launcher-owned registry metadata after
    // reading `mods/` — gating a read on that would make opening the Installed
    // tab refuse the next pack update. Its lost-update window against a
    // concurrent writer is the registry's missing read-modify-write lock, not a
    // claim question.
    ("commands/mods.rs", "mods_search", "platform search; no instance"),
    (
        "commands/mods.rs",
        "mods_filter_satisfying",
        "pure version-range filter; no instance",
    ),
    ("commands/mods.rs", "mods_project", "platform metadata; no instance"),
    ("commands/mods.rs", "mods_projects", "platform metadata; no instance"),
    ("commands/mods.rs", "mods_versions", "platform metadata; no instance"),
    ("commands/mods.rs", "mods_changelog", "platform metadata; no instance"),
    (
        "commands/mods.rs",
        "mods_plugin_versions",
        "platform metadata; no instance",
    ),
    (
        "commands/mods.rs",
        "mods_datapack_versions",
        "platform metadata; no instance",
    ),
    (
        "commands/mods.rs",
        "mods_resolve_deps",
        "platform dependency query; no instance",
    ),
    (
        "commands/mods.rs",
        "optimise_resolve",
        "plans the Optimise set from the installed list (reconcile only); the installs are gated",
    ),
    (
        "commands/mods.rs",
        "mods_resolve_install_plan",
        "plans an install from the installed list (reconcile only); the install is gated",
    ),
    (
        "commands/mods.rs",
        "mods_list_installed",
        "the installed list — reconcile and display-name backfill only",
    ),
    (
        "commands/mods.rs",
        "mods_check_updates",
        "network query over the installed list (reconcile only); updates nothing",
    ),
    (
        "commands/mods.rs",
        "mods_pack_origin_summary",
        "reads pack_origin",
    ),
    (
        "commands/mods.rs",
        "mods_enrich_pack_mods",
        "backfills platform identity into registry rows — launcher metadata, auto-run when \
         the Installed tab opens; gating it would refuse the next pack update for no visible \
         operation",
    ),
    (
        "commands/mods.rs",
        "mods_inspect_local",
        "reads a dropped jar and judges it (reconcile only); the install is gated",
    ),
    (
        "commands/mods.rs",
        "check_instance_mod_compat",
        "network query over the installed list (reconcile only)",
    ),
    (
        "commands/mods.rs",
        "scan_instance_mod_compat",
        "reads jars (reconcile only); persists the launcher's jar-scan cache, which lives \
         in the app dir, not the instance",
    ),
    (
        "commands/mods.rs",
        "mods_plan_mc_migration",
        "plans the migration; the apply is gated",
    ),
    (
        "commands/mods.rs",
        "mods_find_orphans",
        "reads the registry's dependency edges (reconcile only)",
    ),
    (
        "commands/mods.rs",
        "mods_resolve_dep_names",
        "name lookup over the installed list and platform metadata (reconcile only)",
    ),
    (
        "commands/mods.rs",
        "mods_dependency_graph",
        "network query over the installed list (reconcile only)",
    ),
    (
        "commands/mods.rs",
        "instance_dependency_preflight",
        "reads jars and the registry — reconcile and display-name backfill only",
    ),
    // commands/modpack_cmds.rs
    (
        "commands/modpack_cmds.rs",
        "modpack_inspect",
        "reads an archive the user picked; no instance",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_import",
        "creates a NEW instance directory no other operation can hold yet",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_search",
        "platform search; no instance",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_resolve_url",
        "parses a URL; no instance",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_fetch_to_temp",
        "downloads an archive to a temp file; no instance",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_status",
        "reads pack_origin against the installed list (reconcile only)",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_get_versions",
        "platform metadata; no instance",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_project",
        "platform metadata; no instance",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_source_caps",
        "static capabilities; no instance",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_update_status",
        "network query against the instance's pack identity",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpacks_check_updates",
        "network query against each instance's pack identity",
    ),
    (
        "commands/modpack_cmds.rs",
        "modpack_compute_update",
        "diffs a downloaded archive against pack_origin; the apply is gated",
    ),
    (
        "commands/modpack_cmds.rs",
        "export_preview",
        "reads the instance to preview an export",
    ),
    (
        "commands/modpack_cmds.rs",
        "export_modpack",
        "reads the instance into an archive outside it — unclaimed, so a concurrent \
         writer can tear the export (a named gap; the instance itself is untouched)",
    ),
    // commands/assets.rs
    (
        "commands/assets.rs",
        "assets_list",
        "the installed packs — backfills the assets registry from pack_origin only",
    ),
    (
        "commands/assets.rs",
        "assets_check_updates",
        "network query over the installed packs",
    ),
];

/// Files whose every `#[tauri::command]` must be gated or declared read-only.
const RATCHETED: &[&str] = &[
    "commands/worlds.rs",
    "commands/datapacks.rs",
    "commands/mods.rs",
    "commands/modpack_cmds.rs",
    "commands/assets.rs",
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

/// The name of the fn a COLUMN-0 signature line defines, for `fn`, `pub fn`,
/// `pub(crate) fn`, `pub(super) fn` and their `async` forms. `None` for
/// anything else — including an indented signature (a test fn, a closure, a
/// nested helper), so a same-named fn inside `mod tests` is never the one
/// found.
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

/// Index of the line closing the top-level fn opened at `sig`: the first `}`
/// in column 0 at or after it. Sound for rustfmt'd top-level fns, which is all
/// this tree contains (same rule as `structural_no_heavy_sync_command.rs`).
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

/// True when `line` is a `#[tauri::command]` attribute, argumented or not.
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
        // Walk forward past any further attributes to the signature.
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

/// True when a CODE line of `lines[sig..=end]` — the signature line excluded,
/// whole-line `//` comments excluded — contains `needle`.
fn body_carries(lines: &[String], sig: usize, end: usize, needle: &str) -> bool {
    lines[sig..=end]
        .iter()
        .skip(1)
        .filter(|l| !l.trim_start().starts_with("//"))
        .any(|l| l.contains(needle))
}

/// Indices of the CODE lines of `lines[sig..=end]` — signature and whole-line
/// `//` comments excluded, same filter as [`body_carries`] — containing
/// `needle`, in order.
fn code_lines_with(lines: &[String], sig: usize, end: usize, needle: &str) -> Vec<usize> {
    (sig + 1..=end)
        .filter(|&i| {
            let l = &lines[i];
            !l.trim_start().starts_with("//") && l.contains(needle)
        })
        .collect()
}

/// The pattern a `let` on `line` binds, when it is not the bare `_` — i.e. the
/// value lives in a binding until the end of its scope. `let _ = claim` drops
/// the guard on the spot (`None`); `let _claim`, `let claim`, `let mut claim`
/// and `let Some(claim) = … else` all hold it.
fn let_binding(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("let ")?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let pattern_end = rest
        .find(|c: char| c.is_whitespace() || c == '=' || c == ':')
        .unwrap_or(rest.len());
    let pattern = &rest[..pattern_end];
    (!pattern.is_empty() && pattern != "_").then_some(pattern)
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Rule 3 on one fn body: `None` when the claim spelled `needle` is held — or
/// absent, which is rule 1's report, not this one's — else the reason it is
/// not held. Held means:
///
///   - bound to a named local, on the claim's own line or — when rustfmt
///     breaks a long `let claim =` — on the code line directly above it,
///     ending in `=`;
///   - taken before the body's first `.await`;
///   - if the body releases it with `drop(<name>)` at the claim's own
///     indentation (the fn's main path; a `drop` nested in an early-return
///     branch is deeper and not counted), that release comes after the body's
///     LAST `.await`. A sync write after the release is still invisible.
fn claim_not_held(lines: &[String], sig: usize, end: usize, needle: &str) -> Option<String> {
    let at = *code_lines_with(lines, sig, end, needle).first()?;
    let binding_line = if let_binding(&lines[at]).is_some() {
        Some(at)
    } else {
        (at > sig + 1).then_some(at - 1).filter(|&prev| {
            let_binding(&lines[prev]).is_some() && lines[prev].trim_end().ends_with('=')
        })
    };
    let Some(binding_line) = binding_line else {
        return Some(format!(
            "line {}: the claim is not bound to a named local, so its guard is dropped — \
             and the instance released — before the write runs",
            at + 1
        ));
    };
    let awaits = code_lines_with(lines, sig, end, ".await");
    if let Some(&first_await) = awaits.first() {
        if first_await < at {
            return Some(format!(
                "line {}: the claim is taken after the first `.await` (line {}), so what \
                 the body read or awaited before it is not covered",
                at + 1,
                first_await + 1
            ));
        }
    }
    let name = let_binding(&lines[binding_line])?;
    let main_path = indent(&lines[binding_line]);
    let early_release = code_lines_with(lines, at, end, &format!("drop({name})"))
        .into_iter()
        .find(|&i| indent(&lines[i]) == main_path);
    if let (Some(release), Some(&last_await)) = (early_release, awaits.last()) {
        if release < last_await {
            return Some(format!(
                "line {}: the claim is released before the body's last `.await` (line {}), \
                 so the work after the release runs unprotected",
                release + 1,
                last_await + 1
            ));
        }
    }
    None
}

#[test]
fn every_listed_writer_opens_with_the_gate() {
    let mut violations = Vec::new();
    for (rel, name, needle, why) in GATED {
        let lines = read_lines(rel);
        let Some((sig, end)) = locate_fn(&lines, name) else {
            violations.push(format!(
                "{rel} — `fn {name}` not found at column 0: renamed or removed? \
                 Update GATED together with the code (it is a writer: {why})."
            ));
            continue;
        };
        if !body_carries(&lines, sig, end, needle) {
            violations.push(format!(
                "{rel}:{} — `{name}` carries no `{needle}` on a code line of its body ({why})",
                sig + 1
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "a writer the maintenance claim must hold off does not open with the \
         maintenance gate. Every short instance, world and datapack writer calls \
         `crate::instances::maintenance::write_allowed(&id)?` (launch and \
         `delete_backup`: `maintenance_is_active(&id)`), every long writer \
         takes `crate::instances::maintenance::claim_write(&id)?`, and every \
         per-item mod, asset or pack-file writer takes \
         `crate::instances::maintenance::claim_shared_write(&id)?`, on a code line \
         of its body — a comment naming the gate is not a gate. If a listed fn was \
         renamed, update GATED in the same change.\n{}",
        violations.join("\n"),
    );
}

#[test]
fn every_listed_claim_is_held_for_the_write() {
    let mut violations = Vec::new();
    for (rel, name, needle, why) in GATED {
        if !CLAIM_SPELLINGS.contains(needle) {
            continue;
        }
        let lines = read_lines(rel);
        // A missing fn or spelling is rule 1's report; nothing to add here.
        let Some((sig, end)) = locate_fn(&lines, name) else {
            continue;
        };
        if let Some(reason) = claim_not_held(&lines, sig, end, needle) {
            violations.push(format!("{rel} — `{name}` {reason} ({why})"));
        }
    }
    assert!(
        violations.is_empty(),
        "a writer takes a maintenance claim but does not hold it for the \
         write. Bind the claim to a named local — `let claim = \
         crate::instances::maintenance::claim_write(&id)?;` (or `let write = \
         …::claim_shared_write(&id)?;`) — as the first thing \
         the command does, before any `.await`, and `drop(claim)` after the last \
         write (in any case after the last `.await`). If a read genuinely must \
         precede the claim, redo it under the claim rather than moving the claim \
         down.\n{}",
        violations.join("\n"),
    );
}

#[test]
fn every_command_in_a_ratcheted_file_is_gated_or_declared_read_only() {
    let mut violations = Vec::new();
    for rel in RATCHETED {
        let lines = read_lines(rel);
        for (name, sig, end) in commands_in(&lines) {
            let declared_read_only = READ_ONLY
                .iter()
                .any(|(f, n, _)| f == rel && *n == name.as_str());
            if declared_read_only {
                continue;
            }
            let gated = GATE_SPELLINGS
                .iter()
                .any(|needle| body_carries(&lines, sig, end, needle));
            if !gated {
                violations.push(format!(
                    "{rel}:{} — `{name}` is neither gated nor declared read-only",
                    sig + 1
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "a #[tauri::command] in a ratcheted file must choose, in code: open with \
         `crate::instances::maintenance::write_allowed(&id)?` (or the datapacks \
         delegate `guard(&id)?`), take a claim — `claim_write` for a rewrite of \
         the instance's pack or tree, `claim_shared_write` for a per-item mod, \
         asset or pack-file writer, `maintenance_begin` for a migration — or be \
         declared in READ_ONLY with the reason it writes nothing a claim \
         protects.\n{}",
        violations.join("\n"),
    );
}

#[test]
fn read_only_entries_still_match() {
    let mut stale = Vec::new();
    for (rel, name, _) in READ_ONLY {
        if !RATCHETED.contains(rel) {
            stale.push(format!(
                "{rel} — not a ratcheted file; a READ_ONLY entry there means nothing"
            ));
            continue;
        }
        let lines = read_lines(rel);
        let Some((sig, end)) = locate_fn(&lines, name) else {
            stale.push(format!("{rel} — `fn {name}` no longer exists"));
            continue;
        };
        let is_command = commands_in(&lines)
            .iter()
            .any(|(n, _, _)| n.as_str() == *name);
        if !is_command {
            stale.push(format!("{rel} — `{name}` is not a #[tauri::command]"));
        }
        if GATE_SPELLINGS
            .iter()
            .any(|needle| body_carries(&lines, sig, end, needle))
        {
            stale.push(format!(
                "{rel}:{} — `{name}` now carries a gate; drop its READ_ONLY entry",
                sig + 1
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "READ_ONLY entry no longer describes the code — fix the entry together \
         with the code.\n{}",
        stale.join("\n"),
    );
}

/// The matchers, pinned directly — same practice as
/// `structural_no_heavy_sync_command.rs`: the scans above only prove what the
/// tree happens to contain today; these pin the decisions the tree does not
/// exercise.
#[cfg(test)]
mod matchers {
    use super::*;

    fn lines(src: &[&str]) -> Vec<String> {
        src.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fn_names_cover_every_top_level_spelling() {
        assert_eq!(
            top_level_fn_name("pub async fn backup_world("),
            Some("backup_world")
        );
        assert_eq!(
            top_level_fn_name("pub fn delete_backup("),
            Some("delete_backup")
        );
        assert_eq!(
            top_level_fn_name("fn guard(instance_id: &str) -> Result<(), crate::error::Error> {"),
            Some("guard")
        );
        assert_eq!(top_level_fn_name("pub(crate) fn f("), Some("f"));
        assert_eq!(
            top_level_fn_name("pub(super) async fn fetch("),
            Some("fetch")
        );
        assert_eq!(
            top_level_fn_name("pub fn generic<T>(x: T)"),
            Some("generic")
        );
        // Indented: a test fn or a nested helper, never the top-level one.
        assert_eq!(top_level_fn_name("    fn guard(x: bool) -> bool {"), None);
        assert_eq!(top_level_fn_name("    app: tauri::AppHandle,"), None);
        assert_eq!(top_level_fn_name("#[tauri::command]"), None);
        assert_eq!(top_level_fn_name("mod tests {"), None);
    }

    #[test]
    fn a_same_named_test_fn_is_not_mistaken_for_the_command() {
        let l = lines(&[
            "pub fn delete_backup(app: AppHandle) -> Result<()> {",
            "    crate::worlds::backup::delete_backup(&app)",
            "}",
            "#[cfg(test)]",
            "mod tests {",
            "    fn delete_backup() {",
            "        maintenance_is_active(\"x\");",
            "    }",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "delete_backup").expect("top-level fn found");
        assert_eq!((sig, end), (0, 2));
        assert!(!body_carries(&l, sig, end, ACTIVE_GATE));
    }

    #[test]
    fn a_gate_named_only_in_a_comment_is_not_a_gate() {
        let l = lines(&[
            "#[tauri::command]",
            "#[specta::specta]",
            "pub async fn backup_world(app: AppHandle, id: String) -> Result<()> {",
            "    // maintenance::write_allowed(&id) is checked by the caller.",
            "    crate::worlds::backup::backup_world(&app, &id).await",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "backup_world").expect("found");
        assert!(!body_carries(&l, sig, end, WRITE_GATE));
    }

    #[test]
    fn a_gate_on_a_code_line_counts_even_with_a_trailing_comment() {
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    crate::instances::maintenance::write_allowed(&id)?; // A5",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        assert!(body_carries(&l, sig, end, WRITE_GATE));
    }

    #[test]
    fn the_signature_line_is_not_scanned() {
        // A fn NAMED after the gate carries nothing by name alone.
        let l = lines(&[
            "pub fn maintenance_is_active(id: &str) -> bool {",
            "    false",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "maintenance_is_active").expect("found");
        assert!(!body_carries(&l, sig, end, ACTIVE_GATE));
    }

    #[test]
    fn commands_are_found_past_intervening_attributes() {
        let l = lines(&[
            "#[tauri::command]",
            "#[specta::specta]",
            "#[allow(clippy::too_many_arguments)]",
            "pub async fn a(app: AppHandle) -> Result<()> {",
            "    Ok(())",
            "}",
            "",
            "fn not_a_command() {}",
            "",
            "#[tauri::command(rename_all = \"snake_case\")]",
            "pub fn b(app: AppHandle) -> Result<()> {",
            "    Ok(())",
            "}",
        ]);
        assert_eq!(
            commands_in(&l),
            vec![("a".to_string(), 3, 5), ("b".to_string(), 10, 12)]
        );
    }

    #[test]
    fn a_named_let_is_told_apart_from_the_discarding_underscore() {
        assert_eq!(
            let_binding("    let claim = claim_write(&id)?;"),
            Some("claim")
        );
        assert_eq!(
            let_binding("    let _claim = claim_write(&id)?;"),
            Some("_claim")
        );
        assert_eq!(
            let_binding("    let mut claim = claim_write(&id)?;"),
            Some("claim")
        );
        assert_eq!(
            let_binding("    let claim: MaintenanceGuard = claim_write(&id)?;"),
            Some("claim")
        );
        assert_eq!(
            let_binding("    let Some(claim) = maintenance_begin(&id) else {"),
            Some("Some(claim)")
        );
        assert_eq!(let_binding("    let _ = claim_write(&id)?;"), None);
        assert_eq!(let_binding("    let _= claim_write(&id)?;"), None);
        assert_eq!(let_binding("    claim_write(&id)?;"), None);
        assert_eq!(let_binding("    // let claim = claim_write(&id)?;"), None);
        assert_eq!(let_binding("    letter = 1;"), None);
    }

    #[test]
    fn a_claim_released_before_the_last_await_is_not_held() {
        // The "keep Play available during the downloads" regression spelled
        // as an early release instead of a late claim.
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    let claim = crate::instances::maintenance::claim_write(&id)?;",
            "    let diff = compute(&root).await?;",
            "    drop(claim);",
            "    download(&diff).await?;",
            "    apply(&diff).await?;",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        let reason = claim_not_held(&l, sig, end, CLAIM_WRITE_GATE);
        assert!(
            reason
                .as_deref()
                .is_some_and(|r| r.contains("released before the body's last `.await`")),
            "got {reason:?}"
        );
    }

    #[test]
    fn a_release_on_an_early_return_branch_or_after_the_last_await_is_held() {
        // A `drop` nested in an early-return branch sits deeper than the
        // claim and is not the main path's release; the main path's release
        // after the last `.await` is the shape every long writer uses.
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    let claim = crate::instances::maintenance::claim_write(&id)?;",
            "    if nothing_to_do() {",
            "        drop(claim);",
            "        return Ok(());",
            "    }",
            "    apply(&root).await?;",
            "    journal(&root);",
            "    drop(claim);",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        assert_eq!(claim_not_held(&l, sig, end, CLAIM_WRITE_GATE), None);
    }

    #[test]
    fn a_named_claim_before_the_first_await_is_held() {
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    // Reads below are computed under the claim.",
            "    let claim = crate::instances::maintenance::claim_write(&id)?;",
            "    let origin = get_pack_origin(&root).await?;",
            "    drop(claim);",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        assert_eq!(claim_not_held(&l, sig, end, CLAIM_WRITE_GATE), None);
    }

    #[test]
    fn a_claim_rustfmt_broke_below_its_let_is_held() {
        let l = lines(&[
            "pub async fn f(from_instance: String) -> Result<()> {",
            "    let from_claim =",
            "        crate::instances::maintenance::maintenance_begin(&from_instance)",
            "            .ok_or(crate::error::Error::InstanceBusy)?;",
            "    work().await",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        assert_eq!(claim_not_held(&l, sig, end, CLAIM_GATE), None);
    }

    #[test]
    fn a_claim_bound_to_underscore_or_never_bound_is_not_held() {
        for claim_line in [
            "    let _ = crate::instances::maintenance::claim_write(&id)?;",
            "    crate::instances::maintenance::claim_write(&id)?;",
        ] {
            let l = lines(&[
                "pub async fn f(id: String) -> Result<()> {",
                claim_line,
                "    swap_mods(&root).await?;",
                "    Ok(())",
                "}",
            ]);
            let (sig, end) = locate_fn(&l, "f").expect("found");
            let reason = claim_not_held(&l, sig, end, CLAIM_WRITE_GATE);
            assert!(
                reason.as_deref().is_some_and(|r| r.contains("named local")),
                "{claim_line:?} must be reported as not held, got {reason:?}"
            );
        }
    }

    #[test]
    fn a_claim_taken_after_the_first_await_is_late() {
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    let installed_before = list(&root).await?;",
            "    let claim = crate::instances::maintenance::claim_write(&id)?;",
            "    apply(&installed_before).await?;",
            "    drop(claim);",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        let reason = claim_not_held(&l, sig, end, CLAIM_WRITE_GATE);
        assert!(
            reason
                .as_deref()
                .is_some_and(|r| r.contains("after the first `.await`")),
            "got {reason:?}"
        );
    }

    #[test]
    fn an_await_named_only_in_a_comment_does_not_make_a_claim_late() {
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    // Taken before the first .await on purpose.",
            "    let claim = crate::instances::maintenance::claim_write(&id)?;",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        assert_eq!(claim_not_held(&l, sig, end, CLAIM_WRITE_GATE), None);
    }

    #[test]
    fn the_shared_and_exclusive_claim_spellings_never_match_each_other() {
        // `GATED` pins WHICH claim a writer takes. If one spelling were a
        // substring of the other, a per-item writer moved to the exclusive
        // claim (or back) would still satisfy its old entry, and the
        // behaviour change would pass unseen.
        let exclusive = "    let claim = crate::instances::maintenance::claim_write(&id)?;";
        let shared = "    let write = crate::instances::maintenance::claim_shared_write(&id)?;";
        assert!(exclusive.contains(CLAIM_WRITE_GATE) && !exclusive.contains(SHARED_WRITE_GATE));
        assert!(shared.contains(SHARED_WRITE_GATE) && !shared.contains(CLAIM_WRITE_GATE));
    }

    #[test]
    fn a_shared_claim_taken_outside_an_interactive_wrapper_is_held() {
        // The shape of the per-item installers: the claim is taken before the
        // `with_interactive(async move { … })` wrapper, whose own `.await` is the
        // body's last one, and released after it.
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    let write = crate::instances::maintenance::claim_shared_write(&id)?;",
            "    let installed = crate::network::throttle::with_interactive(async move {",
            "        resolve(&id).await?;",
            "        commit(&id).await",
            "    })",
            "    .await;",
            "    drop(write);",
            "    installed",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        assert_eq!(claim_not_held(&l, sig, end, SHARED_WRITE_GATE), None);
    }

    #[test]
    fn a_shared_claim_released_before_the_commit_is_not_held() {
        // "Only the commit touches mods/" — but an install that drops its claim
        // after resolving lets a pack update start between the resolution it
        // pruned against and the jars it then lands.
        let l = lines(&[
            "pub async fn f(id: String) -> Result<()> {",
            "    let write = crate::instances::maintenance::claim_shared_write(&id)?;",
            "    let plan = resolve(&id).await?;",
            "    drop(write);",
            "    commit(&plan).await?;",
            "    Ok(())",
            "}",
        ]);
        let (sig, end) = locate_fn(&l, "f").expect("found");
        let reason = claim_not_held(&l, sig, end, SHARED_WRITE_GATE);
        assert!(
            reason
                .as_deref()
                .is_some_and(|r| r.contains("released before the body's last `.await`")),
            "got {reason:?}"
        );
    }

    #[test]
    fn a_new_ungated_command_in_a_ratcheted_file_would_fail() {
        // The ratchet's decision on a synthetic file: one gated directly, one
        // through the datapacks delegate, one that takes the claim itself,
        // one long writer through `claim_write`, one per-item writer through
        // `claim_shared_write`, and one that nobody gated or declared.
        let l = lines(&[
            "#[tauri::command]",
            "pub async fn gated(id: String) -> Result<()> {",
            "    crate::instances::maintenance::write_allowed(&id)?;",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn delegated(id: String) -> Result<()> {",
            "    guard(&id)?;",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn claims(id: String) -> Result<()> {",
            "    let _claim = crate::instances::maintenance::maintenance_begin(&id)",
            "        .ok_or(crate::error::Error::InstanceBusy)?;",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn long_writer(id: String) -> Result<()> {",
            "    let claim = crate::instances::maintenance::claim_write(&id)?;",
            "    drop(claim);",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn item_writer(id: String) -> Result<()> {",
            "    let write = crate::instances::maintenance::claim_shared_write(&id)?;",
            "    drop(write);",
            "    Ok(())",
            "}",
            "#[tauri::command]",
            "pub async fn silent(id: String) -> Result<()> {",
            "    crate::worlds::something_that_writes(&id)",
            "}",
        ]);
        let ungated: Vec<String> = commands_in(&l)
            .into_iter()
            .filter(|(_, sig, end)| {
                !GATE_SPELLINGS
                    .iter()
                    .any(|n| body_carries(&l, *sig, *end, n))
            })
            .map(|(name, _, _)| name)
            .collect();
        assert_eq!(ungated, vec!["silent".to_string()]);
    }
}
