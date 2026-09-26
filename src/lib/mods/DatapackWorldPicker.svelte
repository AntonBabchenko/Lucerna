<script lang="ts">
  import {
    commands,
    type DatapackPlacementView,
    type DatapackWorldView,
    type IgnoredReason,
    type LevelDatPresence,
    type PackCompat,
    type WorldPackState,
  } from '$lib/ipc/bindings';
  import { ignoredLabelKey } from '$lib/worlds/datapack-state';
  import { levelDatBlockedKey } from '$lib/worlds/datapacks-gating';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { compatLine } from '$lib/worlds/datapack-compat';
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
    worlds,
    compat,
    onClose,
    onApplied,
  }: {
    instanceId: string;
    filename: string;
    /** Display name for the title (falls back to the filename upstream). */
    packName: string;
    /** The pack's current per-world placements from the library listing. */
    placements: DatapackPlacementView[];
    /**
     * Every world folder the library listing saw, with its level.dat presence
     * (`DatapackLibraryView.worlds`). Required: a world the pack is not in yet
     * has no placement, and only this says whether Lucerna may change anything
     * there (§3 L.6/L.8).
     */
    worlds: DatapackWorldView[];
    /** The pack's verdict from the library listing; null when it could not be read. */
    compat: PackCompat | null;
    onClose: () => void;
    /** Called after any world was changed, so the owner refreshes its view. */
    onApplied: () => void;
  } = $props();

  type Row = {
    world: string;
    /** null ⟹ the world does not reference the pack at all (addable). */
    state: WorldPackState | null;
    /**
     * The world's placement has no state: its lists or its datapacks folder
     * could not be read, or the folder has no level file (see `levelDat`) —
     * no safe action exists.
     */
    unknown: boolean;
    /**
     * The world's level.dat presence; null when it could not be told or the
     * library listing did not see the folder. Anything but 'present' means
     * the backend refuses the picker's add and toggle there.
     */
    levelDat: LevelDatPresence | null;
    /**
     * Why the game ignores this world's entry when `state` is 'ignored'. Also
     * 'unreadable' on an `unknown` row Lucerna could not check at all.
     */
    ignoredReason: IgnoredReason | null;
  };

  // §0.5 I11: a pack this version skips is not offered for adding. The
  // backend does not refuse it (the file is valid and the game only skips
  // it), so the picker offers no world and says why.
  const skipped = $derived(
    compat !== null && compat.kind === 'wont_load' ? compatLine(compat) : null,
  );

  let rows = $state<Row[] | null>(null);
  let loadError = $state<string | null>(null);
  let busy = $state(false);
  const ticked = new SvelteSet<string>();

  $effect(() => {
    const reqInstance = instanceId;
    // Set when the picker closes (or the instance changes) mid-load: the
    // answer is dropped before any prop is read, since a closed picker's props
    // may already point at nothing.
    let cancelled = false;
    void (async () => {
      const res = await commands.listWorldNames(reqInstance);
      if (cancelled || instanceId !== reqInstance) return;
      if (res.status !== 'ok') {
        loadError = formatError(res.error);
        rows = [];
        return;
      }
      // Everything here is keyed by the EXACT folder name: `folder_name`,
      // `DatapackPlacementView.world` and `DatapackWorldView.world` all come
      // from a read_dir of saves/, and on a case-sensitive filesystem two
      // names differing in case are two worlds. Case-folding would hand one
      // world the other's placement, and a tick would then toggle a world that
      // never held the pack. A world the library saw wins over its placement,
      // including its null ("seen, could not tell"): a placement's answer
      // must never turn "could not tell" into a known state.
      const byWorld = new Map(placements.map((p) => [p.world, p]));
      const presence = new Map(worlds.map((w) => [w.world, w.level_dat]));
      const levelDatOf = (name: string, p: DatapackPlacementView | undefined) =>
        presence.has(name) ? (presence.get(name) ?? null) : (p?.level_dat ?? null);
      const seen = new Set<string>();
      const out: Row[] = [];
      for (const w of res.data) {
        const p = byWorld.get(w.folder_name);
        seen.add(w.folder_name);
        out.push({
          world: w.folder_name,
          state: p ? p.state : null,
          unknown: p !== undefined && p.state === null,
          levelDat: levelDatOf(w.folder_name, p),
          ignoredReason: p ? p.ignored_reason : null,
        });
      }
      // A placement can name a world the quick listing missed (e.g. its
      // level.dat is locked); it still deserves a row rather than vanishing.
      for (const p of placements) {
        if (!seen.has(p.world)) {
          out.push({
            world: p.world,
            state: p.state,
            unknown: p.state === null,
            levelDat: levelDatOf(p.world, p),
            ignoredReason: p.ignored_reason,
          });
        }
      }
      out.sort((a, b) => a.world.localeCompare(b.world));
      rows = out;
    })();
    return () => {
      cancelled = true;
    };
  });

  /**
   * Could not tell the world's level.dat — or the library listing never saw
   * the folder (a world created after it) — or could not read its pack lists.
   * Shown, never ticked blind: the backend refuses absent/only-old worlds
   * anyway (§3 L.4), but a tick must not be a guess.
   */
  function isUnknown(row: Row): boolean {
    return row.unknown || row.levelDat === null;
  }

  /** Why this world takes no change at all because of its level.dat (D2). */
  function blockedKey(row: Row): TranslationKey | null {
    return levelDatBlockedKey(row.levelDat);
  }

  function selectable(row: Row): boolean {
    // Already enabled: nothing to do. Orphaned: the file is gone — repair
    // lives on the library row, not here. No usable level.dat (D2) or
    // unknown: acting would be a refused write, or a guess with a level.dat
    // write attached. Ignored: the game skips the entry; adding cannot fix it
    // (§2 N.6). Skipped: this version skips the pack itself, so no world is
    // offered.
    if (skipped !== null) return false;
    return (
      blockedKey(row) === null &&
      !isUnknown(row) &&
      row.state !== 'enabled' &&
      row.state !== 'orphaned' &&
      row.state !== 'ignored'
    );
  }

  function stateNote(row: Row): string | null {
    if (isUnknown(row)) return $t('addons.datapacks.stateUnknown');
    switch (row.state) {
      case 'enabled':
        return $t('worlds.datapacks.stateEnabled');
      case 'disabled':
        return $t('worlds.datapacks.stateDisabled');
      case 'orphaned':
        return $t('worlds.datapacks.stateOrphaned');
      case 'ignored':
        return $t(ignoredLabelKey(row.ignoredReason));
      default:
        return null;
    }
  }

  // False once this picker is torn down. A plain flag, written in the
  // teardown and read only after an `await`, so no reactive read is involved.
  let mounted = true;
  $effect(() => () => {
    mounted = false;
  });

  // The props are read BEFORE the first `await`: the owner can tear the
  // picker down while an add runs (an instance switch closes it, and so does
  // its close button), and a torn-down picker's props point at nothing.
  // Re-read inside the loop they throw half-way, or aim the remaining worlds
  // at the instance the user switched to. The adds finish on the instance
  // they started on. The owner is always told to refresh, but it is asked to
  // close only a picker that is still open: once this one is gone, the owner
  // may have opened another dialog since, and its close handler would close
  // that one.
  async function apply() {
    if (ticked.size === 0 || rows === null) return;
    const id = instanceId;
    const file = filename;
    const done = { onApplied, onClose };
    const chosen = rows.filter((row) => ticked.has(row.world));
    busy = true;
    try {
      let ok = 0;
      const failed: string[] = [];
      for (const row of chosen) {
        // Present-but-disabled ⟹ the minimal honest edit is the toggle;
        // everything else ticked here is a genuine add.
        const res =
          row.state === 'disabled'
            ? await commands.datapacksSetEnabledInWorld(id, row.world, file, true)
            : await commands.datapacksAddToWorld(id, row.world, file);
        if (res.status === 'ok') ok += 1;
        else failed.push(`${row.world}: ${formatError(res.error)}`);
      }
      if (ok > 0) pushSuccess(get(t)('addons.datapacks.picker.toastAdded', { count: ok }));
      if (failed.length > 0)
        pushWarning(
          get(t)('addons.datapacks.picker.toastFailed', { count: failed.length }),
          failed,
        );
      if (ok > 0) done.onApplied();
      if (mounted) done.onClose();
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

    {#if skipped}
      <p class="text-sm text-warning-text" data-testid="datapack-picker-wont-load">
        {$t(skipped.key, skipped.args)}
      </p>
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
          {@const blocked = blockedKey(row)}
          <!-- The reason goes UNDER the name, in the name's own column: beside
               it, a long reason squeezed the name to one character and two
               only-old worlds read as the same "(" row. The name wraps and is
               never cut. Checkbox + label + note is the removal dialog's
               cascade-checkbox layout. -->
          <label
            class="flex items-start gap-2 rounded border border-border-subtle px-2 py-1.5 text-sm
              {selectable(row) ? '' : 'opacity-70'}"
          >
            <input
              type="checkbox"
              class="mt-0.5"
              data-testid="datapack-picker-world"
              data-world={row.world}
              data-level-dat={row.levelDat ?? 'unknown'}
              checked={row.state === 'enabled' || ticked.has(row.world)}
              disabled={!selectable(row) || busy}
              onchange={(e) => {
                if ((e.currentTarget as HTMLInputElement).checked) ticked.add(row.world);
                else ticked.delete(row.world);
              }}
            />
            <span class="flex-1 min-w-0">
              <span class="block break-words text-primary">{row.world}</span>
              {#if blocked !== null}
                <span class="block text-xs text-warning-text" data-testid="datapack-picker-blocked"
                  >{$t(blocked)}</span
                >
              {/if}
            </span>
            {#if blocked === null && note}
              <StatusBadge
                variant={row.state === 'enabled'
                  ? 'success'
                  : row.state === 'orphaned'
                    ? 'neutral'
                    : row.state === 'disabled'
                      ? 'muted'
                      : row.state === 'ignored'
                        ? 'warning'
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
