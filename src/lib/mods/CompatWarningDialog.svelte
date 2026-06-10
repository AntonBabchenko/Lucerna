<script lang="ts">
  // Confirmation modal shown when one or more dropped jars look
  // incompatible with the active instance. Compatible jars are never in
  // question — the dialog appears only because of the mismatched ones.
  //
  // `onConfirm` = install every jar (compatible + mismatched).
  // `onCancel`  = install only the compatible jars; skip the mismatched.

  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';

  type MismatchRow = { filename: string; reason: string };

  let {
    rows,
    busy = false,
    onConfirm,
    onCancel,
  }: {
    rows: MismatchRow[];
    busy?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();
</script>

<!-- While the install runs, neither button works, so block the implicit
     close paths too — the in-flight IPC can't be aborted. -->
<Modal
  ariaLabel={$t('instance.compat.dialogLabel')}
  onClose={onCancel}
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
>
  <header class="p-4 border-b">
    <h2 class="text-lg font-semibold text-warning-text flex items-center gap-1.5">
      <Icon name="warning" class="text-warning-text" />
      {$t('instance.compat.heading', { count: rows.length })}
    </h2>
  </header>
  <ul class="p-4 space-y-2 max-h-[50vh] overflow-y-auto">
    {#each rows as r (r.filename)}
      <li class="text-sm bg-warning-bg border border-warning-text/30 rounded px-2 py-1.5">
        <span class="font-medium">{r.filename}</span>
        <span class="text-warning-text"> — {r.reason}</span>
      </li>
    {/each}
  </ul>
  <footer class="p-4 border-t flex justify-end gap-2">
    <!-- Skip is blocked while the install runs: cancelling now can't abort the in-flight IPC, so we wait for it to settle. -->
    <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={onCancel}>
      {$t('instance.compat.skipBtn')}
    </button>
    <BusyButton {busy} class="btn-warning btn-sm" onclick={onConfirm}>
      {$t('instance.compat.installAnywayBtn')}
    </BusyButton>
  </footer>
</Modal>
