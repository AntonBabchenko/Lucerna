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
  import { levelDatBlockedKey } from '$lib/worlds/datapacks-gating';

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
  // name, which skips (and lists apart) the worlds every world writer refuses
  // (D2). `this-world` — one pack out of ONE world, from the world tab's
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
  // it: the cascade cannot compare it with the library copy either. The same
  // mark with no state is a placement Lucerna could not check at all (the
  // world's datapacks/ unreadable, or R2 could not tell), in any folder — the
  // one predicate covers both.
  function isUnchecked(p: DatapackPlacementView): boolean {
    return (p.state === null && p.level_dat !== 'absent') || p.ignored_reason === 'unreadable';
  }
  // D2: every world writer refuses a folder with no level.dat or only
  // level.dat_old. A worlds-only removal goes world by world through those
  // writers, so such worlds are listed apart as left unchanged and never
  // tried. The library cascade differs: it unlinks the file in a folder with
  // no level file (A3) and fails an only-old world, which the only-old note
  // says up front.
  const unchanged = $derived(
    mode.kind === 'worlds-only'
      ? placements.filter((p) => p.level_dat === 'absent' || p.level_dat === 'only_old')
      : [],
  );
  const tried = $derived(placements.filter((p) => !unchanged.includes(p)));
  const affected = $derived(tried.filter((p) => !isUnchecked(p)));
  const unchecked = $derived(tried.filter(isUnchecked));
  // D2: the cascade refuses a world with only level.dat_old and reports it
  // Failed, which keeps the library copy. Said up front, not only in the toast.
  const anyOnlyOld = $derived(placements.some((p) => p.level_dat === 'only_old'));

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
    // Set when the dialog closes (or its target changes) mid-check: the
    // answer is dropped before any prop is read, since a closed dialog's
    // props may already point at nothing.
    let cancelled = false;
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
        cancelled ||
        instanceId !== reqInstance ||
        filename !== reqFile ||
        mode.kind !== 'this-world' ||
        mode.world !== reqWorld
      )
        return;
      verdict = next;
    })();
    return () => {
      cancelled = true;
    };
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

  /** What a confirm acts on, read once before its first `await` (see `confirm`). */
  type Target = { instanceId: string; filename: string; packName: string };

  async function confirmThisWorld(to: Target, world: string) {
    const res = await commands.datapacksRemoveFromWorld(to.instanceId, world, to.filename);
    if (res.status !== 'ok') {
      pushWarning(get(t)('addons.datapacks.remove.toastFailed', { name: to.packName }), [
        formatError(res.error),
      ]);
      return;
    }
    pushSuccess(get(t)('addons.datapacks.remove.toastRemoved', { name: to.packName }));
  }

  /** A world left as it is, with why, the way the dialog listed it. */
  function unchangedLine(p: DatapackPlacementView): string {
    const why = levelDatBlockedKey(p.level_dat);
    return why === null ? p.world : `${p.world} — ${get(t)(why)}`;
  }

  async function confirmWorldsOnly(
    to: Target,
    worlds: DatapackPlacementView[],
    left: DatapackPlacementView[],
  ) {
    let removed = 0;
    const failed: string[] = [];
    for (const p of worlds) {
      const res = await commands.datapacksRemoveFromWorld(to.instanceId, p.world, to.filename);
      if (res.status === 'ok') removed += 1;
      else failed.push(`${p.world}: ${formatError(res.error)}`);
    }
    if (failed.length > 0) {
      // Not `toastFailedWorlds`: that one says the library copy was kept, and
      // a worlds-only row has none.
      pushWarning(
        get(t)('addons.datapacks.remove.toastFailedWorldsOnly', { count: failed.length }),
        failed,
      );
    } else if (left.length > 0) {
      // D2: the worlds listed as won't-be-changed were never tried and still
      // hold the pack. A plain "Removed" would claim they were cleaned too.
      pushWarning(
        get(t)('addons.datapacks.remove.toastRemovedSomeUnchanged', { name: to.packName }),
        left.map(unchangedLine),
      );
    } else if (removed > 0) {
      pushSuccess(get(t)('addons.datapacks.remove.toastRemoved', { name: to.packName }));
    }
  }

  async function confirmFromLibrary(to: Target, cascade: boolean) {
    const res = await commands.datapacksRemoveFromLibrary(to.instanceId, to.filename, cascade);
    if (res.status !== 'ok') {
      pushWarning(get(t)('addons.datapacks.remove.toastFailed', { name: to.packName }), [
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
      pushSuccess(get(t)('addons.datapacks.remove.toastRemoved', { name: to.packName }));
    }
  }

  // Every prop, and everything derived from one, is read BEFORE the first
  // `await`. The owner can tear this dialog down while a removal runs (an
  // instance switch closes it), and a torn-down dialog's props point at
  // nothing: re-read after an `await` they throw half-way through a loop, or
  // aim the rest of it at the instance the user switched to. The job finishes
  // on the instance it started on and reports the result against it.
  async function confirm() {
    const to: Target = { instanceId, filename, packName };
    const m = mode;
    const worlds = tried;
    const left = unchanged;
    const withCascade = cascade;
    const done = { onRemoved, onClose };
    busy = true;
    try {
      if (m.kind === 'this-world') await confirmThisWorld(to, m.world);
      else if (m.kind === 'library') await confirmFromLibrary(to, withCascade);
      else await confirmWorldsOnly(to, worlds, left);
      done.onRemoved();
      done.onClose();
    } finally {
      busy = false;
    }
  }
</script>

<!-- One listed world, with why Lucerna leaves it alone when its level.dat
     rules a change out (D2): no level.dat, or only level.dat_old. The library
     cascade does change a folder with no level file — it unlinks the file
     there (§0.5 A3) — so in that mode such a folder carries no mark. -->
{#snippet worldItem(p: DatapackPlacementView)}
  {@const blocked =
    mode.kind === 'library' && p.level_dat === 'absent' ? null : levelDatBlockedKey(p.level_dat)}
  <li class="truncate">
    {p.world}
    {#if blocked !== null}
      <span class="text-xs text-muted" data-testid="datapack-remove-world-blocked"
        >— {$t(blocked)}</span
      >
    {/if}
  </li>
{/snippet}

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
              {@render worldItem(p)}
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
              {@render worldItem(p)}
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
      {#if unchanged.length > 0}
        <div class="text-sm text-secondary" data-testid="datapack-remove-unchanged">
          <p>{$t('addons.datapacks.remove.unchangedWorlds', { count: unchanged.length })}</p>
          <ul class="mt-1 list-disc list-inside text-primary">
            {#each unchanged as p (p.world)}
              {@render worldItem(p)}
            {/each}
          </ul>
        </div>
      {/if}
      {#if mode.kind === 'library' && cascade && anyOnlyOld}
        <p class="text-xs text-muted" data-testid="datapack-remove-only-old-note">
          {$t('addons.datapacks.remove.onlyOldKeepsLibrary')}
        </p>
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
        disabled={(mode.kind === 'this-world' && verdict === null) ||
          (mode.kind === 'worlds-only' && tried.length === 0)}
        onclick={confirm}
        data-testid="datapack-remove-confirm"
      >
        {confirmLabel}
      </BusyButton>
    </div>
  </div>
</Modal>
