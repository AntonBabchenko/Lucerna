<script lang="ts">
  // Rename an instance's on-disk folder.
  //
  // Deliberately a dialog with an explicit confirm, NOT the commit-on-blur
  // pattern the display-name field above it uses. Renaming moves a directory and
  // changes the instance's identity (the folder name IS the id), so a stray blur
  // must never trigger it.
  //
  // The slug preview comes from the backend (`previewInstanceDirName`) rather
  // than a TypeScript reimplementation of the rules — duplicating them here
  // would drift from `naming::derive_base` the first time either side changed.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import type { InstanceWithStatus, IpcError } from '$lib/ipc/bindings';
  import { debounceTrailing } from '$lib/ui/debounce';
  import { onDestroy } from 'svelte';

  let {
    instanceId,
    initialValue,
    formatIpcError,
    onCancel,
    onRenamed,
  }: {
    instanceId: string;
    /** Seed for the input: the current folder for a plain rename, the suggested
     *  ASCII name when opened from the "this will not launch" banner. */
    initialValue: string;
    /** Reuses the parent modal's IPC error formatting so wording stays uniform. */
    formatIpcError: (e: IpcError) => string;
    onCancel: () => void;
    onRenamed: (updated: InstanceWithStatus) => void;
  } = $props();

  let value = $state(initialValue);
  let preview = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);

  // Guards against an out-of-order preview response overwriting a newer one.
  let previewSeq = 0;

  async function refreshPreview() {
    const seq = ++previewSeq;
    const next = await commands.previewInstanceDirName(value);
    if (seq !== previewSeq) return;
    preview = next;
  }

  const previewDebounced = debounceTrailing(() => void refreshPreview(), 250);
  onDestroy(() => previewDebounced.cancel());

  // Seed the preview once on open, then debounce every keystroke.
  void refreshPreview();
  $effect(() => {
    // Touch `value` so the effect re-runs per keystroke.
    void value;
    previewDebounced.call();
  });

  const canSubmit = $derived(preview.length > 0 && !busy);

  async function submit() {
    if (!canSubmit) return;
    busy = true;
    error = null;
    const result = await commands.renameInstanceDir(instanceId, value);
    busy = false;
    if (result.status === 'ok') {
      onRenamed(result.data);
    } else {
      error = formatIpcError(result.error);
    }
  }
</script>

<Modal
  ariaLabelledby="rename-folder-title"
  onClose={onCancel}
  panelClass="w-[480px] p-5 flex flex-col gap-3"
>
  <h3 id="rename-folder-title" class="font-semibold text-primary text-base">
    {$t('instance.manage.folderRenameTitle')}
  </h3>

  <form
    class="flex flex-col gap-3"
    onsubmit={(e) => {
      e.preventDefault();
      void submit();
    }}
  >
    <div class="flex flex-col gap-1">
      <label class="text-xs text-secondary" for="rename-folder-input">
        {$t('instance.manage.folderRenameLabel')}
      </label>
      <input
        id="rename-folder-input"
        data-autofocus
        class="border rounded px-2 py-1 text-sm w-full"
        bind:value
        autocomplete="off"
        spellcheck="false"
      />
      <p class="text-xs text-muted">{$t('instance.manage.folderRenameHint')}</p>
      {#if preview}
        <p class="text-xs text-secondary" role="status">
          {$t('instance.manage.folderRenamePreview', { name: preview })}
        </p>
      {:else}
        <p class="text-xs text-danger" role="status">
          {$t('instance.manage.folderRenameUnusable')}
        </p>
      {/if}
    </div>

    <!-- Always shown: shortcuts live on the user's desktop and we never record
         creating them, so we genuinely cannot tell whether any exist. -->
    <p class="text-xs text-muted">
      {$t('instance.manage.folderRenameShortcutWarning')}
    </p>

    {#if error}
      <p class="text-sm text-danger" role="alert">{error}</p>
    {/if}

    <div class="flex justify-end gap-2 mt-2">
      <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
        {$t('common.cancel')}
      </button>
      <button type="submit" class="btn-primary btn-sm" disabled={!canSubmit}>
        {$t('instance.manage.folderRenameConfirm')}
      </button>
    </div>
  </form>
</Modal>
