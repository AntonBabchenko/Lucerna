import { commands, type LoaderKind, type ModLocalCompat } from '$lib/ipc/bindings';

/**
 * The instance mod-compatibility scan, held once for the whole app.
 *
 * Two surfaces consume it — the Overview's "incompatible mods" indicator and
 * the Installed tab's «Несовместимые» chip — and they used to each keep a
 * private copy of the same `scan_instance_mod_compat` result, refreshed by
 * different triggers. Two caches of one command with no shared invalidation
 * drift apart, and on a real instance they did: the Overview reported two
 * loader-mismatched jars while the Installed tab showed none.
 *
 * The surfaces still derive DIFFERENT views from this one scan, deliberately:
 * the Overview counts only offline-decidable mismatches because it must not
 * make a network call, while the Installed tab additionally folds in the live
 * platform verdicts it fetches. Sharing the data fixes the divergence; sharing
 * the count would either put network work behind the Overview or re-introduce
 * the false positives the `live_checkable` guard exists to prevent.
 */

type ScanKey = { instanceId: string; mcVersion: string; loader: LoaderKind };

let key = $state<ScanKey | null>(null);
let entries = $state<ModLocalCompat[]>([]);

// Monotonic run id: a scan captures it at entry, and any later run supersedes
// it. Without this an instance switch landing mid-scan would write the previous
// instance's verdicts into the new instance's state.
let generation = 0;

// The scan currently running, if any. Both surfaces refresh on an instance
// switch, and the first caller clears `key` synchronously — which makes the
// second caller's `sameKey` check false, so it issues its own command. On a
// 140-mod instance that is every jar read and unzipped twice per switch. A
// later caller for the same key joins this promise instead. Only reusable
// while `gen` is still current: once a newer run supersedes it, its write is
// dropped by the generation guard, so joining it would return nothing.
let inFlight: { key: ScanKey; gen: number; promise: Promise<void> } | null = null;

function sameKey(a: ScanKey | null, b: ScanKey): boolean {
  return (
    a !== null &&
    a.instanceId === b.instanceId &&
    a.mcVersion === b.mcVersion &&
    a.loader === b.loader
  );
}

/** The current scan's entries, or `[]` when nothing has been scanned yet. */
export function compatScanEntries(): ModLocalCompat[] {
  return entries;
}

/**
 * Whether a single scanned mod will not load, on either offline-decidable
 * axis — the file-level verdict that both `offlineMismatchCount` (the
 * Overview) and `compatSummaryFromScan` (the post-version-change summary in
 * the Manage modal) report on. Exported so both call the same predicate
 * instead of each re-deriving it: two answers to this one question is
 * exactly the defect this shared scan exists to eliminate.
 *
 * - `platform_mismatch` is read straight off the jar's declared MC/loader
 *   range against what the instance provides. It needs no `live_checkable`
 *   guard and no live confirmation — the range violation is a property of
 *   the file that will actually be launched, not a suspicion about the
 *   project. No platform answer can overrule it.
 * - `loader_mismatch` is offline-authoritative too (spec D5). The false
 *   positives the old `!live_checkable` guard existed for are excluded INSIDE
 *   `scan_instance`: a family-inclusive (multi-loader) jar never flags, and
 *   `connector_installed` clears Fabric-on-Forge under Sinytra Connector. The
 *   project-level live «compatible» must not clear a foreign-family FILE —
 *   that clearing is exactly what kept a Forge→Fabric switch silent while
 *   FabricLoader skipped ten dead jars.
 */
export function isOfflineMismatch(entry: ModLocalCompat): boolean {
  return entry.platform_mismatch || entry.loader_mismatch;
}

/**
 * Mods that will not load, on either offline-decidable axis. This is the
 * Overview's count — every jar counted here can be decided from the file
 * alone, with no network call. See `isOfflineMismatch` for the predicate.
 */
export function offlineMismatchCount(): number {
  return entries.filter(isOfflineMismatch).length;
}

/**
 * Scan unless the current state already answers for this exact
 * (instance, mc, loader). `force` re-runs it regardless.
 *
 * `force` is REQUIRED whenever the mod set changed — an install, uninstall or
 * enable/disable does not alter the key, so an unforced call is deduplicated
 * away and no surface would ever notice the new jar. The manual "Check
 * compatibility" button needs it for the same reason.
 *
 * A failed REFRESH of the instance already on screen leaves the previous result
 * in place rather than blanking it: an empty list reads as "nothing is wrong",
 * which a transient error must never be allowed to say. A scan for a DIFFERENT
 * instance clears first — showing another instance's verdicts, even briefly, is
 * worse than showing none.
 */
export async function ensureCompatScan(
  instanceId: string | null,
  mcVersion: string | null,
  loader: LoaderKind | null,
  opts: { force?: boolean } = {},
): Promise<void> {
  if (!instanceId || !loader || mcVersion == null) {
    generation++;
    inFlight = null;
    key = null;
    entries = [];
    return;
  }
  const next: ScanKey = { instanceId, mcVersion, loader };
  const isRefresh = sameKey(key, next);
  if (!opts.force && isRefresh) return;
  if (!opts.force && inFlight && inFlight.gen === generation && sameKey(inFlight.key, next)) {
    await inFlight.promise;
    return;
  }

  const gen = ++generation;
  if (!isRefresh) {
    // Different instance/version/loader: drop the old verdicts synchronously so
    // nothing renders the previous instance's chip while this scan is in flight.
    key = null;
    entries = [];
  }
  const run = (async () => {
    const r = await commands.scanInstanceModCompat(instanceId);
    if (gen !== generation) return; // superseded by a newer scan
    if (r.status !== 'ok') return; // keep whatever we had
    key = next;
    entries = r.data;
  })();
  inFlight = { key: next, gen, promise: run };
  try {
    await run;
  } finally {
    // Only clear our own entry — a newer run may have replaced it while we were
    // awaiting, and dropping that one would let the next caller start a third.
    if (inFlight?.promise === run) inFlight = null;
  }
}

/** Drop the cached scan so the next `ensureCompatScan` refetches. */
export function invalidateCompatScan(): void {
  generation++;
  inFlight = null;
  key = null;
  entries = [];
}
