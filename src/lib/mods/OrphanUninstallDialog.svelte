<script lang="ts">
  import type { OrphanRef } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';

  let {
    removingNames,
    orphans,
    onCancel,
    onConfirm,
  }: {
    removingNames: string[];
    orphans: OrphanRef[];
    onCancel: () => void;
    onConfirm: (alsoRemoveShas: string[]) => void;
  } = $props();

  // Default to UNCHECKED: removing a dependency is a destructive *secondary*
  // action, and the edge model can't tell "only ever a dependency" from "the
  // user also installed this directly". Opt-in (apt-style autoremove), so a
  // single Uninstall click never deletes a mod the user may have wanted.
  // svelte-ignore state_referenced_locally
  let checked = $state<boolean[]>(orphans.map(() => false));
  const anyChecked = $derived(checked.some(Boolean));
  function confirm() {
    onConfirm(orphans.filter((_, i) => checked[i]).map((o) => o.sha1));
  }
</script>

<Modal
  ariaLabelledby="orphan-dialog-title"
  onClose={onCancel}
  panelClass="w-[440px] max-w-[90vw] p-5"
>
  <h2 id="orphan-dialog-title" class="text-base font-semibold text-primary mb-3">
    {$t('mods.orphan.heading', { count: removingNames.length })}
  </h2>
  <ul class="text-sm text-secondary list-disc pl-5 mb-3 max-h-32 overflow-auto">
    {#each removingNames as n}<li>{n}</li>{/each}
  </ul>
  {#if orphans.length > 0}
    <div class="text-xs uppercase tracking-wide text-muted mb-1">
      {$t('mods.orphan.alsoRemoveLabel')}
    </div>
    <ul class="text-sm text-primary space-y-1 mb-3">
      {#each orphans as o, i (o.sha1)}
        <li>
          <label class="inline-flex items-center gap-2">
            <input
              type="checkbox"
              checked={checked[i]}
              onchange={(e) => (checked[i] = (e.currentTarget as HTMLInputElement).checked)}
            />
            {o.name}
          </label>
        </li>
      {/each}
    </ul>
  {/if}
  <div class="flex justify-end gap-2 mt-4">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}
      >{$t('common.cancel')}</button
    >
    <button type="button" class="btn-danger btn-sm" onclick={confirm}>
      {$t('mods.orphan.confirmBtn', {
        count: anyChecked
          ? removingNames.length + checked.filter(Boolean).length
          : removingNames.length,
      })}
    </button>
  </div>
</Modal>
