<script lang="ts">
  // Confirmation modal shown when one or more dropped jars look
  // incompatible with the active instance. Compatible jars are never in
  // question — the dialog appears only because of the mismatched ones.
  //
  // `onConfirm` = install every jar (compatible + mismatched).
  // `onCancel`  = install only the compatible jars; skip the mismatched.

  type MismatchRow = { filename: string; reason: string };

  let {
    rows,
    onConfirm,
    onCancel,
  }: {
    rows: MismatchRow[];
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();
</script>

<div
  class="fixed inset-0 bg-black/40 flex items-center justify-center z-50"
  role="dialog"
  aria-modal="true"
  aria-label="Mod compatibility warning"
>
  <div class="bg-surface rounded-lg shadow-xl max-w-lg w-full">
    <header class="p-4 border-b">
      <h2 class="text-lg font-semibold text-warning-text">
        ⚠ {rows.length} mod{rows.length === 1 ? '' : 's'} may not be compatible
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
      <button type="button" class="btn-secondary btn-sm" onclick={onCancel}> Skip these </button>
      <button type="button" class="btn-warning btn-sm" onclick={onConfirm}> Install anyway </button>
    </footer>
  </div>
</div>
