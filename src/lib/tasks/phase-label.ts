// Per-kind "what is the task DOING right now" text — the thing the strip
// dropped when it moved off the event-driven per-surface components. Those
// components (`$lib/install/PhaseStatusRow.svelte`'s `phaseLabel()` /
// `modPhaseLabel()`, `$lib/ops/OperationsView.svelte`'s `importPhaseLabel()`
// + its clone/launcher-import inline phase text) each hand-rolled a switch
// over ONE kind's raw phase enum. This module is the same mapping, moved
// here and reindexed by `TaskKind` so every task surface (bottom strip,
// expanded panel, eventually the report modal) can share it — see the
// module doc comment on `./types.ts`.
//
// Deliberately reuses the i18n keys those components already had rather
// than inventing parallel copy: `install.phase.*`, `install.modPhase.*`,
// `modpacks.import.progress.phase*`, `instances.import.phase.*`,
// `instance.clone.copying`, and (a mapping the two deleted components never
// had, found instead in `$lib/settings/DataLocationProgressDialog.svelte`,
// which is NOT being deleted — it is the modal a `data-migration` task
// already owns) `settings.storage.dataLocation.progress.*`.
//
// `Task.phase` is only the raw backend discriminant string (see
// `registry.svelte.ts`'s adapters, which reduce the wire union down to its
// `.phase` tag before storing it) — never the full tagged-union payload. Two
// consequences follow directly from that:
//
// 1. `pack-import`/`pack-update`'s `installing_file` phase needs `{fileName}`
//    to fill `modpacks.import.progress.phaseInstallingFile`, and nothing on
//    `Task` carries it (the adapter's `translateProgress` discards
//    `ModpackProgress`'s `file_name` field along with the rest of the
//    tagged union). Rather than interpolate a blank/invented name, that one
//    phase renders no text — the strip still shows the kind label plus
//    whatever byte/file progress is flowing, just no phase line for this
//    one tick.
// 2. `creating_instance` needs `{name}`, which `Task.title` already carries
//    (the adapters set `title` to the same pack/instance name the backend's
//    `name` field would report), so that one substitutes cleanly.
//
// Every `TaskKind` gets a deliberate entry, `null` included: a kind whose
// raw phase space has no existing i18n mapping to reuse (verify, repair —
// no per-`VerifyPhase` text ever existed in the old UI; server-upload — the
// raw phase IS a file path, not a translatable enum; app-update — the
// adapter never sets `phase` at all) is `null` here, not a fallthrough
// default, so a future kind added to `TASK_KINDS` without a considered
// choice fails to compile.
import type { Translate } from '$lib/i18n/typed';
import { categoryLabelKey } from '$lib/instances/import/category-display';
import type { ContentCategory } from '$lib/ipc/bindings';
import type { Task, TaskKind } from './types';

const CONTENT_CATEGORIES: readonly ContentCategory[] = [
  'mods',
  'config',
  'saves',
  'resource_packs',
  'shaderpacks',
  'options_txt',
];

function isContentCategory(value: string): value is ContentCategory {
  return (CONTENT_CATEGORIES as readonly string[]).includes(value);
}

type PhaseLabelFn = (tr: Translate, task: Task) => string | null;

/** `install.phase.*` — mirrors `PhaseStatusRow.svelte`'s `phaseLabel()`. */
function gameInstallPhase(tr: Translate, task: Task): string | null {
  switch (task.phase) {
    case 'manifest':
      return tr('install.phase.manifest');
    case 'forge_install':
      return tr('install.phase.forge_install');
    case 'jre':
      return tr('install.phase.jre');
    case 'libraries':
      return tr('install.phase.libraries');
    case 'assets':
      return tr('install.phase.assets');
    case 'client':
      return tr('install.phase.client');
    case 'complete':
      return tr('install.phase.complete');
    default:
      return null;
  }
}

/** `install.modPhase.*` — shared by `mod-install` and `mod-update`, which
 *  both stream the same `ModInstallProgress['phase']` space. Mirrors
 *  `PhaseStatusRow.svelte`'s `modPhaseLabel()`. */
function modInstallPhase(tr: Translate, task: Task): string | null {
  switch (task.phase) {
    case 'downloading':
      return tr('install.modPhase.downloading');
    case 'verifying':
      return tr('install.modPhase.verifying');
    case 'copying':
      return tr('install.modPhase.copying');
    default:
      return null;
  }
}

/** `modpacks.import.progress.phase*` — shared by `pack-import` and
 *  `pack-update`, which both stream `ModpackProgress`. Mirrors
 *  `OperationsView.svelte`'s `importPhaseLabel()`, minus `installing_file`
 *  (see the module doc comment). */
function modpackPhase(tr: Translate, task: Task): string | null {
  switch (task.phase) {
    case 'inspecting':
      return tr('modpacks.import.progress.phaseInspecting');
    case 'creating_instance':
      return tr('modpacks.import.progress.phaseCreatingInstance', { name: task.title });
    case 'installing_file':
      // The key needs `{fileName}`, which the registry's bare `phase: string`
      // cannot carry — see the module doc comment. No text rather than a
      // blank interpolation.
      return null;
    case 'extracting_overrides':
      return task.progress === null
        ? null
        : tr('modpacks.import.progress.phaseExtractingOverrides', {
            current: task.progress.current,
            total: task.progress.total,
          });
    case 'enriching':
      return tr('modpacks.import.progress.phaseEnriching');
    case 'done':
      return tr('modpacks.import.progress.phaseDone');
    default:
      return null;
  }
}

/** `instances.import.phase.*` — all four variants are argument-free, so
 *  `launcher-import` (unlike pack-import) loses nothing by only keeping the
 *  raw phase discriminant. */
function launcherImportPhase(tr: Translate, task: Task): string | null {
  switch (task.phase) {
    case 'creating_instance':
      return tr('instances.import.phase.creating_instance');
    case 'copying':
      return tr('instances.import.phase.copying');
    case 'recovering_identities':
      return tr('instances.import.phase.recovering_identities');
    case 'done':
      return tr('instances.import.phase.done');
    default:
      return null;
  }
}

/** `instance.clone.copying` — the adapter stores the content category
 *  itself as `phase` (see `adapters/clone.ts`), so the category label comes
 *  from the same `categoryLabelKey` map the launcher-import dialog and
 *  clone dialog already use. */
function clonePhase(tr: Translate, task: Task): string | null {
  if (task.phase === null || task.progress === null || !isContentCategory(task.phase)) {
    return null;
  }
  return tr('instance.clone.copying', {
    category: tr(categoryLabelKey(task.phase)),
    current: task.progress.current,
    total: task.progress.total,
  });
}

/** `settings.storage.dataLocation.progress.*` — reused from
 *  `DataLocationProgressDialog.svelte` (the modal a `data-migration` task
 *  already owns; that component is NOT one of the two being deleted). Its
 *  `null`-phase "preparing" case corresponds exactly to a `data-migration`
 *  task before its first tick: `DataMigrationProgress.phase` is always a
 *  populated string once a tick arrives (see `adapters/data-migration.ts`),
 *  so `Task.phase === null` here only ever means "not ticked yet". */
function dataMigrationPhase(tr: Translate, task: Task): string | null {
  switch (task.phase) {
    case null:
      return tr('settings.storage.dataLocation.progress.preparing');
    case 'copying':
      return tr('settings.storage.dataLocation.progress.copying');
    case 'verifying':
      return tr('settings.storage.dataLocation.progress.verifying');
    case 'deleting':
      return tr('settings.storage.dataLocation.progress.deleting');
    default:
      return null;
  }
}

/** One deliberate entry per kind — `null` for a kind with no phase
 *  vocabulary to reuse (see the module doc comment for why each is `null`).
 *  `Record<TaskKind, …>` rather than a lookup function: a new kind added to
 *  `TASK_KINDS` without an entry here fails to compile. */
const PHASE_LABEL: Record<TaskKind, PhaseLabelFn | null> = {
  'game-install': gameInstallPhase,
  'mod-install': modInstallPhase,
  'mod-update': modInstallPhase,
  'pack-import': modpackPhase,
  'pack-update': modpackPhase,
  'launcher-import': launcherImportPhase,
  clone: clonePhase,
  verify: null,
  repair: null,
  'server-upload': null,
  'app-update': null,
  'data-migration': dataMigrationPhase,
};

/** The localized "what is this task doing" line for `task`, or `null` when
 *  its kind has no phase vocabulary, its current raw phase isn't one this
 *  module maps, or (pack-import/pack-update's `installing_file`) the
 *  mapping exists but the data it needs was never available. */
export function phaseLabel(tr: Translate, task: Task): string | null {
  const fn = PHASE_LABEL[task.kind];
  return fn === null ? null : fn(tr, task);
}
