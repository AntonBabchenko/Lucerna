<script lang="ts">
  import type { OrphanRef } from '$lib/ipc/bindings';

  let { removingNames, orphans, onCancel, onConfirm }: {
    removingNames: string[];
    orphans: OrphanRef[];
    onCancel: () => void;
    onConfirm: (alsoRemoveShas: string[]) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let checked = $state<boolean[]>(orphans.map(() => true));
  function confirm() {
    onConfirm(orphans.filter((_, i) => checked[i]).map((o) => o.sha1));
  }
</script>

<div class="fixed inset-0 z-40 flex items-center justify-center bg-black/40">
  <div role="dialog" aria-modal="true" aria-label="Confirm uninstall" class="bg-surface rounded shadow-xl w-[440px] max-w-[90vw] p-5">
    <h2 class="text-base font-semibold text-primary mb-3">Uninstall {removingNames.length} mod{removingNames.length === 1 ? '' : 's'}?</h2>
    <ul class="text-sm text-secondary list-disc pl-5 mb-3 max-h-32 overflow-auto">
      {#each removingNames as n}<li>{n}</li>{/each}
    </ul>
    {#if orphans.length > 0}
      <div class="text-xs uppercase tracking-wide text-muted mb-1">These dependencies will no longer be needed</div>
      <ul class="text-sm text-primary space-y-1 mb-3">
        {#each orphans as o, i (o.sha1)}
          <li><label class="inline-flex items-center gap-2">
            <input type="checkbox" checked={checked[i]} onchange={(e) => (checked[i] = (e.currentTarget as HTMLInputElement).checked)} />
            {o.name}
          </label></li>
        {/each}
      </ul>
    {/if}
    <div class="flex justify-end gap-2 mt-4">
      <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>Cancel</button>
      <button type="button" class="btn-danger btn-sm" onclick={confirm}>Uninstall</button>
    </div>
  </div>
</div>
