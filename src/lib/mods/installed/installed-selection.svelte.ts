import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, type ModUpdateCheck, type ModVersion, type OrphanRef } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { updateMod } from '$lib/tasks/adapters/mod-install';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
import type { Row } from './installed-data.svelte';
import { rowDisplayName } from './row-utils';

// Which bulk action is currently in flight, so the bulk bar can spin only the
// clicked button (not all four). `null` when idle.
export type BulkAction = 'enable' | 'disable' | 'update' | 'uninstall';

// Owns bulk-action selection state and the bulk operations themselves. The
// `selected` Set is always reassigned whole (never mutated in place). Selections
// for rows hidden by a filter/search change are dropped. `getUpdateChecks` lets
// bulk Update target only rows with a pending update; `onMutated` is called
// after install-set changes (uninstall) so the caller can invalidate the graph.
export function createInstalledSelection(
  getFiltered: () => Row[],
  getInstanceId: () => string | null,
  refresh: () => Promise<void>,
  getUpdateChecks: () => Map<string, ModUpdateCheck>,
  onMutated: () => void,
) {
  let selected = $state<Set<string>>(new Set());
  let busy = $state(false);
  // The specific bulk action in flight (drives per-button spinners); `busy`
  // stays the aggregate gate that disables the whole bar.
  let busyAction = $state<BulkAction | null>(null);
  let error = $state<string | null>(null);
  let uninstallPrompt = $state<{
    removing: string[];
    names: string[];
    orphans: OrphanRef[];
  } | null>(null);

  const selectedRows = $derived(getFiltered().filter((r) => selected.has(r.installed.sha1)));
  const allSelected = $derived.by(() => {
    const rows = getFiltered();
    return rows.length > 0 && rows.every((r) => selected.has(r.installed.sha1));
  });
  const selectedUpdatable = $derived(
    selectedRows.filter(
      (r) => getUpdateChecks().get(r.installed.sha1)?.state.kind === 'update_available',
    ),
  );

  // Drop selections for rows no longer visible (filter/search change), and clear
  // selection on instance switch. Wrapped in $effect.root for unit-testability
  // and torn down via dispose() on component unmount.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      // Both effects below write `selected`. They are order-safe: on an instance
      // switch the clear effect empties the set, then the drop-hidden effect sees
      // size 0 === size 0 and skips its write. Keep the clear effect last so a
      // switch always wins; do not reorder.
      $effect(() => {
        // Drop selections for rows no longer visible (filter/search change).
        const visible = new Set(getFiltered().map((r) => r.installed.sha1));
        const next = new Set([...selected].filter((sha) => visible.has(sha)));
        if (next.size !== selected.size) selected = next;
      });
      $effect(() => {
        // Clear selection on instance switch.
        void getInstanceId();
        selected = new Set();
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — effects inert, which is what unit tests want */
  }

  function toggleSelect(sha1: string, checked: boolean) {
    const next = new Set(selected);
    if (checked) next.add(sha1);
    else next.delete(sha1);
    selected = next;
  }
  function toggleSelectAll(checked: boolean) {
    selected = checked ? new Set(getFiltered().map((r) => r.installed.sha1)) : new Set();
  }
  function clear() {
    selected = new Set();
  }

  async function applyUpdate(sha1: string, target: ModVersion): Promise<boolean> {
    const id = getInstanceId();
    if (!id) return false;
    const r = await updateMod(id, target.name, sha1, target);
    if (r.status === 'error') {
      error = formatError(r.error);
      return false;
    }
    return true;
  }

  async function bulkSetEnabled(enable: boolean) {
    const id = getInstanceId();
    if (!id || selected.size === 0) return;
    busy = true;
    busyAction = enable ? 'enable' : 'disable';
    error = null;
    let ok = 0;
    let failed = 0;
    for (const r of selectedRows) {
      if (r.installed.enabled === enable) {
        ok++;
        continue;
      }
      const res = enable
        ? await commands.modsEnable(id, r.installed.sha1)
        : await commands.modsDisable(id, r.installed.sha1);
      if (res.status === 'error') failed++;
      else ok++;
    }
    busy = false;
    busyAction = null;
    selected = new Set();
    await refresh();
    if (failed === 0) {
      pushSuccess(
        get(t)(enable ? 'mods.installed.toastEnabled' : 'mods.installed.toastDisabled', {
          count: ok,
        }),
      );
    } else {
      pushWarning(
        get(t)(
          enable ? 'mods.installed.toastEnabledFailed' : 'mods.installed.toastDisabledFailed',
          {
            count: ok,
            failed,
          },
        ),
        [],
      );
    }
  }

  async function bulkUpdate() {
    const id = getInstanceId();
    if (!id) return;
    const checks = getUpdateChecks();
    const targets = selectedUpdatable.flatMap((r) => {
      const st = checks.get(r.installed.sha1)?.state;
      return st?.kind === 'update_available' ? [{ sha1: r.installed.sha1, target: st.target }] : [];
    });
    if (targets.length === 0) return;
    busy = true;
    busyAction = 'update';
    error = null;
    let ok = 0;
    let failed = 0;
    for (const tgt of targets) {
      if (await applyUpdate(tgt.sha1, tgt.target)) ok++;
      else failed++;
    }
    busy = false;
    busyAction = null;
    selected = new Set();
    await refresh();
    if (failed === 0) pushSuccess(get(t)('mods.installed.toastUpdated', { count: ok }));
    else pushWarning(get(t)('mods.installed.toastUpdatedFailed', { count: ok, failed }), []);
  }

  async function requestBulkUninstall() {
    const id = getInstanceId();
    if (!id || selected.size === 0) return;
    const removing = selectedRows.map((r) => r.installed.sha1);
    const names = selectedRows.map((r) => rowDisplayName(r));
    const r = await commands.modsFindOrphans(id, removing);
    const orphans = r.status === 'ok' ? r.data : [];
    uninstallPrompt = { removing, names, orphans };
  }

  async function confirmBulkUninstall(alsoRemove: string[]) {
    const id = getInstanceId();
    if (!id || !uninstallPrompt) return;
    const all = [...uninstallPrompt.removing, ...alsoRemove];
    uninstallPrompt = null;
    busy = true;
    busyAction = 'uninstall';
    error = null;
    let ok = 0;
    let failed = 0;
    for (const sha1 of all) {
      const res = await commands.modsUninstall(id, sha1);
      if (res.status === 'error') failed++;
      else ok++;
    }
    busy = false;
    busyAction = null;
    selected = new Set();
    // Removed mods would otherwise linger as stale roots in the dep tree.
    onMutated();
    await refresh();
    if (failed === 0) pushSuccess(get(t)('mods.installed.toastUninstalled', { count: ok }));
    else pushWarning(get(t)('mods.installed.toastUninstalledFailed', { count: ok, failed }), []);
  }

  function cancelUninstall() {
    uninstallPrompt = null;
  }

  return {
    get selected() {
      return selected;
    },
    get allSelected() {
      return allSelected;
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
    get error() {
      return error;
    },
    get uninstallPrompt() {
      return uninstallPrompt;
    },
    toggleSelect,
    toggleSelectAll,
    clear,
    bulkSetEnabled,
    bulkUpdate,
    requestBulkUninstall,
    confirmBulkUninstall,
    cancelUninstall,
    dispose() {
      stopEffects?.();
    },
  };
}
