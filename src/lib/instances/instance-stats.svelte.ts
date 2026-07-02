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
  type MissingModStatus,
  type PlaytimeStats,
} from '$lib/ipc/bindings';
import { isUnresolvedMissingState } from '$lib/modpacks/missing-mod';

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
  let incompatibleCount = $state(0);
  let playtime = $state<PlaytimeStats>({ ...EMPTY_PLAYTIME });
  let packMissingMods = $state<MissingModStatus[]>([]);

  // Per-refresher monotonic request ids. Each `refresh*` awaits an IPC call; a
  // rapid instance switch can land mid-flight, so a stale run must not commit
  // the previous instance's data over the newer one. Each refresher bumps its
  // own counter and drops the commit if a newer call has started.
  let statsSeq = 0;
  let incompatSeq = 0;
  let playtimeSeq = 0;
  let packSeq = 0;

  // Lightweight installed-mods stats for the Overview pane. Re-fetched on
  // instance change and whenever the launcher emits an install / uninstall /
  // toggle event from the mod browser.
  async function refreshInstalledStats(id: string | null) {
    if (!id) {
      installedStats = { ...EMPTY_INSTALLED };
      return;
    }
    const seq = ++statsSeq;
    const r = await commands.modsListInstalled(id);
    if (seq !== statsSeq) return;
    if (r.status !== 'ok') return;
    const total = r.data.length;
    const enabled = r.data.filter((m) => m.enabled).length;
    installedStats = { total, enabled, disabled: total - enabled };
  }

  // Offline incompatible-mod count for the Overview indicator (network-free).
  // Counts ONLY manual jars whose loader family mismatches (`!live_checkable`)
  // — those are the definitive offline verdicts. Platform suspects need the
  // live auto-confirm the Installed tab performs (the Overview makes no network
  // call), so counting their raw offline suspicion here would re-introduce
  // false positives. Empty for vanilla / version-less instances.
  async function refreshIncompatible(id: string | null, instances: InstanceWithStatus[]) {
    const inst = id ? instances.find((i) => i.id === id) : null;
    if (!inst?.mc_version || inst.loader === 'vanilla') {
      incompatibleCount = 0;
      return;
    }
    const seq = ++incompatSeq;
    const r = await commands.scanInstanceModCompat(inst.id, inst.mc_version, inst.loader);
    if (seq !== incompatSeq) return;
    incompatibleCount =
      r.status === 'ok' ? r.data.filter((x) => x.loader_mismatch && !x.live_checkable).length : 0;
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
    packMissingMods = r.status === 'ok' && r.data ? r.data.missing_mods : [];
  }

  return {
    get installedStats() {
      return installedStats;
    },
    get incompatibleCount() {
      return incompatibleCount;
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
  };
}

export type InstanceStats = ReturnType<typeof createInstanceStats>;
