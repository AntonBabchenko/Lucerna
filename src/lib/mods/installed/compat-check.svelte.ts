import { commands, type LoaderKind, type ModLocalCompat, type ModSource } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

// Tooltip hint descriptor. The component maps these to i18n strings (it owns the
// instance loader/mc for interpolation).
export type CompatHint = { key: 'loader'; detected: string } | { key: 'noRelease' };

type LiveVerdict = 'compatible' | 'incompatible' | 'unknown';
// Minimal row shape the composable needs to live-query a platform suspect.
type CompatRow = {
  installed: { sha1: string; source: ModSource | null; project_id: string | null };
};

// Owns proactive compatibility state as a two-stage AUTO pipeline:
//   1. offline scan (network-free) flags loader-family SUSPECTS;
//   2. for platform suspects (live_checkable), auto-query the platform to confirm.
// A mod is flagged iff the live verdict is `incompatible`, OR it is a manual
// suspect (loader_mismatch && !live_checkable) with no compatible live verdict.
// Live `compatible` clears; live failure/`unknown` never flags (transient errors
// must not read as incompatible). Auto-confirm is bounded to suspects, sequential
// (gentle on Modrinth rate limits), and cached per session by sha.
export function createCompatCheck(
  getInstanceId: () => string | null,
  getMcVersion: () => string | null,
  getLoader: () => LoaderKind | null,
  getRows: () => CompatRow[],
) {
  let offline = $state<Map<string, ModLocalCompat>>(new Map());
  let live = $state<Map<string, LiveVerdict>>(new Map());
  let checking = $state(false);
  let error = $state<string | null>(null);

  // Monotonic run id. Every scan / live-check captures the value at entry and
  // bumps it; any later run supersedes earlier ones. Because the live-confirm
  // loop awaits up to 25 sequential network calls (seconds), an instance switch
  // can land mid-loop — without this guard a stale run would write a verdict
  // computed against the previous instance's loader/mc into the new instance's
  // `live` map (wrong chip + inflated `Incompatible (N)` count). Mirrors the
  // project's generation-token composable pattern.
  let generation = 0;

  const incompatibleShas = $derived.by(() => {
    const out = new Set<string>();
    // Only count live verdicts for mods still present in the current scan — a
    // stale `live` entry for a since-uninstalled mod must not inflate the count.
    for (const [sha, v] of live) if (v === 'incompatible' && offline.has(sha)) out.add(sha);
    for (const [sha, lc] of offline) {
      // Manual suspect: offline verdict stands (no platform identity to verify).
      if (lc.loader_mismatch && !lc.live_checkable) out.add(sha);
    }
    return out;
  });
  const incompatibleCount = $derived(incompatibleShas.size);

  function hintFor(sha1: string): CompatHint | null {
    if (live.get(sha1) === 'incompatible') return { key: 'noRelease' };
    const lc = offline.get(sha1);
    if (lc?.loader_mismatch && !lc.live_checkable)
      return { key: 'loader', detected: lc.detected_loader ?? '?' };
    return null;
  }

  // Cap on how many platform suspects we auto-query in one pass. The offline
  // pre-filter already makes suspects rare (a multi-loader jar that includes the
  // instance's family is not a suspect), so this only guards a pathological
  // instance from firing a burst of Modrinth requests on tab open (we have hit
  // Modrinth's 429 rate limit before). Suspects beyond the cap simply stay
  // unconfirmed → unflagged (safe; the manual "Check compatibility" button does
  // the full pass on demand).
  const AUTO_CONFIRM_CAP = 25;

  // Stage 2: auto-confirm platform suspects via the platform. Sequential (gentle
  // on the rate limit), capped, and skips shas already decided this session.
  // `gen` is its parent run's id; the loop bails the moment a newer run starts.
  async function autoConfirmSuspects(gen: number) {
    const id = getInstanceId();
    const loader = getLoader();
    const mc = getMcVersion();
    if (!id || !loader || mc == null) return;
    const rows = getRows();
    const suspects = [...offline.values()]
      .filter((lc) => lc.loader_mismatch && lc.live_checkable && !live.has(lc.sha1))
      .slice(0, AUTO_CONFIRM_CAP);
    for (const s of suspects) {
      const row = rows.find((r) => r.installed.sha1 === s.sha1);
      if (!row?.installed.source || !row?.installed.project_id) continue;
      const r = await commands.modsVersions(
        row.installed.source,
        row.installed.project_id,
        mc,
        loader,
      );
      if (gen !== generation) return; // superseded by a newer scan — drop the write
      const verdict: LiveVerdict =
        r.status === 'error' ? 'unknown' : r.data.length > 0 ? 'compatible' : 'incompatible';
      const next = new Map(live);
      next.set(s.sha1, verdict);
      live = next;
    }
  }

  // Stage 1: offline scan, then auto-confirm.
  async function runOfflineScan() {
    const gen = ++generation;
    const id = getInstanceId();
    const loader = getLoader();
    const mc = getMcVersion();
    if (!id || !loader || mc == null) {
      if (gen === generation) {
        offline = new Map();
        live = new Map();
      }
      return;
    }
    const r = await commands.scanInstanceModCompat(id, mc, loader);
    if (gen !== generation) return; // superseded mid-scan
    offline = r.status === 'ok' ? new Map(r.data.map((x) => [x.sha1, x])) : new Map();
    await autoConfirmSuspects(gen);
  }

  // Manual "Check compatibility" button — FULL re-check of every platform mod
  // (also catches MC-only incompatibility the suspect pre-filter skips).
  async function runLiveCheck() {
    const gen = ++generation;
    const id = getInstanceId();
    const loader = getLoader();
    const mc = getMcVersion();
    if (!id || !loader || mc == null) return;
    checking = true;
    error = null;
    const r = await commands.checkInstanceModCompat(id, mc, loader);
    checking = false;
    if (gen !== generation) return; // superseded (e.g. instance switched mid-check)
    if (r.status === 'error') {
      error = formatError(r.error);
      return;
    }
    const next = new Map(live);
    for (const c of r.data) next.set(c.sha1, c.status.status as LiveVerdict);
    live = next;
  }

  // NOTE: the pipeline is driven by the consuming component (InstalledModsView),
  // which calls `runOfflineScan()` from a `$effect` on instance/mc/loader change
  // and from the mod add/remove/toggle event handlers. The composable does NOT
  // own a self-effect: a self-effect here would also fire under @testing-library
  // AND flush during the unit tests' awaits, racing the tests' direct
  // `runOfflineScan()` calls against the generation guard. Keeping the trigger in
  // the component gives one well-defined caller and keeps this factory a pure,
  // directly-testable state machine.

  return {
    get incompatibleShas() {
      return incompatibleShas;
    },
    get incompatibleCount() {
      return incompatibleCount;
    },
    get checking() {
      return checking;
    },
    get error() {
      return error;
    },
    hintFor,
    runOfflineScan,
    runLiveCheck,
    // No internal effect to tear down; kept for API symmetry with the other
    // installed-tab composables (InstalledModsView calls dispose() on unmount).
    dispose() {},
  };
}
