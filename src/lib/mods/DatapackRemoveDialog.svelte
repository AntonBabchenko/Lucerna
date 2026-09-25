<script lang="ts">
  import { commands, type DatapackPlacementView, type WorldEntryKind } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { get } from 'svelte/store';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  // The ONE removal confirmation for datapacks, shared by the library screen
  // and the catalog card's trash action (slice-2 design §7.6) — the catalog
  // seam is the #4083 hazard from the other side: a one-click silent delete
  // that must cascade into every world holding the pack.
  //
  // «Убрать также из миров» is CHECKED by default. Unchecked leaves the world
  // links in place — the pack keeps loading in game, and the library listing
  // then shows it as "only in worlds" — which is a supported state, not an
  // error, so the note spells the consequence out instead of forbidding it.
  //
  // Three modes. `library` — the cascade removal from the library row.
  // `worlds-only` — a row whose library copy is gone: per-world removal by
  // name. `this-world` — one pack out of ONE world, from the world tab's
  // trash and the library sub-row's trash (spec 2026-09-24 §4 U1). It asks
  // the backend what the entry IS first: only the library's own copy
  // survives a removal, anything else is the only copy and is deleted
  // permanently (never the Recycle Bin; Linux/macOS builds exist). An IPC
  // error is its own honest copy ("couldn't check"), never a guessed kind,
  // and Confirm stays allowed then — nothing is written until Confirm.
  //
  // A placement whose state could not be read (its world's `datapacks/` or
  // pack lists were unreadable) is not an "affected world": Lucerna does not
  // know whether the pack is there. Those are listed apart as "couldn't be
  // checked". The cascade re-checks every world; one it still cannot check is
  // reported Failed and KEEPS the library copy (`LibraryRemoval`'s
  // `removed_from_library: false`), which the note says up front.
  let {
    instanceId,
    filename,
    packName,
    mode,
    onClose,
    onRemoved,
  }: {
    instanceId: string;
    filename: string;
    packName: string;
    /**
     * `library` / `worlds-only` — see the header comment; `placements` are the
     * worlds referencing the pack, from the library listing. `worlds-only` ⟹ a
     * row whose library copy is gone: the cascade compares world copies against
     * a library file that no longer exists, so every world would come back
     * `kept_not_ours`; the user confirmed this exact world list, so the honest
     * action is per-world removal by name. `this-world` — one world.
     */
    mode:
      | { kind: 'library' | 'worlds-only'; placements: DatapackPlacementView[] }
      | { kind: 'this-world'; world: string };
    onClose: () => void;
    /** Called after the removal ran (fully or partially) so the owner refreshes. */
    onRemoved: () => void;
  } = $props();

  let cascade = $state(true);
  let busy = $state(false);

  const placements = $derived(mode.kind === 'this-world' ? [] : mode.placements);

  // Unknown state in a folder that has a level file (or whose level file could
  // not be told): could not check. A folder with neither level file has no list
  // to hold a state (`absent`) — it is an affected world the cascade unlinks.
  // An `unreadable` entry is Lucerna failing to read it, not the game ignoring
  // it: the cascade cannot compare it with the library copy either.
  function isUnchecked(p: DatapackPlacementView): boolean {
    return (p.state === null && p.level_dat !== 'absent') || p.ignored_reason === 'unreadable';
  }
  const affected = $derived(placements.filter((p) => !isUnchecked(p)));
  const unchecked = $derived(placements.filter(isUnchecked));

  type Verdict = WorldEntryKind['kind'] | 'unchecked';
  const BODY: Record<Verdict, TranslationKey> = {
    library_copy: 'addons.datapacks.remove.bodyLibraryCopy',
    own_file: 'addons.datapacks.remove.bodyOwnFile',
    own_folder: 'addons.datapacks.remove.bodyOwnFolder',
    missing: 'addons.datapacks.remove.bodyMissing',
    unchecked: 'addons.datapacks.remove.bodyUnchecked',
  };
  /** `null` while the backend is being asked. */
  let verdict = $state<Verdict | null>(null);

  $effect(() => {
    if (mode.kind !== 'this-world') return;
    const reqInstance = instanceId;
    const reqWorld = mode.world;
    const reqFile = filename;
    verdict = null;
    void (async () => {
      let next: Verdict;
      try {
        const res = await commands.datapacksWorldEntryKind(reqInstance, reqWorld, reqFile);
        next = res.status === 'ok' ? res.data.kind : 'unchecked';
      } catch {
        // A thrown invoke is "could not tell" as well — worded, never guessed.
        next = 'unchecked';
      }
      if (
        instanceId !== reqInstance ||
        filename !== reqFile ||
        mode.kind !== 'this-world' ||
        mode.world !== reqWorld
      )
        return;
      verdict = next;
    })();
  });

  // Only the library's copy LEAVES the world; a missing file deletes nothing
  // either. Everything else — including "couldn't check" — is a deletion.
  const confirmLabel = $derived(
    mode.kind !== 'this-world'
      ? $t('addons.datapacks.remove.confirm')
      : verdict === 'library_copy' || verdict === 'missing'
        ? $t('addons.datapacks.remove.confirmFromWorld')
        : $t('addons.datapacks.remove.confirmDelete'),
  );

  async function confirmThisWorld(world: string) {
    const res = await commands.datapacksRemoveFromWorld(instanceId, world, filename);
    if (res.status !== 'ok') {
      pushWarning(get(t)('addons.datapacks.remove.toastFailed', { name: packName }), [
        formatError(res.error),
      ]);
      return;
    }
    pushSuccess(get(t)('addons.datapacks.remove.toastRemoved', { name: packName }));
  }

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
      if (mode.kind === 'this-world') await confirmThisWorld(mode.world);
      else if (mode.kind === 'library') await confirmFromLibrary();
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
        {mode.kind === 'this-world'
          ? $t('addons.datapacks.remove.titleThisWorld', { name: packName })
          : $t('addons.datapacks.remove.title', { name: packName })}
      </h2>
      <CloseButton onClick={onClose} ariaLabel={$t('common.close')} />
    </div>

    {#if mode.kind === 'this-world'}
      {#if verdict === null}
        <div class="flex justify-center py-3 text-secondary" data-testid="datapack-remove-checking">
          <Spinner labelPlacement="below" label={$t('common.loading')} />
        </div>
      {:else}
        <p class="text-sm text-secondary" data-testid="datapack-remove-body">
          {$t(BODY[verdict])}
        </p>
      {/if}
    {:else if placements.length > 0}
      {#if affected.length > 0}
        <div class="text-sm text-secondary" data-testid="datapack-remove-affected">
          <p>{$t('addons.datapacks.remove.affectedWorlds', { count: affected.length })}</p>
          <ul class="mt-1 list-disc list-inside text-primary">
            {#each affected as p (p.world)}
              <li class="truncate">{p.world}</li>
            {/each}
          </ul>
        </div>
      {/if}
      {#if unchecked.length > 0}
        <!-- Could not tell is its own list, never an "affected" world and never
             dropped (Fallback discipline Q2). -->
        <div class="text-sm text-secondary" data-testid="datapack-remove-unchecked">
          <p>{$t('addons.datapacks.remove.uncheckedWorlds', { count: unchecked.length })}</p>
          <ul class="mt-1 list-disc list-inside text-primary">
            {#each unchecked as p (p.world)}
              <li class="truncate">{p.world}</li>
            {/each}
          </ul>
          {#if mode.kind === 'library' && cascade}
            <p class="mt-1 text-xs text-muted">
              {$t('addons.datapacks.remove.uncheckedKeepsLibrary')}
            </p>
          {:else if mode.kind === 'worlds-only'}
            <p class="mt-1 text-xs text-muted">
              {$t('addons.datapacks.remove.uncheckedWorldsOnly')}
            </p>
          {/if}
        </div>
      {/if}
      {#if mode.kind === 'library'}
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
        disabled={mode.kind === 'this-world' && verdict === null}
        onclick={confirm}
        data-testid="datapack-remove-confirm"
      >
        {confirmLabel}
      </BusyButton>
    </div>
  </div>
</Modal>
