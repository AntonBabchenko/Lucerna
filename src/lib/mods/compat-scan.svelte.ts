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
 * Mods whose loader family mismatches the instance AND that cannot be checked
 * against a platform — the offline-decidable verdicts. This is the Overview's
 * count: a platform suspect is excluded because confirming it needs a network
 * call the Overview does not make, and counting the raw suspicion would flag
 * multi-loader jars that are perfectly fine.
 */
export function offlineMismatchCount(): number {
  return entries.filter((e) => e.loader_mismatch && !e.live_checkable).length;
}

/**
 * Scan unless the current state already answers for this exact
 * (instance, mc, loader). `force` re-runs it regardless — used by the manual
 * "Check compatibility" button, which must not report a stale verdict.
 *
 * A failed scan leaves the previous result in place rather than blanking it: an
 * empty list reads as "nothing is wrong", which a transient error must never
 * be allowed to say.
 */
export async function ensureCompatScan(
  instanceId: string | null,
  mcVersion: string | null,
  loader: LoaderKind | null,
  opts: { force?: boolean } = {},
): Promise<void> {
  if (!instanceId || !loader || mcVersion == null) {
    generation++;
    key = null;
    entries = [];
    return;
  }
  const next: ScanKey = { instanceId, mcVersion, loader };
  if (!opts.force && sameKey(key, next)) return;

  const gen = ++generation;
  const r = await commands.scanInstanceModCompat(instanceId, mcVersion, loader);
  if (gen !== generation) return; // superseded by a newer scan
  if (r.status !== 'ok') return; // keep the last good result
  key = next;
  entries = r.data;
}

/** Drop the cached scan so the next `ensureCompatScan` refetches. */
export function invalidateCompatScan(): void {
  generation++;
  key = null;
  entries = [];
}
