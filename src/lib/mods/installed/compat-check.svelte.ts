import { commands, type LoaderKind } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import {
  compatScanEntries,
  ensureCompatScan,
  isOfflineMismatch,
  offlineMismatchCount,
} from '$lib/mods/compat-scan.svelte';

// Tooltip hint descriptor. The component maps these to i18n strings (it owns the
// instance loader/mc for interpolation).
export type CompatHint =
  | { key: 'loader'; detected: string }
  | { key: 'noRelease' }
  | { key: 'platformMc'; declared: string }
  | { key: 'platformLoader'; declared: string };

type LiveVerdict = 'compatible' | 'incompatible' | 'unknown';

// Live verdicts, held once for the whole app and keyed by the FULL platform
// triple each was computed against — `(instanceId, mc, loader, sha1)`.
//
// Module-level so a verdict survives the Installed tab unmounting (observed
// live 2026-08-11: Обзор → back wiped the manual check's result and the chip
// read «всё в порядке»). Triple-keyed so a verdict can never outlive the
// platform it answered for: an entry written under mc 1.21 simply does not
// exist for the same sha under 1.20.1 (the stale-carryover half of audit #9 —
// four perfect 1.20.1 builds stayed flagged with 1.21 verdicts), and a
// hardlink-shared sha on another instance never inflates this one's chip.
// Invalidation is by key construction — there is no imperative clear to
// forget, and none on a momentary null instance id.
let liveVerdicts = $state<Map<string, LiveVerdict>>(new Map());

// `|` cannot occur in an instance id (a directory name), version string, loader kind or sha —
// an unambiguous join.
function verdictKey(id: string, mc: string, loader: LoaderKind, sha1: string): string {
  return `${id}|${mc}|${loader}|${sha1}`;
}

// Monotonic run id. Every scan / live-check captures the value at entry and
// bumps it; any later run supersedes earlier ones. Because the live-confirm
// loop awaits up to 25 sequential network calls (seconds), a superseding run
// can start mid-loop — without this guard the stale run would keep writing.
// MODULE-level on purpose, like the store it protects: a composable disposed
// by a remount leaves its loop running (dispose() cannot cancel an await), and
// with a per-composable counter that orphan's late writes would pass its own
// guard and overwrite verdicts the successor just paid for (review finding on
// this PR: a late `unknown` could silently unflag a real incompatible).
// Sharing the counter makes ANY newer run — same composable or its
// replacement — supersede the orphan.
let generation = 0;

/** Test-only: the store is deliberately module-level (it must survive
 * remounts), so unit tests reset it between cases. */
export function __resetLiveVerdictsForTests(): void {
  liveVerdicts = new Map();
  liveInFlight = null;
  generation++;
}

// One in-flight full check per platform triple: both surfaces (the Overview's
// refresh and the Installed tab's pipeline) ensure on the same triggers, and
// joining the running command beats issuing a duplicate N-mod query burst.
let liveInFlight: { key: string; promise: Promise<void> } | null = null;

/**
 * Ensure every platform mod of the current offline scan has a live verdict
 * under this exact (instance, mc, loader) — the same full check the manual
 * «Проверить совместимость» button runs (concurrency-bounded and version-
 * cache-backed in the backend), executed at most once per triple: once every
 * checkable mod is decided this is a pure store read. This is what makes
 * live-only incompatibles (permissive file range, but the project publishes
 * no build for the platform) surface on tab open and on the Overview instead
 * of only after the button (maintainer request, 2026-08-12).
 *
 * Fire-and-forget safe WITHOUT a generation token: results are written under
 * the triple they were queried FOR, so a call that outlives an instance
 * switch lands its verdicts under the OLD triple, invisible to the new one.
 *
 * Offline behaviour: the backend maps per-mod query failures to `unknown`,
 * which never flags AND marks the triple decided — no retry hammer while the
 * network is down; the manual button (force) re-decides once it is back.
 */
export async function ensureLiveCompat(
  id: string | null,
  mc: string | null,
  loader: LoaderKind | null,
): Promise<void> {
  if (!id || !loader || mc == null) return;
  const undecided = compatScanEntries().some(
    (lc) => lc.live_checkable && !liveVerdicts.has(verdictKey(id, mc, loader, lc.sha1)),
  );
  if (!undecided) return;
  const key = `${id}|${mc}|${loader}`;
  if (liveInFlight?.key === key) return liveInFlight.promise;
  const run = (async () => {
    const r = await commands.checkInstanceModCompat(id, mc, loader);
    // Command-level failure: stay undecided so the next ensure retries.
    if (r.status !== 'ok') return;
    const next = new Map(liveVerdicts);
    for (const c of r.data)
      next.set(verdictKey(id, mc, loader, c.sha1), c.status.status as LiveVerdict);
    liveVerdicts = next;
  })();
  liveInFlight = { key, promise: run };
  try {
    await run;
  } finally {
    // Only clear our own entry — a newer ensure may have replaced it.
    if (liveInFlight?.promise === run) liveInFlight = null;
  }
}

/**
 * The union count the headline surfaces show: offline-decidable mismatches
 * plus live «no build for this platform» verdicts already in the store. A
 * PURE READ — the Overview renders it instantly and never blocks on the
 * network; pair with a fire-and-forget `ensureLiveCompat` to fill the live
 * half (the count is reactive, so the row updates when verdicts land).
 * Falls back to the plain offline count while the triple is unknown.
 */
export function knownIncompatibleCount(
  id: string | null,
  mc: string | null,
  loader: LoaderKind | null,
): number {
  if (!id || !loader || mc == null) return offlineMismatchCount();
  let n = 0;
  for (const lc of compatScanEntries()) {
    if (
      isOfflineMismatch(lc) ||
      liveVerdicts.get(verdictKey(id, mc, loader, lc.sha1)) === 'incompatible'
    )
      n++;
  }
  return n;
}

// Owns proactive compatibility state as a two-stage AUTO pipeline:
//   1. offline scan (network-free) flags loader-family SUSPECTS and reads the
//      platform-range verdict (`platform_mismatch`) straight off the jar;
//   2. `ensureLiveCompat` — the SAME full platform check the manual button
//      runs, once per (instance, mc, loader): live-only incompatibles show up
//      on tab open, and the button can never reveal mods the auto pass did
//      not already know about.
// A mod is flagged iff `platform_mismatch` is set (unconditional — read off the
// file that will actually be launched, never cleared by a live verdict), OR the
// live verdict is `incompatible`, OR it is a manual loader-family suspect
// (loader_mismatch && !live_checkable) with no compatible live verdict. Live
// `compatible` clears a loader-family suspicion but never a `platform_mismatch`
// — the live check answers "does the PROJECT have a build for this MC+loader",
// not "is the FILE on disk that build". Live failure/`unknown` never flags
// (transient errors must not read as incompatible). Verdicts live in the
// module-level store per (instance, mc, loader, sha) — surviving remounts,
// expiring with the platform.
export function createCompatCheck(
  getInstanceId: () => string | null,
  getMcVersion: () => string | null,
  getLoader: () => LoaderKind | null,
) {
  // The offline half is NOT owned here — it is the app-wide scan, shared with
  // the Overview indicator. Keeping a private copy is what let the two
  // surfaces disagree about the same instance. The live half lives in the
  // module-level `liveVerdicts` (see above) for the same reason, plus a
  // lifecycle one: verdicts must survive this composable's unmount and die
  // with the platform triple they were computed for.
  const offline = $derived(new Map(compatScanEntries().map((x) => [x.sha1, x])));
  let checking = $state(false);
  let error = $state<string | null>(null);

  const incompatibleShas = $derived.by(() => {
    const out = new Set<string>();
    const id = getInstanceId();
    const mc = getMcVersion();
    const loader = getLoader();
    // Iterating the SCAN (not the live store) means only mods still installed
    // can count, and looking live verdicts up by the full platform key means a
    // verdict computed for another instance or a previous mc/loader simply
    // does not exist here.
    for (const [sha, lc] of offline) {
      // Authoritative and unconditional — deliberately NOT cleared by a live
      // `compatible`, which answers about the PROJECT rather than the FILE.
      // That conflation is the defect this whole feature exists to fix: every
      // one of the six projects in the original bug published a build for the
      // target MC, so the live check said "compatible" while the file on disk
      // was still wrong.
      if (lc.platform_mismatch) out.add(sha);
      // Manual suspect: offline verdict stands (no platform identity to verify).
      if (lc.loader_mismatch && !lc.live_checkable) out.add(sha);
      if (
        id &&
        loader &&
        mc != null &&
        liveVerdicts.get(verdictKey(id, mc, loader, sha)) === 'incompatible'
      )
        out.add(sha);
    }
    return out;
  });
  const incompatibleCount = $derived(incompatibleShas.size);

  function hintFor(sha1: string): CompatHint | null {
    // Platform-range verdict is read off the jar itself — no live confirmation
    // needed, and it is the more specific explanation, so it takes priority
    // over both the loader-family suspect and any live verdict.
    const lc = offline.get(sha1);
    if (lc?.platform_mismatch) {
      const declared = lc.platform_declared ?? '?';
      return lc.platform_axis === 'minecraft'
        ? { key: 'platformMc', declared }
        : { key: 'platformLoader', declared };
    }
    const id = getInstanceId();
    const mc = getMcVersion();
    const loader = getLoader();
    if (
      id &&
      loader &&
      mc != null &&
      liveVerdicts.get(verdictKey(id, mc, loader, sha1)) === 'incompatible'
    )
      return { key: 'noRelease' };
    if (lc?.loader_mismatch && !lc.live_checkable)
      return { key: 'loader', detected: lc.detected_loader ?? '?' };
    return null;
  }

  // Stage 1: offline scan (shared store), then the once-per-triple full
  // live check. "Already decided" is judged per platform triple — a verdict
  // from the previous mc/loader never pins a mod (audit #10: the old
  // sha-keyed check let a suspect stay "compatible" across the exact version
  // change the feature protects against).
  async function runOfflineScan(opts: { force?: boolean } = {}) {
    const gen = ++generation;
    const id = getInstanceId();
    const loader = getLoader();
    const mc = getMcVersion();
    if (!id || !loader || mc == null) {
      // No live-store wipe here: verdicts are keyed by platform triple, so a
      // momentary null id (remount flush) cannot make them count for anything
      // — clearing them only destroyed paid-for answers (observed live:
      // Обзор → back read «всё в порядке» after a manual check found 4).
      await ensureCompatScan(id, mc, loader);
      return;
    }
    await ensureCompatScan(id, mc, loader, opts);
    if (gen !== generation) return; // superseded mid-scan
    await ensureLiveCompat(id, mc, loader);
  }

  // Manual "Check compatibility" button — FULL re-check: the offline scan is
  // re-run first, then every platform mod is queried.
  //
  // It used to write only `live`. Both halves of `incompatibleShas` read the
  // offline scan, so a loader-family mismatch could never be surfaced by this
  // button — pressing it on an affected instance answered "nothing found",
  // which is worse than not offering the button at all.
  async function runLiveCheck() {
    const id = getInstanceId();
    const loader = getLoader();
    const mc = getMcVersion();
    if (!id || !loader || mc == null) return;
    checking = true;
    error = null;
    await ensureCompatScan(id, mc, loader, { force: true });
    const gen = ++generation;
    const r = await commands.checkInstanceModCompat(id, mc, loader);
    checking = false;
    if (gen !== generation) return; // superseded (e.g. instance switched mid-check)
    if (r.status === 'error') {
      error = formatError(r.error);
      return;
    }
    const next = new Map(liveVerdicts);
    for (const c of r.data)
      next.set(verdictKey(id, mc, loader, c.sha1), c.status.status as LiveVerdict);
    liveVerdicts = next;
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
