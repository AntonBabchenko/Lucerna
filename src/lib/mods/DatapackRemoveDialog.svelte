<script lang="ts">
  import { commands, type DatapackPlacementView } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';

  // The ONE removal confirmation for datapacks, shared by the library screen
  // and the catalog card's trash action (slice-2 design §7.6) — the catalog
  // seam is the #4083 hazard from the other side: a one-click silent delete
  // that must cascade into every world holding the pack.
  //
  // «Убрать также из миров» is CHECKED by default. Unchecked leaves the world
  // links in place — the pack keeps loading in game, and the library listing
  // then shows it as "only in worlds" — which is a supported state, not an
  // error, so the note spells the consequence out instead of forbidding it.
  let {
    instanceId,
    filename,
    packName,
    placements,
    inLibrary,
    onClose,
    onRemoved,
  }: {
    instanceId: string;
    filename: string;
    packName: string;
    /** Worlds referencing the pack, from the library listing. */
    placements: DatapackPlacementView[];
    /**
     * `false` ⟹ a worlds-only row (`in_library: false`). The library cascade
     * cannot act on those: its identity check compares world copies against a
     * library file that no longer exists, so every world comes back
     * `kept_not_ours` and nothing is removed. The user confirmed this exact
     * world list, so the honest action is per-world removal by name.
     */
    inLibrary: boolean;
    onClose: () => void;
    /** Called after the removal ran (fully or partially) so the owner refreshes. */
    onRemoved: () => void;
  } = $props();

  let cascade = $state(true);
  let busy = $state(false);

  async function confirmWorldsOnly() {
    let removed = 0;
    const failed: string[] = [];
    for (const p of placements) {
      const res = await commands.datapacksRemoveFromWorld(instanceId, p.world, filename);
      if (res.status === 'ok') removed += 1;
      else failed.push(`${p.world}: ${formatError(res.error)}`);
    }
    if (failed.length > 0) {
      pushWarning(
        get(t)('addons.datapacks.remove.toastFailedWorlds', { count: failed.length }),
        failed,
      );
    } else if (removed > 0) {
      pushSuccess(get(t)('addons.datapacks.remove.toastRemoved', { name: packName }));
    }
  }

  async function confirmFromLibrary() {
    const res = await commands.datapacksRemoveFromLibrary(instanceId, filename, cascade);
    if (res.status !== 'ok') {
      pushWarning(get(t)('addons.datapacks.remove.toastFailed', { name: packName }), [
        formatError(res.error),
      ]);
      return;
    }
    // Per-world precision (F3): every world that still holds the pack after
    // this removal is NAMED — a silent partial success would leave content
    // loading in game with the UI claiming it is gone.
    const failed = res.data.worlds.filter((w) => w.kind === 'failed');
    const keptNotOurs = res.data.worlds.filter((w) => w.kind === 'kept_not_ours');
    if (failed.length > 0) {
      pushWarning(
        get(t)('addons.datapacks.remove.toastFailedWorlds', { count: failed.length }),
        failed.map((w) => (w.kind === 'failed' ? `${w.world}: ${w.details}` : w.kind)),
      );
    }
    if (cascade && keptNotOurs.length > 0) {
      pushWarning(
        get(t)('addons.datapacks.remove.toastKeptNotOurs', { count: keptNotOurs.length }),
        keptNotOurs.map((w) => w.world),
      );
    }
    if (failed.length === 0 && (!cascade || keptNotOurs.length === 0)) {
      pushSuccess(get(t)('addons.datapacks.remove.toastRemoved', { name: packName }));
    }
  }

  async function confirm() {
    busy = true;
    try {
      if (inLibrary) await confirmFromLibrary();
      else await confirmWorldsOnly();
      onRemoved();
      onClose();
    } finally {
      busy = false;
    }
  }
</script>

<Modal ariaLabelledby="datapack-remove-title" {onClose} panelClass="w-full max-w-md">
  <div class="p-4 flex flex-col gap-3" data-testid="datapack-remove-dialog">
    <div class="flex items-start justify-between">
      <h2 id="datapack-remove-title" class="text-base font-semibold text-primary flex-1">
        {$t('addons.datapacks.remove.title', { name: packName })}
      </h2>
      <CloseButton onClick={onClose} ariaLabel={$t('common.close')} />
    </div>

    {#if placements.length > 0}
      <div class="text-sm text-secondary">
        <p>{$t('addons.datapacks.remove.affectedWorlds', { count: placements.length })}</p>
        <ul class="mt-1 list-disc list-inside text-primary">
          {#each placements as p (p.world)}
            <li class="truncate">{p.world}</li>
          {/each}
        </ul>
      </div>
      {#if inLibrary}
        <label class="flex items-start gap-2 text-sm text-primary">
          <input type="checkbox" bind:checked={cascade} disabled={busy} class="mt-0.5" />
          <span>
            {$t('addons.datapacks.remove.cascade')}
            <span class="block text-xs text-muted">
              {$t('addons.datapacks.remove.keepNote')}
            </span>
          </span>
        </label>
      {:else}
        <!-- A worlds-only row: there is no library copy left, so the ONLY
             thing this removal can do is clear the listed worlds. -->
        <p class="text-xs text-muted">{$t('addons.datapacks.remove.worldsOnlyNote')}</p>
      {/if}
    {:else}
      <p class="text-sm text-secondary">{$t('addons.datapacks.remove.bodyNoWorlds')}</p>
    {/if}

    <div class="flex items-center justify-end gap-2 pt-1">
      <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={onClose}>
        {$t('common.cancel')}
      </button>
      <BusyButton
        class="btn-danger btn-sm"
        {busy}
        onclick={confirm}
        data-testid="datapack-remove-confirm"
      >
        {$t('addons.datapacks.remove.confirm')}
      </BusyButton>
    </div>
  </div>
</Modal>
