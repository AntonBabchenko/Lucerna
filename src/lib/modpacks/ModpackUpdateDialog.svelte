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
  <div class="bg-white rounded-lg shadow-xl w-[480px] max-h-[80vh] p-5 flex flex-col gap-3">
    <h3 class="font-semibold text-base">Update to {diff.new_version_number}</h3>

    <div class="text-sm text-neutral-700">
      +{diff.added.length} added · −{diff.removed.length} removed · ⟳{diff.updated.length} updated
    </div>

    {#if diff.version_bump}
      <div class="text-sm bg-amber-50 border border-amber-200 rounded p-2 text-amber-900">
        Minecraft {diff.version_bump.old_game_version} → {diff.version_bump.new_game_version}. After
        updating, click <span class="font-semibold">Install</span> to download the new Minecraft
        version.
      </div>
    {/if}

    <div
      class="flex-1 overflow-y-auto border rounded divide-y text-sm"
      data-testid="update-diff-list"
    >
      {#each diff.added as f (f.install_path)}
        <div class="px-2 py-1 text-green-700">+ {f.name}</div>
      {/each}
      {#each diff.updated as e (e.new.install_path)}
        <div class="px-2 py-1 text-blue-700">⟳ {e.new.name}</div>
      {/each}
      {#each diff.removed as f (f.install_path)}
        <div class="px-2 py-1 text-red-700 line-through">− {f.name}</div>
      {/each}
      {#if diff.added.length + diff.updated.length + diff.removed.length === 0}
        <div class="px-2 py-3 text-neutral-500 text-center">No file changes in this version.</div>
      {/if}
    </div>

    <div class="flex justify-end gap-2">
      <button type="button" class="border rounded px-3 py-1 text-sm" onclick={onCancel}>
        Cancel
      </button>
      <button
        type="button"
        class="bg-blue-600 text-white rounded px-3 py-1 text-sm hover:bg-blue-700"
        onclick={onConfirm}
        data-testid="update-confirm"
      >
        Update
      </button>
    </div>
  </div>
</div>
