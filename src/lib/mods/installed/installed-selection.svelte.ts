import type { ModUpdateCheck } from '$lib/ipc/bindings';
import {
  disableMods,
  enableMods,
  type ModOpOutcome,
  type ModOpScope,
  type ModOpTarget,
  uninstallMods,
} from '$lib/mods/mod-ops.svelte';
import { createListSelection } from '$lib/ui/list-selection.svelte';
import type { Row } from './installed-data.svelte';
import { rowDisplayName } from './row-utils';
import { pushUpdatesReport, runUpdates, type UpdateTarget, updatedShas } from './update-review';

// Which bulk action is currently in flight, so the bulk bar can spin only the
// clicked button (not all four). `null` when idle.
export type BulkAction = 'enable' | 'disable' | 'update' | 'uninstall';

const toTarget = (r: Row): ModOpTarget => ({ sha1: r.installed.sha1, name: rowDisplayName(r) });

// Owns the bulk operations of the installed mods list. The selection itself — the keys, select
// all, pruning to the filtered rows, the clear on an instance switch — is the shared model every
// bulk bar uses (`createListSelection`); this composable adds the mod operations on top of it.
// `getUpdateChecks` lets bulk Update target only rows with a pending update (a held project has
// none); `onMutated` is called after install-set changes (uninstall) so the caller can
// invalidate the graph. Enable, disable and removal go through the guarded path (mod-ops), which
// asks about dependents, requirements and unneeded libraries and reports the outcome; every
// operation here reports through a toast.
export function createInstalledSelection(
  getFiltered: () => Row[],
  getInstanceId: () => string | null,
  refresh: () => Promise<void>,
  getUpdateChecks: () => Map<string, ModUpdateCheck>,
  onMutated: () => void,
  // What a guarded operation captures (instance, profile name, row names); the view supplies it.
  scopeFor: (instanceId: string) => ModOpScope = (instanceId) => ({ instanceId }),
) {
  const sel = createListSelection(() => getFiltered().map((r) => r.installed.sha1), getInstanceId);
  let busy = $state(false);
  // The specific bulk action in flight (drives per-button spinners); `busy`
  // stays the aggregate gate that disables the whole bar.
  let busyAction = $state<BulkAction | null>(null);

  const selectedRows = $derived(getFiltered().filter((r) => sel.selected.has(r.installed.sha1)));
  const selectedUpdatable = $derived(
    selectedRows.filter(
      (r) => getUpdateChecks().get(r.installed.sha1)?.state.kind === 'update_available',
    ),
  );

  // Through the guarded path (spec §6.1): one impact check for the selection, one dialog, then
  // the flips, which report their own outcome. Rows already in the wanted state are left alone;
  // a cancelled dialog keeps the selection so the user can adjust it.
  async function bulkSetEnabled(enable: boolean) {
    const id = getInstanceId();
    if (!id || sel.count === 0) return;
    const targets = selectedRows.filter((r) => r.installed.enabled !== enable).map(toTarget);
    busy = true;
    busyAction = enable ? 'enable' : 'disable';
    let outcome: ModOpOutcome = 'cancelled';
    try {
      if (targets.length > 0)
        outcome = enable
          ? await enableMods(scopeFor(id), targets, { bulk: true })
          : await disableMods(scopeFor(id), targets, { bulk: true });
    } finally {
      busy = false;
      busyAction = null;
    }
    if (outcome === 'cancelled' && targets.length > 0) return;
    sel.clear();
    await refresh();
  }

  // The selected rows with a pending update, one after another, then one notice for the run
  // (D9: the dependencies they installed, why the rest failed). Returns the sha1s that updated, so
  // the caller drops their now-stale checks — and only those.
  async function bulkUpdate(): Promise<string[]> {
    const id = getInstanceId();
    if (!id) return [];
    const checks = getUpdateChecks();
    const targets = selectedUpdatable.flatMap((r): UpdateTarget[] => {
      const st = checks.get(r.installed.sha1)?.state;
      return st?.kind === 'update_available'
        ? [{ sha1: r.installed.sha1, name: rowDisplayName(r), target: st.target }]
        : [];
    });
    if (targets.length === 0) return [];
    const scope = scopeFor(id);
    busy = true;
    busyAction = 'update';
    let attempts: Awaited<ReturnType<typeof runUpdates>> = [];
    try {
      attempts = await runUpdates(id, targets);
    } finally {
      busy = false;
      busyAction = null;
    }
    sel.clear();
    await refresh();
    const profile = getInstanceId() !== id ? (scope.profileName ?? null) : null;
    pushUpdatesReport(attempts, { profile });
    return updatedShas(attempts);
  }

  // ONE guarded removal for the selection (spec §6.1): the dependents dialog if any, the
  // unneeded-libraries question if any, then one `mods_uninstall_many` → one token → one toast.
  async function requestBulkUninstall() {
    const id = getInstanceId();
    if (!id || sel.count === 0) return;
    const targets = selectedRows.map(toTarget);
    busy = true;
    busyAction = 'uninstall';
    let outcome: ModOpOutcome = 'cancelled';
    try {
      outcome = await uninstallMods(scopeFor(id), targets, { offerOrphans: true });
    } finally {
      busy = false;
      busyAction = null;
    }
    if (outcome === 'cancelled') return;
    sel.clear();
    // Removed mods would otherwise linger as stale roots in the dep tree.
    onMutated();
    await refresh();
  }

  return {
    get selected() {
      return sel.selected;
    },
    get allSelected() {
      return sel.allSelected;
    },
    get selectedUpdatable() {
      return selectedUpdatable;
    },
    get busy() {
      return busy;
    },
    get busyAction() {
      return busyAction;
    },
    toggleSelect: sel.toggle,
    toggleSelectAll: sel.toggleAll,
    clear: sel.clear,
    bulkSetEnabled,
    bulkUpdate,
    requestBulkUninstall,
    dispose: sel.dispose,
  };
}
