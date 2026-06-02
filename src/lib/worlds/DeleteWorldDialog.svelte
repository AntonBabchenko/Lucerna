<script lang="ts">
  import { commands, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';

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
      {$t('worlds.delete.title', { world: world.folder_name })}
    </h3>
    <p class="text-sm text-secondary mb-3">
      {$t('worlds.delete.description')}
    </p>
    <label class="block text-xs text-secondary mb-1" for="del-world-confirm">
      {$t('worlds.delete.typeToConfirm', { word: CONFIRM_WORD })}
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
      <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={busy}>
        {$t('common.cancel')}
      </button>
      <button
        type="button"
        class="btn-danger btn-sm"
        disabled={!canDelete}
        onclick={() => void onConfirm()}
      >
        {$t('worlds.delete.deleteBtn')}
      </button>
    </div>
  </div>
</div>
