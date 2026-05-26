<script lang="ts">
  import type { ModpackUpdateDiff } from '$lib/ipc/bindings';

  let {
    diff,
    onCancel,
    onConfirm,
  }: {
    diff: ModpackUpdateDiff;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<div
  class="fixed inset-0 bg-black/50 flex items-center justify-center z-[60]"
  role="dialog"
  aria-modal="true"
  aria-label="Modpack update confirmation"
>
  <div class="bg-surface rounded-lg shadow-xl w-[480px] max-h-[80vh] p-5 flex flex-col gap-3">
    <h3 class="font-semibold text-base text-primary">Update to {diff.new_version_number}</h3>

    <div class="text-sm text-secondary">
      +{diff.added.length} added · −{diff.removed.length} removed · ⟳{diff.updated.length} updated
    </div>

    {#if diff.version_bump}
      <div
        class="text-sm bg-warning-bg border border-warning-text/30 rounded p-2 text-warning-text"
      >
        Minecraft {diff.version_bump.old_game_version} → {diff.version_bump.new_game_version}. After
        updating, click <span class="font-semibold">Install</span> to download the new Minecraft version.
      </div>
    {/if}

    <div
      class="flex-1 overflow-y-auto border rounded divide-y text-sm"
      data-testid="update-diff-list"
    >
      {#each diff.added as f (f.install_path)}
        <div class="px-2 py-1 text-success">+ {f.name}</div>
      {/each}
      {#each diff.updated as e (e.new.install_path)}
        <div class="px-2 py-1 text-accent">⟳ {e.new.name}</div>
      {/each}
      {#each diff.removed as f (f.install_path)}
        <div class="px-2 py-1 text-danger line-through">− {f.name}</div>
      {/each}
      {#if diff.added.length + diff.updated.length + diff.removed.length === 0}
        <div class="px-2 py-3 text-muted text-center">No file changes in this version.</div>
      {/if}
    </div>

    <div class="flex justify-end gap-2">
      <button type="button" class="border rounded px-3 py-1 text-sm" onclick={onCancel}>
        Cancel
      </button>
      <button
        type="button"
        class="bg-accent text-white rounded px-3 py-1 text-sm hover:bg-accent"
        onclick={onConfirm}
        data-testid="update-confirm"
      >
        Update
      </button>
    </div>
  </div>
</div>
