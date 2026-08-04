<script lang="ts">
  import { commands, type DatapackPlacementView, type WorldPackState } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { SvelteSet } from 'svelte/reactivity';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';

  // The post-install world picker (slice-2 design §7.3). Opens after a pack
  // lands in the library — catalog, file picker, or once per drag-drop batch —
  // and answers "which worlds should actually load it".
  //
  // Two rules this dialog exists to keep:
  //   * Zero worlds is an EXPLANATION, not an empty checkbox list — worlds are
  //     created in-game, and this is exactly the no-worlds case that motivated
  //     the whole library screen.
  //   * A world where the pack is present-but-DISABLED is re-enabled via
  //     datapacks_set_enabled_in_world, never datapacks_add_to_world: the add
  //     entry point ends with an unconditional enable by design, but it also
  //     re-materializes the file — the toggle is the honest, minimal edit.
  let {
    instanceId,
    filename,
    packName,
    placements,
    onClose,
    onApplied,
  }: {
    instanceId: string;
    filename: string;
    /** Display name for the title (falls back to the filename upstream). */
    packName: string;
    /** The pack's current per-world placements from the library listing. */
    placements: DatapackPlacementView[];
    onClose: () => void;
    /** Called after any world was changed, so the owner refreshes its view. */
    onApplied: () => void;
  } = $props();

  type Row = {
    world: string;
    /** null ⟹ the world does not reference the pack at all (addable). */
    state: WorldPackState | null;
    /** The world's level.dat was unreadable — no safe action exists. */
    unknown: boolean;
  };

  let rows = $state<Row[] | null>(null);
  let loadError = $state<string | null>(null);
  let busy = $state(false);
  const ticked = new SvelteSet<string>();

  $effect(() => {
    const reqInstance = instanceId;
    void (async () => {
      const res = await commands.listWorldNames(reqInstance);
      if (instanceId !== reqInstance) return;
      if (res.status !== 'ok') {
        loadError = formatError(res.error);
        rows = [];
        return;
      }
      const byWorld = new Map(placements.map((p) => [p.world.toLowerCase(), p]));
      const seen = new Set<string>();
      const out: Row[] = [];
      for (const w of res.data) {
        const p = byWorld.get(w.folder_name.toLowerCase());
        seen.add(w.folder_name.toLowerCase());
        out.push({
          world: w.folder_name,
          state: p ? p.state : null,
          unknown: p !== undefined && p.state === null,
        });
      }
      // A placement can name a world the quick listing missed (e.g. its
      // level.dat is locked); it still deserves a row rather than vanishing.
      for (const p of placements) {
        if (!seen.has(p.world.toLowerCase())) {
          out.push({ world: p.world, state: p.state, unknown: p.state === null });
        }
      }
      out.sort((a, b) => a.world.localeCompare(b.world));
      rows = out;
    })();
  });

  function selectable(row: Row): boolean {
    // Already enabled: nothing to do. Orphaned: the file is gone — repair
    // lives on the library row, not here. Unknown: acting blind on a world we
    // could not read would be a guess with a level.dat write attached.
    return !row.unknown && row.state !== 'enabled' && row.state !== 'orphaned';
  }

  function stateNote(row: Row): string | null {
    if (row.unknown) return $t('addons.datapacks.stateUnknown');
    switch (row.state) {
      case 'enabled':
        return $t('worlds.datapacks.stateEnabled');
      case 'disabled':
        return $t('worlds.datapacks.stateDisabled');
      case 'orphaned':
        return $t('worlds.datapacks.stateOrphaned');
      default:
        return null;
    }
  }

  async function apply() {
    if (ticked.size === 0 || rows === null) return;
    busy = true;
    try {
      let ok = 0;
      const failed: string[] = [];
      for (const row of rows) {
        if (!ticked.has(row.world)) continue;
        // Present-but-disabled ⟹ the minimal honest edit is the toggle;
        // everything else ticked here is a genuine add.
        const res =
          row.state === 'disabled'
            ? await commands.datapacksSetEnabledInWorld(instanceId, row.world, filename, true)
            : await commands.datapacksAddToWorld(instanceId, row.world, filename);
        if (res.status === 'ok') ok += 1;
        else failed.push(`${row.world}: ${formatError(res.error)}`);
      }
      if (ok > 0) pushSuccess(get(t)('addons.datapacks.picker.toastAdded', { count: ok }));
      if (failed.length > 0)
        pushWarning(
          get(t)('addons.datapacks.picker.toastFailed', { count: failed.length }),
          failed,
        );
      if (ok > 0) onApplied();
      onClose();
    } finally {
      busy = false;
    }
  }
</script>

<Modal ariaLabelledby="datapack-picker-title" {onClose} panelClass="w-full max-w-md">
  <div class="p-4 flex flex-col gap-3" data-testid="datapack-world-picker">
    <div class="flex items-start justify-between">
      <h2 id="datapack-picker-title" class="text-base font-semibold text-primary flex-1">
        {$t('addons.datapacks.picker.title', { name: packName })}
      </h2>
      <CloseButton onClick={onClose} ariaLabel={$t('common.close')} />
    </div>

    {#if loadError}
      <p class="text-sm text-danger">{loadError}</p>
    {/if}

    {#if rows === null}
      <div class="flex justify-center py-6 text-secondary">
        <Spinner labelPlacement="below" label={$t('common.loading')} />
      </div>
    {:else if rows.length === 0}
      <!-- The state the whole slice was motivated by: an instance with no
           worlds still has a working library, and the pack will appear here
           the moment a world exists. -->
      <p class="text-sm text-secondary" data-testid="datapack-picker-no-worlds">
        {$t('addons.datapacks.picker.noWorlds')}
      </p>
    {:else}
      <div class="flex flex-col gap-1 max-h-72 overflow-y-auto" role="group">
        {#each rows as row (row.world)}
          {@const note = stateNote(row)}
          <label
            class="flex items-center gap-2 rounded border border-border-subtle px-2 py-1.5 text-sm
              {selectable(row) ? '' : 'opacity-70'}"
          >
            <input
              type="checkbox"
              data-testid="datapack-picker-world"
              data-world={row.world}
              checked={row.state === 'enabled' || ticked.has(row.world)}
              disabled={!selectable(row) || busy}
              onchange={(e) => {
                if ((e.currentTarget as HTMLInputElement).checked) ticked.add(row.world);
                else ticked.delete(row.world);
              }}
            />
            <span class="flex-1 min-w-0 truncate text-primary">{row.world}</span>
            {#if note}
              <StatusBadge
                variant={row.state === 'enabled'
                  ? 'success'
                  : row.state === 'orphaned'
                    ? 'danger'
                    : row.state === 'disabled'
                      ? 'muted'
                      : 'neutral'}
              >
                {note}
              </StatusBadge>
            {/if}
          </label>
        {/each}
      </div>
    {/if}

    <div class="flex items-center justify-end gap-2 pt-1">
      <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={onClose}>
        {$t('addons.datapacks.picker.libraryOnly')}
      </button>
      {#if rows !== null && rows.length > 0}
        <BusyButton
          class="btn-primary btn-sm"
          {busy}
          disabled={ticked.size === 0}
          onclick={apply}
          data-testid="datapack-picker-apply"
        >
          {$t('addons.datapacks.picker.apply')}
        </BusyButton>
      {/if}
    </div>
  </div>
</Modal>
