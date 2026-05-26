<script lang="ts">
  import type { ModpackFile, ModpackSummary, ModpackUnresolvable } from '$lib/ipc/bindings';

  // Modal that shows the parsed `ModpackSummary` to the user and lets them
  // choose which optional mods to install. Required mods are listed but
  // not toggleable (the pack author marked them mandatory). Unresolvable
  // entries — files the backend cannot auto-download due to
  // distribution-disabled or host-allowlist constraints — render as a
  // red list with manual-action links the user clicks to fetch the mod
  // themselves (the installed-mods view will pick them up on next launch).
  //
  // No IPC here. The owning view drives `modpack_import` when `onConfirm`
  // fires with the chosen sha1 list (required + selected optionals).

  let {
    summary,
    onCancel,
    onConfirm,
  }: {
    summary: ModpackSummary;
    onCancel: () => void;
    onConfirm: (selectedShas: string[]) => void;
  } = $props();

  const required: ModpackFile[] = $derived(
    summary.files.filter((f) => f.env_client === 'required'),
  );
  const optional: ModpackFile[] = $derived(
    summary.files.filter((f) => f.env_client === 'optional'),
  );
  const unresolvable: ModpackUnresolvable[] = $derived(summary.unresolvable);

  // Optional selections — Set keyed by sha1. Defaults to empty so optional
  // mods are off by default; the user opts in explicitly.
  let optionalSelected = $state<Set<string>>(new Set());

  function toggle(sha: string) {
    // Re-assign so Svelte 5's $state reactivity picks up the change
    // (Set mutations in place do not trigger reactivity).
    const next = new Set(optionalSelected);
    if (next.has(sha)) next.delete(sha);
    else next.add(sha);
    optionalSelected = next;
  }

  const selectedShas = $derived([
    ...required.map((f) => f.sha1),
    ...optional.filter((f) => optionalSelected.has(f.sha1)).map((f) => f.sha1),
  ]);

  function formatSize(size: number | null): string {
    if (size == null) return '';
    return `${(size / 1024 / 1024).toFixed(1)} MiB`;
  }
</script>

<div
  class="fixed inset-0 bg-black/40 flex items-center justify-center z-50"
  role="dialog"
  aria-modal="true"
  aria-label="Modpack import picker"
>
  <div class="bg-surface rounded-lg shadow-xl max-w-2xl w-full max-h-[80vh] flex flex-col">
    <header class="p-4 border-b">
      <h2 class="text-lg font-semibold">{summary.name}</h2>
      <div class="text-sm text-muted">
        v{summary.version} ·
        {summary.format === 'modrinth' ? 'Modrinth .mrpack' : 'CurseForge .zip'}
        · MC {summary.game_version} · {summary.loader}{summary.loader_version
          ? ` ${summary.loader_version}`
          : ''}
      </div>
    </header>

    {#if summary.has_saves_in_overrides}
      <div
        class="m-4 p-3 bg-warning-bg border border-warning-text/30 rounded text-sm text-warning-text"
      >
        ⚠ This pack includes saved worlds in its overrides. They will be copied into the new
        instance.
      </div>
    {/if}

    <div class="flex-1 overflow-y-auto p-4">
      <h3 class="font-medium text-sm text-secondary mb-2">Required ({required.length})</h3>
      <ul class="space-y-1 mb-4">
        {#each required as f (f.sha1)}
          <li class="text-sm py-1 flex items-center">
            <input
              type="checkbox"
              checked
              disabled
              class="mr-2"
              aria-label={`Required: ${f.name}`}
            />
            <span>{f.name}</span>
            <span class="ml-auto text-placeholder text-xs">{formatSize(f.size)}</span>
          </li>
        {/each}
      </ul>

      {#if optional.length > 0}
        <h3 class="font-medium text-sm text-secondary mb-2">Optional ({optional.length})</h3>
        <ul class="space-y-1 mb-4">
          {#each optional as f (f.sha1)}
            <li class="text-sm py-1 flex items-center">
              <input
                type="checkbox"
                checked={optionalSelected.has(f.sha1)}
                onchange={() => toggle(f.sha1)}
                class="mr-2"
                aria-label={`Install ${f.name}`}
              />
              <span>{f.name}</span>
              <span class="ml-auto text-placeholder text-xs">{formatSize(f.size)}</span>
            </li>
          {/each}
        </ul>
      {/if}

      {#if unresolvable.length > 0}
        <h3 class="font-medium text-sm text-danger mb-2">
          Cannot auto-install ({unresolvable.length})
        </h3>
        <ul class="space-y-1">
          {#each unresolvable as u (u.manual_action_url)}
            <li class="text-sm py-1 flex items-center bg-danger/10 px-2 rounded">
              <span class="flex-1">{u.mod_name}</span>
              <a
                href={u.manual_action_url}
                target="_blank"
                rel="noopener"
                class="text-accent hover:underline text-xs">Open ↗</a
              >
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <footer class="p-4 border-t flex justify-end gap-2">
      <button type="button" class="px-3 py-1.5 text-sm rounded border" onclick={onCancel}
        >Cancel</button
      >
      <button
        type="button"
        class="px-3 py-1.5 text-sm rounded bg-accent text-white disabled:bg-muted"
        disabled={selectedShas.length === 0}
        onclick={() => onConfirm(selectedShas)}
      >
        Install {selectedShas.length} selected
      </button>
    </footer>
  </div>
</div>
