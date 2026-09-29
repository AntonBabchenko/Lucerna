<script lang="ts">
  import { t } from '$lib/i18n';
  import { displayVersion } from '$lib/format/version';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import { Icon } from '$lib/ui/icons';
  import ChangelogModal from '../ChangelogModal.svelte';
  import type { UpdateReviewItem } from './update-review';

  // The review before «Обновить все» (spec D9, §6.6): every pending update, all ticked; the user
  // unticks what stays. Dependencies are not predicted (D15) — the run reports them. A changelog
  // opens ON TOP: it renders after the dialog, and Modals stack by DOM order (DESIGN.md §8). While
  // the updates run, ConfirmDialog locks the dialog and parks focus on its body.
  let {
    items,
    busy = false,
    onCancel,
    onConfirm,
  }: {
    items: UpdateReviewItem[];
    busy?: boolean;
    onCancel: () => void;
    onConfirm: (sha1s: string[]) => void;
  } = $props();

  // svelte-ignore state_referenced_locally — the items (a snapshot) seed the ticks
  let checked = $state(new Set(items.map((i) => i.sha1)));
  let changelogFor = $state<UpdateReviewItem | null>(null);
  const count = $derived(items.filter((i) => checked.has(i.sha1)).length);

  function toggle(sha1: string, on: boolean) {
    const next = new Set(checked);
    if (on) next.add(sha1);
    else next.delete(sha1);
    checked = next;
  }
</script>

<ConfirmDialog
  title={$t('mods.updates.reviewTitle')}
  confirmLabel={$t('mods.updates.reviewConfirm', { count })}
  confirmDisabled={count === 0}
  confirmTestid="update-review-confirm"
  {busy}
  panelClass="w-[520px] max-w-[90vw] p-5 flex flex-col gap-3"
  {onCancel}
  onConfirm={() => onConfirm(items.filter((i) => checked.has(i.sha1)).map((i) => i.sha1))}
>
  {#snippet body()}
    <p class="text-sm text-secondary">{$t('mods.updates.reviewHint')}</p>
    <ul class="flex flex-col gap-1 max-h-80 overflow-y-auto" data-testid="update-review-list">
      {#each items as item (item.sha1)}
        <li class="flex items-center gap-2 text-sm">
          <label class="flex flex-1 min-w-0 items-center gap-2">
            <input
              type="checkbox"
              checked={checked.has(item.sha1)}
              disabled={busy}
              onchange={(e) => toggle(item.sha1, (e.currentTarget as HTMLInputElement).checked)}
            />
            <span class="truncate text-primary">{item.name}</span>
          </label>
          <span class="flex-shrink-0 inline-flex items-center gap-1 text-xs text-muted">
            {displayVersion(item.from ?? '?')}
            <Icon name="arrowRight" size={12} />
            {displayVersion(item.to)}
          </span>
          {#if item.changelog}
            <button
              type="button"
              class="btn-link text-xs flex-shrink-0"
              aria-label={$t('mods.updates.changelogFor', { name: item.name })}
              onclick={() => (changelogFor = item)}>{$t('mods.changelog.view')}</button
            >
          {/if}
        </li>
      {/each}
    </ul>
  {/snippet}
</ConfirmDialog>

{#if changelogFor?.changelog}
  <ChangelogModal
    source={changelogFor.changelog.source}
    projectId={changelogFor.changelog.projectId}
    title={`${changelogFor.name} ${changelogFor.from ?? ''} → ${changelogFor.to}`}
    targetVersionId={changelogFor.changelog.targetVersionId}
    baseVersionId={changelogFor.changelog.baseVersionId}
    onClose={() => (changelogFor = null)}
  />
{/if}
