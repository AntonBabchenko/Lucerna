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

  // Confirm by typing the literal word "Delete" rather than the folder
  // name — players name their worlds anything (long, unicode, emoji,
  // accidental whitespace) and re-typing it as a safety gate becomes
  // user-hostile.
  const CONFIRM_WORD = 'Delete';
  const canDelete = $derived(typed === CONFIRM_WORD && !busy);

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
  <div class="bg-surface border border-border-subtle rounded shadow-lg max-w-md w-full p-4">
    <h3 id="delete-world-title" class="font-semibold text-lg text-primary mb-2">
      Delete "{world.folder_name}"?
    </h3>
    <p class="text-sm text-secondary mb-3">
      This will permanently delete the world folder and all its contents. The backups for this world
      will also be removed. This cannot be undone.
    </p>
    <label class="block text-xs text-secondary mb-1" for="del-world-confirm">
      Type <span class="font-mono font-semibold">{CONFIRM_WORD}</span> to confirm:
    </label>
    <input
      id="del-world-confirm"
      class="border rounded px-2 py-1 w-full mb-3"
      bind:value={typed}
      disabled={busy}
      placeholder={CONFIRM_WORD}
      autocomplete="off"
    />
    {#if error}
      <p class="text-xs text-danger mb-2">{error}</p>
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
        class="bg-danger text-white rounded px-3 py-1 text-sm hover:bg-danger disabled:bg-muted"
        disabled={!canDelete}
        onclick={() => void onConfirm()}
      >
        Delete
      </button>
    </div>
  </div>
</div>
