<script module lang="ts">
  import type { DatapackPlacementView } from '$lib/ipc/bindings';
  /** One pack of a this-world removal; a ghost (its file is gone) is `missing` without asking. */
  export type ThisWorldEntry = { filename: string; name: string; ghost: boolean };
  /** One pack of a library removal, with the worlds the library listing says reference it. */
  export type LibraryEntry = {
    filename: string;
    name: string;
    inLibrary: boolean;
    placements: DatapackPlacementView[];
  };
  export type BulkRemoveMode =
    | { kind: 'this-world'; world: string; entries: ThisWorldEntry[] }
    | { kind: 'library'; entries: LibraryEntry[] };
</script>

<script lang="ts">
  // N packs out of ONE world, or out of the library, with the single dialog's rules
  // (DatapackRemoveDialog, spec 2026-09-24 §4 U1; slice-2 design §7.6):
  //   * this-world: the backend is asked what each entry IS first — only the library's own copy
  //     survives a removal, anything else is the only copy and is deleted permanently. A ghost is
  //     `missing` by definition. An IPC error or a thrown invoke is "couldn't check", worded as a
  //     deletion, never a guessed kind; Confirm waits for every verdict.
  //   * library: ONE cascade answer for the batch («Also remove from worlds», checked); the worlds
  //     the packs are used in are listed once, distinct; the ones Lucerna could not check apart;
  //     packs no longer in the library are listed apart and always removed from their worlds
  //     (per tried world — a folder with no level.dat or only level.dat_old is never tried).
  // Every prop is read before the first `await` (the owner may tear the dialog down mid-run); the
  // run reports once, and `onRunning` tells the owner when it holds the files.
  import { commands, type WorldEntryKind } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { type ReportLine, reasonLines } from '$lib/format/reason-lines';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { bulkNameLines, runBulk } from '$lib/ui/bulk-run';
  import { levelDatBlockedKey } from '$lib/worlds/datapacks-gating';
  import { splitPlacements } from './datapack-remove-model';

  let {
    instanceId,
    mode,
    onClose,
    onRemoved,
    onRunning = () => {},
  }: {
    instanceId: string;
    mode: BulkRemoveMode;
    onClose: () => void;
    /** Called after the removal ran (fully or partially) so the owner refreshes. */
    onRemoved: () => void;
    /** `true` before the first command, `false` when the run is over — the owner's busy. */
    onRunning?: (running: boolean) => void;
  } = $props();

  type Verdict = WorldEntryKind['kind'] | 'unchecked';
  /** `null` while the backend is being asked (this-world only). */
  let verdicts = $state<Map<string, Verdict> | null>(null);
  let cascade = $state(true);
  let busy = $state(false);

  $effect(() => {
    if (mode.kind !== 'this-world') return;
    const reqInstance = instanceId;
    const reqWorld = mode.world;
    const entries = mode.entries;
    verdicts = null;
    let cancelled = false;
    void (async () => {
      const out = new Map<string, Verdict>();
      await Promise.all(
        entries.map(async (e) => {
          if (e.ghost) {
            out.set(e.filename, 'missing');
            return;
          }
          try {
            const res = await commands.datapacksWorldEntryKind(reqInstance, reqWorld, e.filename);
            out.set(e.filename, res.status === 'ok' ? res.data.kind : 'unchecked');
          } catch {
            // A thrown invoke is "could not tell" as well — worded, never guessed.
            out.set(e.filename, 'unchecked');
          }
        }),
      );
      if (
        cancelled ||
        instanceId !== reqInstance ||
        mode.kind !== 'this-world' ||
        mode.world !== reqWorld
      )
        return;
      verdicts = out;
    })();
    return () => {
      cancelled = true;
    };
  });

  const counts = $derived.by(() => {
    const c = { libraryCopy: 0, own: 0, missing: 0, unchecked: 0 };
    for (const v of verdicts?.values() ?? []) {
      if (v === 'library_copy') c.libraryCopy += 1;
      else if (v === 'missing') c.missing += 1;
      else if (v === 'unchecked') c.unchecked += 1;
      else c.own += 1;
    }
    return c;
  });
  // Only the library's copy LEAVES the world; a missing file deletes nothing either. Everything
  // else — including "couldn't check" — is a deletion. Until every verdict is in, the button reads
  // as the deletion it may be (the single dialog's rule).
  const deletes = $derived(verdicts === null || counts.own + counts.unchecked > 0);

  /** A world Lucerna leaves as it is, with why (D2), the way the single dialog lists it. */
  function unchangedLine(pack: string, p: DatapackPlacementView): string {
    const why = levelDatBlockedKey(p.level_dat);
    return why === null ? `${pack}: ${p.world}` : `${pack}: ${p.world} — ${get(t)(why)}`;
  }

  const lib = $derived.by(() => {
    if (mode.kind !== 'library') return null;
    const inLib = mode.entries.filter((e) => e.inLibrary);
    const worldsOnly = mode.entries.filter((e) => !e.inLibrary);
    const affected = new Set<string>();
    const unchecked = new Set<string>();
    let anyOnlyOld = false;
    for (const e of inLib) {
      const s = splitPlacements('library', e.placements);
      for (const p of s.affected) affected.add(p.world);
      for (const p of s.unchecked) unchecked.add(p.world);
      anyOnlyOld = anyOnlyOld || s.anyOnlyOld;
    }
    // A pack no longer in the library goes world by world through the world writers, which never
    // change a folder with no level.dat or only level.dat_old: those worlds keep it, and the
    // dialog says so before anything runs.
    const unchangedLines = worldsOnly.flatMap((e) =>
      splitPlacements('worlds-only', e.placements).unchanged.map((p) => unchangedLine(e.name, p)),
    );
    // Nothing in the library, and no world any of the packs is in can be changed: there is nothing
    // to remove, so Confirm is off (the single dialog's `nothingToRemove`).
    const nothingToRemove =
      inLib.length === 0 &&
      worldsOnly.every((e) => splitPlacements('worlds-only', e.placements).tried.length === 0);
    return {
      inLib,
      worldsOnly,
      affected: [...affected],
      unchecked: [...unchecked],
      anyOnlyOld,
      unchangedLines,
      nothingToRemove,
    };
  });

  const count = $derived(mode.entries.length);
  const names = $derived(bulkNameLines(mode.entries.map((e) => e.name)));
  const title = $derived(
    mode.kind === 'this-world'
      ? $t('addons.datapacks.remove.manyTitleThisWorld', { count })
      : $t('addons.datapacks.remove.manyTitle', { count }),
  );
  const confirmLabel = $derived(
    mode.kind !== 'this-world'
      ? $t('addons.datapacks.remove.confirm')
      : deletes
        ? $t('addons.datapacks.remove.confirmDelete')
        : $t('addons.datapacks.remove.confirmFromWorld'),
  );

  // False once this dialog is torn down; read only after an `await`.
  let mounted = true;
  $effect(() => () => {
    mounted = false;
  });

  async function removeFromWorld(to: string, world: string, entries: ThisWorldEntry[]) {
    const outcome = await runBulk(
      entries,
      (e) => commands.datapacksRemoveFromWorld(to, world, e.filename),
      formatError,
      (e) => e.name,
    );
    const values = { ok: outcome.ok, total: outcome.total, failed: outcome.failed };
    if (outcome.failed === 0) pushSuccess(get(t)('worlds.datapacks.bulkRemoved', values));
    else pushWarning(get(t)('worlds.datapacks.bulkRemovedFailed', values), outcome.reasons);
  }

  // A thrown invoke is that pack's (or world's) failure, worded by its message — never the end of
  // the run, which would leave the packs already removed unreported and the owner's list stale.
  function thrownReason(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  async function removeFromLibrary(to: string, entries: LibraryEntry[], withCascade: boolean) {
    const failures: { name: string; reason: string }[] = [];
    const kept: string[] = [];
    const left: string[] = [];
    let ok = 0;
    for (const e of entries) {
      if (e.inLibrary) {
        let res: Awaited<ReturnType<typeof commands.datapacksRemoveFromLibrary>>;
        try {
          res = await commands.datapacksRemoveFromLibrary(to, e.filename, withCascade);
        } catch (err) {
          failures.push({ name: e.name, reason: thrownReason(err) });
          continue;
        }
        if (res.status !== 'ok') {
          failures.push({ name: e.name, reason: formatError(res.error) });
          continue;
        }
        let packFailed = false;
        for (const w of res.data.worlds) {
          if (w.kind === 'failed') {
            packFailed = true;
            failures.push({ name: `${e.name}: ${w.world}`, reason: formatError(w.error) });
          } else if (w.kind === 'kept_not_ours' && withCascade) {
            kept.push(`${e.name}: ${w.world}`);
          }
        }
        // The backend keeps the library copy whenever a world could not be cleaned, and says so
        // with a failed world today; a kept copy with none is still not a removal.
        if (!res.data.removed_from_library && !packFailed) {
          packFailed = true;
          failures.push({
            name: e.name,
            reason: get(t)('addons.datapacks.remove.keptInLibrary'),
          });
        }
        if (!packFailed) ok += 1;
      } else {
        // No library copy is left: the only thing a removal can do is clear the tried worlds. A
        // world it never tries (D2) still holds the pack, so the pack is not removed — said with
        // the world and why, never counted as done.
        const split = splitPlacements('worlds-only', e.placements);
        let packFailed = split.unchanged.length > 0;
        for (const p of split.unchanged) left.push(unchangedLine(e.name, p));
        for (const p of split.tried) {
          let res: Awaited<ReturnType<typeof commands.datapacksRemoveFromWorld>>;
          try {
            res = await commands.datapacksRemoveFromWorld(to, p.world, e.filename);
          } catch (err) {
            packFailed = true;
            failures.push({ name: `${e.name}: ${p.world}`, reason: thrownReason(err) });
            continue;
          }
          if (res.status !== 'ok') {
            packFailed = true;
            failures.push({ name: `${e.name}: ${p.world}`, reason: formatError(res.error) });
          }
        }
        if (!packFailed) ok += 1;
      }
    }
    const total = entries.length;
    const values = { ok, total, failed: total - ok };
    const lines: ReportLine[] = [
      ...(failures.length > 0
        ? [get(t)('addons.datapacks.remove.manyFailedWorlds'), ...reasonLines(failures)]
        : []),
      ...(kept.length > 0 ? [get(t)('addons.datapacks.remove.manyKeptNotOurs'), ...kept] : []),
      ...(left.length > 0 ? [get(t)('addons.datapacks.remove.manyLeftUnchanged'), ...left] : []),
    ];
    if (values.failed > 0) pushWarning(get(t)('ui.bulk.removedFailed', values), lines);
    else if (lines.length > 0) pushWarning(get(t)('ui.bulk.removed', values), lines);
    else pushSuccess(get(t)('ui.bulk.removed', values));
  }

  async function confirm() {
    const to = instanceId;
    const m = mode;
    const withCascade = cascade;
    const done = { onRemoved, onClose, onRunning };
    busy = true;
    done.onRunning(true);
    try {
      if (m.kind === 'this-world') await removeFromWorld(to, m.world, m.entries);
      else await removeFromLibrary(to, m.entries, withCascade);
      done.onRemoved();
      if (mounted) done.onClose();
    } finally {
      busy = false;
      done.onRunning(false);
    }
  }
</script>

<ConfirmDialog
  {title}
  {confirmLabel}
  variant="danger"
  {busy}
  confirmDisabled={(mode.kind === 'this-world' && verdicts === null) ||
    (lib?.nothingToRemove ?? false)}
  confirmTestid="datapack-bulk-remove-confirm"
  panelClass="w-[520px] max-w-full p-5 flex flex-col gap-3"
  onCancel={onClose}
  onConfirm={() => void confirm()}
>
  {#snippet body()}
    <div class="flex flex-col gap-3" data-testid="datapack-bulk-remove-dialog">
      <!-- What is removed comes first, under the title; what it touches follows. -->
      <ul
        class="list-disc list-inside text-sm text-primary"
        data-testid="datapack-bulk-remove-names"
      >
        {#each names as line, i (i)}<li class="break-words">{line}</li>{/each}
      </ul>
      {#if mode.kind === 'this-world'}
        {#if verdicts === null}
          <div
            class="flex justify-center py-3 text-secondary"
            data-testid="datapack-bulk-remove-checking"
          >
            <Spinner labelPlacement="below" label={$t('common.loading')} />
          </div>
        {:else}
          <div
            class="flex flex-col gap-1 text-sm text-secondary"
            data-testid="datapack-bulk-remove-body"
          >
            {#if counts.libraryCopy > 0}
              <p>
                {$t('addons.datapacks.remove.manyLibraryCopies', { count: counts.libraryCopy })}
              </p>
            {/if}
            {#if counts.own > 0}
              <p>{$t('addons.datapacks.remove.manyOwnFiles', { count: counts.own })}</p>
            {/if}
            {#if counts.missing > 0}
              <p>{$t('addons.datapacks.remove.manyMissing', { count: counts.missing })}</p>
            {/if}
            {#if counts.unchecked > 0}
              <p>{$t('addons.datapacks.remove.manyUnchecked', { count: counts.unchecked })}</p>
            {/if}
          </div>
        {/if}
      {:else if lib}
        {#if lib.inLib.length > 0}
          {#if lib.affected.length > 0}
            <div class="text-sm text-secondary" data-testid="datapack-bulk-remove-affected">
              <p>{$t('addons.datapacks.remove.manyUsedIn', { count: lib.affected.length })}</p>
              <ul class="mt-1 list-disc list-inside text-primary">
                {#each lib.affected as w (w)}<li class="break-words">{w}</li>{/each}
              </ul>
            </div>
          {:else if lib.worldsOnly.length === 0}
            <p class="text-sm text-secondary">{$t('addons.datapacks.remove.manyNoWorlds')}</p>
          {/if}
          {#if lib.unchecked.length > 0}
            <!-- Could not tell is its own list, never an "affected" world and never dropped. -->
            <div class="text-sm text-secondary" data-testid="datapack-bulk-remove-unchecked">
              <p>
                {$t('addons.datapacks.remove.uncheckedWorlds', { count: lib.unchecked.length })}
              </p>
              <ul class="mt-1 list-disc list-inside text-primary">
                {#each lib.unchecked as w (w)}<li class="break-words">{w}</li>{/each}
              </ul>
              {#if cascade}
                <p class="mt-1 text-xs text-muted">
                  {$t('addons.datapacks.remove.uncheckedKeepsLibrary')}
                </p>
              {/if}
            </div>
          {/if}
          {#if cascade && lib.anyOnlyOld}
            <p class="text-xs text-muted">{$t('addons.datapacks.remove.onlyOldKeepsLibrary')}</p>
          {/if}
          <label class="flex items-start gap-2 text-sm text-primary">
            <input
              type="checkbox"
              bind:checked={cascade}
              disabled={busy}
              class="mt-0.5"
              data-testid="datapack-bulk-remove-cascade"
            />
            <span>
              {$t('addons.datapacks.remove.cascade')}
              <span class="block text-xs text-muted">{$t('addons.datapacks.remove.keepNote')}</span>
            </span>
          </label>
        {/if}
        {#if lib.unchangedLines.length > 0}
          <div class="text-sm text-secondary" data-testid="datapack-bulk-remove-unchanged">
            <p>
              {$t('addons.datapacks.remove.unchangedWorlds', {
                count: lib.unchangedLines.length,
              })}
            </p>
            <!-- Wraps, never an ellipsis: a cut mark read "…from the backu…". -->
            <ul class="mt-1 list-disc list-inside text-primary">
              {#each lib.unchangedLines as line, i (i)}<li class="break-words">{line}</li>{/each}
            </ul>
          </div>
        {/if}
        {#if lib.nothingToRemove}
          <!-- Every listed world is one Lucerna won't change: no deletion to warn about, and this
               line is why Confirm is off. -->
          <p class="text-xs text-muted" data-testid="datapack-bulk-remove-nothing">
            {$t('addons.datapacks.remove.worldsOnlyNothing')}
          </p>
        {:else if lib.worldsOnly.length > 0}
          <p class="text-xs text-muted" data-testid="datapack-bulk-remove-worlds-only">
            {$t('addons.datapacks.remove.manyWorldsOnlyNote', { count: lib.worldsOnly.length })}
          </p>
        {/if}
      {/if}
    </div>
  {/snippet}
</ConfirmDialog>
