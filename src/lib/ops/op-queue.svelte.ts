// Page-level orchestration for the four "fire and report" instance
// operations (integrity verify/repair, modpack import, launcher-instance
// import, instance clone) that used to be this module's own strictly-serial
// FIFO queue. The task registry (`$lib/tasks/registry.svelte`) and its
// adapters (`$lib/tasks/adapters/integrity`, `clone`, `launcher-import`,
// `pack-import`) now own progress and the running/queued/finished state that
// the retired `$lib/ops/OperationsView.svelte` used to render — see
// `$lib/tasks/OperationsBar.svelte` / `OperationsPanel.svelte` for the
// replacement UI, and `IntegritySection.svelte`, which now reads the
// registry's `taskFor` directly instead of a getter this module used to
// export (`opStatusFor`).
//
// What remains here is business logic the registry deliberately does NOT own
// (its own doc comment: "Pure state — no IPC in this module"):
//  - per-request dedupe, so a double click doesn't fire the same op twice;
//  - the completion toast, including the `ops.openInstance` action that
//    switches to a freshly imported/cloned instance;
//  - the two page-level completion ticks +page.svelte's effects latch onto
//    to refresh the instance list / force a modpack-update sweep.
//
// Cross-kind FIFO serialization — "exactly one SERIAL-lane op running at a
// time across all kinds" — IS reproduced, but not by this module: the
// registry's `start()` (`registry.svelte.ts`) hands each serial adapter a
// gate promise that only settles once the registry itself promotes the
// task, and every serial adapter (`integrity`, `clone`, `pack-import`,
// `pack-update`, `launcher-import`) awaits that gate BEFORE invoking its
// backend command. So a clone of instance A and a verify of instance B
// genuinely queue behind each other — assets/+libraries/ caches and the
// network chokepoint they share are serial by design, exactly like the old
// strictly-serial queue enforced, even though each call here still
// "invokes its adapter immediately" from this module's own point of view.
// This module still owns:
//  - per-instance dedupe for verify/repair/clone, via the registry's
//    `taskFor(scope)` (an active op — of ANY kind — for that instance blocks
//    a new one; see IntegritySection.svelte's doc comment for why this is a
//    deliberate widening of the old integrity-only guard);
//  - per-request dedupe for launcher-import/pack-import (no instance exists
//    yet at enqueue time, so there is no scope to key on), via a small local
//    in-flight set keyed the same way the old queue did (foreign root /
//    import path+source+version).

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { CloneRequest } from '$lib/instances/clone-request';
import type { LauncherImportRequest } from '$lib/instances/import/launcher-import-runner';
import { commands } from '$lib/ipc/bindings';
import type { ModpackImportRequest } from '$lib/modpacks/import-request';
import { cloneInstance } from '$lib/tasks/adapters/clone';
import { runRepair, runVerify } from '$lib/tasks/adapters/integrity';
import { importLauncherInstance } from '$lib/tasks/adapters/launcher-import';
import { importModpack } from '$lib/tasks/adapters/pack-import';
import { taskFor } from '$lib/tasks/registry.svelte';
import { pushActionToast, pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

export type IntegrityKind = 'verify' | 'repair';

let completionTick = $state(0);
// Bumped only when an import lands a new instance (ok/partial), so the page can
// force a modpack-update sweep for the freshly-imported pack without forcing one
// on every integrity op. Kept distinct from `completionTick` to avoid that over-fire.
let importCompletionTick = $state(0);

async function selectInstance(id: string): Promise<void> {
  if (!id) return;
  await commands.setActiveInstance(id);
  completionTick += 1; // page effect re-reads the active instance
}

// ── integrity (verify / repair) ─────────────────────────────────────────────

/** Enqueue verify/repair for an instance. Dedupe: an instance that already has
 *  ANY active op (of any kind — see the module doc comment) is a no-op. */
export function enqueueIntegrity(instanceId: string, name: string, kind: IntegrityKind): void {
  if (taskFor({ instanceId }) !== null) return;
  void runIntegrityOp(instanceId, name, kind);
}

async function runIntegrityOp(
  instanceId: string,
  name: string,
  kind: IntegrityKind,
): Promise<void> {
  try {
    const outcome =
      kind === 'verify' ? await runVerify(instanceId, name) : await runRepair(instanceId, name);
    const tr = get(t);
    if (outcome.status === 'ok') {
      const report = outcome.data;
      if (kind === 'verify') {
        if (report.healthy) {
          pushSuccess(tr('instance.integrity.toastVerifyOk', { name }));
        } else {
          pushWarning(
            tr('instance.integrity.toastVerifyProblems', {
              name,
              count: report.problems.length,
            }),
          );
        }
      } else if (report.healthy) {
        pushSuccess(tr('instance.integrity.toastRepaired', { name }));
      } else {
        pushWarning(
          tr('instance.integrity.toastRepairPartial', { name, count: report.problems.length }),
        );
      }
    } else if (outcome.status === 'cancelled') {
      // The user cancelled the queued op themselves — not a failure, so no
      // toast (see registry.svelte.ts's `cancelQueued`/`TaskCancelledError`).
    } else {
      pushWarning(tr('instance.integrity.toastError', { name, error: outcome.message }));
    }
  } finally {
    completionTick += 1;
  }
}

// ── modpack import ──────────────────────────────────────────────────────────

function importKey(r: ModpackImportRequest): string {
  return r.path || `${r.source}:${r.projectId}:${r.versionId}`;
}

/** In-flight import keys — no registry scope exists for a pack-import task
 *  until an instance is created mid-run, so dedupe can't use `taskFor`. */
const inFlightImportKeys = new Set<string>();

export function enqueueImport(name: string, request: ModpackImportRequest): void {
  const key = importKey(request);
  if (inFlightImportKeys.has(key)) return;
  inFlightImportKeys.add(key);
  void runImportOp(name, request, key);
}

async function runImportOp(
  name: string,
  request: ModpackImportRequest,
  key: string,
): Promise<void> {
  try {
    const outcome = await importModpack(name, request);
    const tr = get(t);
    if (outcome.status === 'ok') {
      const skippedLines = outcome.skipped.map((s) =>
        tr('page.modpackImport.skippedOverrideLine', {
          name: s.path.split('/').pop() ?? s.path,
          mb: Math.round((s.size ?? 0) / (1024 * 1024)),
        }),
      );
      const inertLines = outcome.inertLoaderJars.map((j) =>
        tr('page.modpackImport.inertLoaderLine', {
          filename: j.filename,
          loader: j.detected_loader,
        }),
      );
      const lines = [...skippedLines, ...inertLines];
      const title =
        outcome.skipped.length > 0
          ? tr('page.modpackImport.importedSkipped', {
              name: outcome.name,
              count: outcome.skipped.length,
            })
          : outcome.inertLoaderJars.length > 0
            ? tr('page.modpackImport.importedInertLoader', {
                name: outcome.name,
                count: outcome.inertLoaderJars.length,
              })
            : tr('page.modpackImport.imported', { name: outcome.name });
      const id = outcome.instanceId;
      pushActionToast(
        'success',
        title,
        { label: tr('ops.openInstance'), run: () => void selectInstance(id) },
        lines,
      );
      importCompletionTick += 1;
    } else if (outcome.status === 'partial') {
      pushWarning(
        tr('page.modpackImport.partialFailure', { count: outcome.failed.length }),
        outcome.failed,
      );
      importCompletionTick += 1;
    } else if (outcome.status === 'cancelled') {
      // The user cancelled the queued op themselves — not a failure, so no
      // toast (see registry.svelte.ts's `cancelQueued`/`TaskCancelledError`).
    } else {
      pushWarning(tr('page.modpackImport.failed'), [outcome.message]);
    }
  } finally {
    inFlightImportKeys.delete(key);
    completionTick += 1;
  }
}

// ── launcher-instance import ────────────────────────────────────────────────

/** In-flight foreign roots — same reasoning as `inFlightImportKeys` above. */
const inFlightLauncherImportRoots = new Set<string>();

export function enqueueLauncherImport(name: string, request: LauncherImportRequest): void {
  // Dedupe by the foreign instance root directory (same launcher instance path → same op).
  const key = request.foreign.root;
  if (inFlightLauncherImportRoots.has(key)) return;
  inFlightLauncherImportRoots.add(key);
  void runLauncherImportOp(name, request, key);
}

async function runLauncherImportOp(
  name: string,
  request: LauncherImportRequest,
  key: string,
): Promise<void> {
  try {
    const outcome = await importLauncherInstance(name, request);
    const tr = get(t);
    if (outcome.status === 'ok') {
      const id = outcome.instanceId;
      pushActionToast(
        'success',
        tr('instances.import.imported', { name: outcome.name }),
        { label: tr('ops.openInstance'), run: () => void selectInstance(id) },
        [],
      );
      importCompletionTick += 1;
    } else if (outcome.status === 'cancelled') {
      // The user cancelled the queued op themselves — not a failure, so no
      // toast (see registry.svelte.ts's `cancelQueued`/`TaskCancelledError`).
    } else {
      pushWarning(tr('instances.import.failed'), [outcome.message]);
    }
  } finally {
    inFlightLauncherImportRoots.delete(key);
    completionTick += 1;
  }
}

// ── instance clone ──────────────────────────────────────────────────────────

/** Enqueue an instance clone. Dedupe: a source instance that already has an
 *  active op (of any kind) is a no-op (double-click guard). */
export function enqueueClone(name: string, request: CloneRequest): void {
  if (taskFor({ instanceId: request.sourceId }) !== null) return;
  void runCloneOp(name, request);
}

async function runCloneOp(name: string, request: CloneRequest): Promise<void> {
  try {
    const outcome = await cloneInstance(name, request);
    const tr = get(t);
    if (outcome.status === 'ok') {
      const id = outcome.instanceId;
      pushActionToast(
        'success',
        tr('instance.clone.done', { name: outcome.name }),
        { label: tr('ops.openInstance'), run: () => void selectInstance(id) },
        [],
      );
    } else if (outcome.status === 'cancelled') {
      // The user cancelled the queued op themselves — not a failure, so no
      // toast (see registry.svelte.ts's `cancelQueued`/`TaskCancelledError`).
    } else {
      pushWarning(tr('instance.clone.failed', { name }), [outcome.message]);
    }
  } finally {
    completionTick += 1;
  }
}

// ── page-level completion ticks ─────────────────────────────────────────────

/** Bumps once per completed op (any kind, success or failure) — the page's
 *  `integritySettled` effect refreshes the instance list from this. Also
 *  bumped a second time by `selectInstance` above so the "Open" toast action
 *  re-reads the active instance. */
export function opCompletionTick(): number {
  return completionTick;
}
/** Bumps when an import lands a new instance (ok/partial); drives the page's
 *  forced post-import modpack-update sweep. */
export function opImportCompletionTick(): number {
  return importCompletionTick;
}

/** Test-only reset of the module singleton. */
export function __resetOpQueueForTest(): void {
  completionTick = 0;
  importCompletionTick = 0;
  inFlightImportKeys.clear();
  inFlightLauncherImportRoots.clear();
}
