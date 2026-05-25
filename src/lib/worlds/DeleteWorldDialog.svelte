<script lang="ts">
  import { commands, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';

  let {
    instanceId,
    world,
    onClose,
    onDeleted,
  }: {
    instanceId: string;
    world: World;
    onClose: () => void;
    onDeleted: () => void;
  } = $props();

  let typed = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  const canDelete = $derived(typed === world.folder_name && !busy);

  async function onConfirm() {
    busy = true;
    error = null;
    const r = await commands.deleteWorld(instanceId, world.folder_name);
    busy = false;
    if (r.status === 'ok') {
      onDeleted();
    } else {
      error = formatError(r.error);
    }
  }
</script>

<div
  class="fixed inset-0 z-50 bg-black/30 flex items-center justify-center"
  role="dialog"
  aria-modal="true"
  aria-labelledby="delete-world-title"
>
  <div class="bg-white border border-neutral-200 rounded shadow-lg max-w-md w-full p-4">
    <h3 id="delete-world-title" class="font-semibold text-lg mb-2">
      Delete "{world.folder_name}"?
    </h3>
    <p class="text-sm text-neutral-700 mb-3">
      This will permanently delete the world folder and all its contents. The backups for this world
      will also be removed. This cannot be undone.
    </p>
    <label class="block text-xs text-neutral-600 mb-1" for="del-world-confirm">
      Type the world's name to confirm:
    </label>
    <input
      id="del-world-confirm"
      class="border rounded px-2 py-1 w-full mb-3"
      bind:value={typed}
      disabled={busy}
    />
    {#if error}
      <p class="text-xs text-red-700 mb-2">{error}</p>
    {/if}
    <div class="flex justify-end gap-2">
      <button
        type="button"
        class="border rounded px-3 py-1 text-sm"
        onclick={onClose}
        disabled={busy}
      >
        Cancel
      </button>
      <button
        type="button"
        class="bg-red-600 text-white rounded px-3 py-1 text-sm hover:bg-red-700 disabled:bg-neutral-300"
        disabled={!canDelete}
        onclick={() => void onConfirm()}
      >
        Delete
      </button>
    </div>
  </div>
</div>
