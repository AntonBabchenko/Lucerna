// Per-instance Overview statistics, extracted from `+page.svelte` so the
// page no longer owns this fetch-on-switch state directly. A small factory
// (not a module singleton) — the page holds exactly one, but a factory keeps
// the unit testable with a fresh instance per case.
//
// Same rune idiom as the other `.svelte.ts` stores: `$state` read through
// getters at call time stays reactive in Svelte 5 templates. No `$effect`
// lives here — the page drives `refresh*` from its `activeInstance` effect and
// from the mod-install/uninstall/toggle event listeners — so there is no
// lifecycle to dispose.

import {
  commands,
  type InstanceWithStatus,
  type LoaderKind,
  type MissingModStatus,
  type PlaytimeStats,
} from '$lib/ipc/bindings';
import { isUnresolvedMissingState } from '$lib/modpacks/missing-mod';
import { ensureCompatScan } from '$lib/mods/compat-scan.svelte';
import { ensureLiveCompat, knownCompatHints } from '$lib/mods/installed/compat-check.svelte';
import { loadStoredUpdateCheck, pendingUpdateCount } from '$lib/mods/update-check-store.svelte';

export type InstalledStats = { total: number; enabled: number; disabled: number };

const EMPTY_INSTALLED: InstalledStats = { total: 0, enabled: 0, disabled: 0 };

// `last_session_unix_ms === null` is the canonical "never played" signal; the
// other fields can read null from the f64-via-specta quirk and are coerced to
// 0 with ?? at the read sites that need a number.
const EMPTY_PLAYTIME: PlaytimeStats = {
  total_seconds: 0,
  session_count: 0,
  last_session_seconds: 0,
  last_session_unix_ms: null,
};

export function createInstanceStats() {
  let installedStats = $state<InstalledStats>({ ...EMPTY_INSTALLED });
  // The profile whose listing `installedStats` holds: null until a read has landed, and after a
  // failed one — its zeros are then a reset, not an answer.
  let installedFor = $state<string | null>(null);
  let playtime = $state<PlaytimeStats>({ ...EMPTY_PLAYTIME });
  let packMissingMods = $state<MissingModStatus[]>([]);

  // Per-refresher monotonic request ids. Each `refresh*` awaits an IPC call; a
  // rapid instance switch can land mid-flight, so a stale run must not commit
  // the previous instance's data over the newer one. Each refresher bumps its
  // own counter and drops the commit if a newer call has started.
  //
  // The compat flags and the update count have no counter because they commit
  // nothing: they are read straight off their shared stores, which own their own
  // guards. A number copied out of a shared store is a second source of truth by
  // another name — #332 removed the duplicated *scan* and left the duplicated
  // *count*, so the Overview kept showing a value the store no longer held.
  let statsSeq = 0;
  let playtimeSeq = 0;
  let packSeq = 0;

  // The profile whose pending updates the Overview shows. The count is read off
  // the app-wide persisted check (update-check-store): a check the Installed tab
  // runs lands there, and the Overview follows at once (plan A18) — a copy would
  // wait for the next mod event. The store keys by profile, so a slow read for
  // the previous profile never lands on this one.
  let updateFor = $state<string | null>(null);

  // The platform triple the last `refreshIncompatible` ran for — the flags
  // getter needs it to look up live verdicts in the shared keyed store.
  let compatTriple = $state<{
    id: string | null;
    mc: string | null;
    loader: LoaderKind | null;
  } | null>(null);

  // Lightweight installed-mods stats for the Overview pane. Re-fetched on
  // instance change and whenever the launcher emits an install / uninstall /
  // toggle event from the mod browser.
  async function refreshInstalledStats(id: string | null) {
    if (!id) {
      installedStats = { ...EMPTY_INSTALLED };
      installedFor = null;
      return;
    }
    const seq = ++statsSeq;
    const r = await commands.modsListInstalled(id);
    if (seq !== statsSeq) return;
    // Reset on error rather than retaining — the labels are per-instance, so a
    // kept value silently attributes the previous instance's mods to this one.
    // Mirrors `refreshPlaytime`, which has always done this.
    if (r.status !== 'ok') {
      installedStats = { ...EMPTY_INSTALLED };
      installedFor = null;
      return;
    }
    const total = r.data.length;
    const enabled = r.data.filter((m) => m.enabled).length;
    installedStats = { total, enabled, disabled: total - enabled };
    installedFor = id;
  }

  // Make sure the app-wide compatibility scan is current for `id`. Commits
  // nothing of its own — the Overview's flags are read from the store (see the
  // `compatHints` getter below), so this is purely "go and refresh it".
  //
  // `force` is required after a mod install / uninstall / toggle: the store
  // keys on (instance, mc, loader), which such a change does not alter, so an
  // unforced call is deduplicated away and neither surface would ever notice a
  // newly-added wrong-loader jar.
  //
  // No vanilla short-circuit. Skipping the call left the PREVIOUS instance's
  // entries in the store, which is exactly the drift being fixed; and on a
  // Vanilla instance `compat_verdict` yields `loader_mismatch: false` for every
  // jar anyway, so 0 comes out by computation rather than by special case.
  async function refreshIncompatible(
    id: string | null,
    instances: InstanceWithStatus[],
    opts: { force?: boolean } = {},
  ) {
    const inst = id ? instances.find((i) => i.id === id) : null;
    // An empty `mc_version` is "not configured yet", not a version to scan for.
    const triple = {
      id: inst?.id ?? null,
      mc: inst?.mc_version || null,
      loader: inst?.loader ?? null,
    };
    compatTriple = triple;
    await ensureCompatScan(triple.id, triple.mc, triple.loader, opts);
    // Fire-and-forget: the Overview must not BLOCK on the network (spec D4).
    // The flags getter is a pure store read; it updates reactively when the
    // verdicts land. Offline this decides everything `unknown` — no flags,
    // no retry hammer.
    void ensureLiveCompat(triple.id, triple.mc, triple.loader);
  }

  // Per-instance playtime stats — refreshed on instance switch and after every
  // game exit (via the page's processExited handler).
  async function refreshPlaytime(id: string | null) {
    if (!id) {
      playtime = { ...EMPTY_PLAYTIME };
      return;
    }
    const seq = ++playtimeSeq;
    const r = await commands.getPlaytime(id);
    if (seq !== playtimeSeq) return;
    // Reset to EMPTY on error rather than retaining the previous instance's
    // playtime — a stale value here would mislabel a fresh instance's Overview.
    playtime = r.status === 'ok' ? r.data : { ...EMPTY_PLAYTIME };
  }

  // Missing mods for the active pack-origin instance — drives the Overview
  // indicator. Empty for non-pack instances and pre-SF2 imports (modpack_status
  // returns null or an empty list).
  async function refreshPackStatus(id: string | null) {
    if (!id) {
      packMissingMods = [];
      return;
    }
    const seq = ++packSeq;
    const r = await commands.modpackStatus(id);
    if (seq !== packSeq) return;
    // NOTE: an errored call and a genuinely non-pack instance both collapse to
    // `[]` here, i.e. "the pack is complete". That is a `Confidence` problem
    // (spec finding 43) and is deliberately left for the PR that introduces the
    // type — it cannot be fixed by clearing, only by being able to say
    // "couldn't check".
    packMissingMods = r.status === 'ok' && r.data ? r.data.missing_mods : [];
  }

  // Re-read the persisted update check of `id` — on an instance switch and
  // after the mod set changes (an update replaces a jar, the check then lists
  // it no more).
  async function refreshUpdateCount(id: string | null) {
    updateFor = id;
    if (id) await loadStoredUpdateCheck(id);
  }

  return {
    /** Profile `id`'s installed-mod counts — null while they are not known: no read has landed
     *  yet, the counts held are another profile's (a switch still reading), or the read failed.
     *  The held zeros are a reset, not an answer, and the Overview said «no mods» on them. There
     *  is no unchecked getter: every reader asks for a profile. */
    installedStatsFor(id: string | null): InstalledStats | null {
      return id !== null && installedFor === id ? installedStats : null;
    },
    /** Whether profile `id` has installed mods (the Add-ons tab's first view, spec D10) — null
     *  while that is not known: no read has landed yet, the count held is another profile's
     *  (a switch still reading), or the read failed. No profile has none. A call site reading it
     *  in a template stays reactive: it reads `$state` at call time. */
    hasInstalledMods(id: string | null): boolean | null {
      if (id === null) return false;
      return installedFor === id ? installedStats.total > 0 : null;
    },
    // Every mod compat flags, with the reason its Installed row reads — read
    // straight off the SHARED stores at call time (offline scan + keyed live
    // verdicts), so it can never be a stale copy (locked C6). The Overview
    // decides each mod's level from these and the page pre-flight with the
    // rows' own `statusOf` (`problemCounts`), so it and the Installed chip
    // cannot disagree. Still a pure read: the network work happens in
    // `refreshIncompatible`'s fire-and-forget ensure, never here.
    get compatHints() {
      return knownCompatHints(
        compatTriple?.id ?? null,
        compatTriple?.mc ?? null,
        compatTriple?.loader ?? null,
      );
    },
    // Pending updates in the persisted check (spec §5.5). null = never checked,
    // or the stored check could not be read — "not known", never a reassuring
    // 0 (spec §9).
    get updateCount() {
      return pendingUpdateCount(updateFor);
    },
    get playtime() {
      return playtime;
    },
    get packMissingMods() {
      return packMissingMods;
    },
    // Computed getter (reads `$state` at call time → reactive in templates).
    get unresolvedMissing() {
      return packMissingMods.filter((m) => isUnresolvedMissingState(m.state));
    },
    refreshInstalledStats,
    refreshIncompatible,
    refreshPlaytime,
    refreshPackStatus,
    refreshUpdateCount,
  };
}

export type InstanceStats = ReturnType<typeof createInstanceStats>;
