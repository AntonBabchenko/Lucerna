import { untrack } from 'svelte';
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import {
  commands,
  type InstalledMod,
  type ModSource,
  type ModUpdateCheck,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { createInFlight } from '$lib/mods/in-flight';
import { type ModOpScope, modWriteReason } from '$lib/mods/mod-ops.svelte';
import {
  checkForUpdates,
  dropUpdateRows,
  isCheckingUpdates,
  loadStoredUpdateCheck,
  storedUpdateCheck,
} from '$lib/mods/update-check-store.svelte';
import { pushWarning } from '$lib/toasts/toasts.svelte';
import {
  depsOf,
  pushUpdatesReport,
  runUpdates,
  type UpdateAttempt,
  type UpdateTarget,
  updatedShas,
} from './update-review';

/** A hold is per project, never per jar (spec §5.5): it survives updates and restores. */
export const holdKey = (source: ModSource, projectId: string): string => `${source}:${projectId}`;

/** Hold writes under way, per (profile, project) — module-wide, as the writes are. */
const holdWrites = createInFlight();

// The Installed tab's side of mod updates. The check itself — one row per mod (keyed by the
// installed sha1) and when it ran — is the app-wide persisted check (`update-check-store`), the
// copy the Overview reads too (plan A18). This adds what only the tab needs: which projects are
// held, the CurseForge key banner, and running the updates the user picked.
export function createUpdateCheck(
  getInstanceId: () => string | null,
  refresh: () => Promise<void>,
  // What an update run captures (profile name, row names); the view supplies it.
  scopeFor: (instanceId: string) => ModOpScope = (instanceId) => ({ instanceId }),
) {
  const stored = $derived(storedUpdateCheck(getInstanceId()));
  const updateChecks = $derived(
    new Map<string, ModUpdateCheck>((stored?.results ?? []).map((c) => [c.sha1, c])),
  );
  const checkedAtMs = $derived(stored?.checkedAtMs ?? null);
  // Held projects by holdKey. `null` = not read, or the read failed: hold controls are then hidden
  // rather than guessing "not held" (fallback Q2 — "could not tell" is not "absent").
  let holds = $state<Set<string> | null>(null);
  let showCfBanner = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let holdsSeq = 0;

  const pending = $derived(
    [...updateChecks.values()].filter((c) => c.state.kind === 'update_available'),
  );
  const updateCount = $derived(pending.length);
  const updatableShas = $derived(new Set(pending.map((c) => c.sha1)));

  async function updateCfBanner(id: string, results: readonly ModUpdateCheck[]) {
    if (!results.some((c) => c.source === 'curseforge' && c.state.kind === 'check_failed')) {
      showCfBanner = false;
      return;
    }
    let key: Awaited<ReturnType<typeof commands.modsGetCurseforgeKeyStatus>> | null = null;
    try {
      key = await commands.modsGetCurseforgeKeyStatus();
    } catch {
      // The key status could not be read: no banner claims the key is missing.
    }
    if (getInstanceId() !== id) return;
    // 'unknown' = the keyring could not be read and no built-in key serves.
    showCfBanner = key?.status === 'ok' && (key.data === 'missing' || key.data === 'unknown');
  }

  /** Read the persisted check of `id` back (the store's rules decide what it may replace). */
  async function loadStored(id: string): Promise<void> {
    await loadStoredUpdateCheck(id);
    if (getInstanceId() !== id) return;
    await updateCfBanner(id, storedUpdateCheck(id)?.results ?? []);
  }

  async function loadHolds(id: string): Promise<void> {
    const seq = ++holdsSeq;
    let r: Awaited<ReturnType<typeof commands.modsListHolds>> | null = null;
    try {
      r = await commands.modsListHolds(id);
    } catch {
      // Transport failure: unknown, handled exactly like a command error below.
    }
    if (seq !== holdsSeq || getInstanceId() !== id) return;
    holds = r?.status === 'ok' ? new Set(r.data.map((h) => holdKey(h.source, h.project_id))) : null;
  }

  // Seed on every profile change. $effect.root so the composable is unit-testable; dispose()
  // tears it down. The effect makes its first run at the caller's first await, under vitest too.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        const id = getInstanceId();
        untrack(() => {
          holds = null;
          showCfBanner = false;
          error = null;
          if (id) {
            void loadStored(id);
            void loadHolds(id);
          }
        });
      });
    });
  } catch {
    /* no reactive runtime to root the effect in — it stays inert; a caller seeds by hand */
  }

  async function checkUpdates() {
    const id = getInstanceId();
    if (!id) return;
    error = null;
    let r: Awaited<ReturnType<typeof checkForUpdates>>;
    try {
      r = await checkForUpdates(id);
    } catch (e) {
      // The bridge failed: nothing was checked.
      if (getInstanceId() === id) error = e instanceof Error ? e.message : String(e);
      return;
    }
    // Switched away meanwhile: the answer is that profile's, and waits for its next visit.
    if (getInstanceId() !== id) return;
    if (r.status === 'error') {
      error = formatError(r.error);
      showCfBanner = false;
      return;
    }
    await updateCfBanner(id, r.data);
  }

  /** Drop the rows of jars an update replaced — in the profile it ran for. */
  function forget(sha1s: Iterable<string>, instanceId: string | null = getInstanceId()) {
    const gone = new Set(sha1s);
    if (!instanceId || gone.size === 0) return;
    dropUpdateRows(instanceId, (c) => gone.has(c.sha1));
  }

  const profileIfLeft = (id: string, scope: ModOpScope): string | null =>
    getInstanceId() !== id ? (scope.profileName ?? null) : null;

  async function run(id: string, scope: ModOpScope, targets: UpdateTarget[]) {
    busy = true;
    error = null;
    let attempts: UpdateAttempt[] = [];
    try {
      attempts = await runUpdates(id, targets);
    } finally {
      busy = false;
    }
    forget(updatedShas(attempts), id);
    await refresh();
    return { attempts, profile: profileIfLeft(id, scope) };
  }

  function targetOf(scope: ModOpScope, sha1: string, name?: string): UpdateTarget | null {
    const c = updateChecks.get(sha1);
    if (!c || c.state.kind !== 'update_available') return null;
    return { sha1, name: name ?? scope.nameOf?.(sha1) ?? c.name, target: c.state.target };
  }

  /** The row's own «Обновить». A plain success shows in the row itself; the update speaks when it
   *  failed, or when it brought dependencies in (D9). */
  async function updateOne(m: InstalledMod, name?: string) {
    const id = getInstanceId();
    if (!id) return;
    const scope = scopeFor(id);
    const target = targetOf(scope, m.sha1, name);
    if (!target) return;
    const { attempts, profile } = await run(id, scope, [target]);
    const [attempt] = attempts;
    if (attempt && (!attempt.ok || depsOf(attempt.summary).length > 0)) {
      pushUpdatesReport(attempts, { single: true, profile });
    }
  }

  /** Run the reviewed subset (D9): one update per sha1, then one notice for the run. */
  async function updateSelected(sha1s: readonly string[]) {
    const id = getInstanceId();
    if (!id) return;
    const scope = scopeFor(id);
    const targets = sha1s.flatMap((sha) => targetOf(scope, sha) ?? []);
    if (targets.length === 0) return;
    const { attempts, profile } = await run(id, scope, targets);
    pushUpdatesReport(attempts, { profile });
  }

  function isHeld(m: InstalledMod): boolean {
    return !!m.source && !!m.project_id && (holds?.has(holdKey(m.source, m.project_id)) ?? false);
  }

  /**
   * Hold a project («Не обновлять») or release it. A hold drops its pending update at once, as
   * the next stored read would; a release reads the stored check again, so an update the last
   * check found is offered once more. The hold list is then re-read, never guessed.
   *
   * A call for a project whose hold is still being written — a double click — does nothing and
   * returns false, silently: the write under way reports.
   */
  async function setHold(m: InstalledMod, hold: boolean, name: string): Promise<boolean> {
    const id = getInstanceId();
    const { source, project_id: projectId } = m;
    if (!id || !source || !projectId) return false;
    const claim = holdWrites.claim([`${id}\n${holdKey(source, projectId)}`]);
    try {
      return claim.free.size > 0 && (await writeHold(id, source, projectId, hold, name));
    } finally {
      claim.release();
    }
  }

  async function writeHold(
    id: string,
    source: ModSource,
    projectId: string,
    hold: boolean,
    name: string,
  ): Promise<boolean> {
    const failed = (reason: string) => {
      pushWarning(get(t)('mods.updates.holdFailed', { name }), [reason]);
      return false;
    };
    let r: Awaited<ReturnType<typeof commands.modsSetHold>>;
    try {
      r = await commands.modsSetHold(id, source, projectId, hold);
    } catch (e) {
      return failed(e instanceof Error ? e.message : String(e));
    }
    // A write under the shared claim: a refusal means another operation holds the profile (A9).
    if (r.status === 'error') return failed(modWriteReason(r.error));
    if (hold) dropUpdateRows(id, (c) => c.source === source && c.project_id === projectId);
    else await loadStoredUpdateCheck(id);
    await loadHolds(id);
    return true;
  }

  return {
    get updateChecks() {
      return updateChecks;
    },
    get checkedAtMs() {
      return checkedAtMs;
    },
    get holds() {
      return holds;
    },
    // The spinner belongs to the profile being checked, whichever view started the check.
    get checking() {
      return isCheckingUpdates(getInstanceId());
    },
    get updateCount() {
      return updateCount;
    },
    get updatableShas() {
      return updatableShas;
    },
    get showCfBanner() {
      return showCfBanner;
    },
    set showCfBanner(v: boolean) {
      showCfBanner = v;
    },
    get busy() {
      return busy;
    },
    get error() {
      return error;
    },
    set error(v: string | null) {
      error = v;
    },
    checkUpdates,
    loadStored,
    loadHolds,
    updateOne,
    updateSelected,
    forget,
    isHeld,
    setHold,
    dispose() {
      stopEffects?.();
    },
  };
}
