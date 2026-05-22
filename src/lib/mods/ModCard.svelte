<script lang="ts">
  import type { InstalledMod, ModSummary, ModUpdateState } from '$lib/ipc/bindings';

  // One result card in ModBrowseView. Shows mod metadata plus
  // install-state-aware controls:
  //   - not installed → blue "Install" button (calls onInstall)
  //   - installed + enabled → green "Installed" pill + Disable + Uninstall
  //   - installed + disabled → grey "Disabled" pill + Enable + Uninstall
  //
  // We don't pre-resolve a version per card (would be 20 extra round
  // trips per search page). onInstall does the lookup-and-install path
  // lazily; if the latest compatible version turns out to be
  // distribution-disabled on CurseForge, the parent surfaces the typed
  // ModsDistributionDisabled error and the user can open the project
  // page from the detail drawer.

  let {
    summary,
    installed,
    onInstall,
    onOpenDetail,
    onToggle,
    onUninstall,
    updateState = null,
    onUpdate = () => {},
    checking = false,
    packChip = null,
  }: {
    summary: ModSummary;
    installed: InstalledMod | null;
    onInstall: () => void;
    onOpenDetail: () => void;
    onToggle: () => void;
    onUninstall: () => void;
    // Update-check extras — only InstalledModsView passes these.
    // `updateState` is the per-mod result; `packChip`, when set, is the
    // modpack name (the card then shows a "from modpack" chip and no
    // update affordance — pack mods are not individually updatable).
    updateState?: ModUpdateState | null;
    onUpdate?: () => void;
    checking?: boolean;
    packChip?: string | null;
  } = $props();

  // True when the installed record came from a different platform than
  // the card we're rendering (user installed Cloth Config via Modrinth,
  // is now looking at the CurseForge entry for the same mod). We still
  // show the Installed state to avoid a misleading Install button, but
  // hint at the source so the user understands why the version number
  // doesn't match what CF lists.
  const crossPlatform = $derived(
    installed !== null && installed.source !== null && installed.source !== summary.source,
  );
  const otherPlatformLabel = $derived(
    installed?.source === 'modrinth'
      ? 'Modrinth'
      : installed?.source === 'curseforge'
        ? 'CurseForge'
        : null,
  );
</script>

<div class="border border-neutral-200 rounded bg-white p-3 flex gap-3">
  {#if summary.icon_url}
    <img src={summary.icon_url} alt="" class="w-12 h-12 rounded" />
  {:else}
    <div class="w-12 h-12 rounded bg-neutral-100 flex items-center justify-center text-neutral-400">
      ◆
    </div>
  {/if}

  <button type="button" class="flex-1 text-left min-w-0" onclick={onOpenDetail}>
    <div class="font-medium text-neutral-900 truncate">{summary.name}</div>
    <div class="text-xs text-neutral-500 truncate">
      by {summary.author} · {(summary.downloads ?? 0).toLocaleString()} dl
    </div>
    <div class="text-sm text-neutral-700 truncate">{summary.summary}</div>
  </button>

  <div class="self-center flex items-center gap-1">
    {#if installed}
      {#if packChip}
        <span
          class="text-xs px-2 py-1 rounded bg-indigo-50 text-indigo-700"
          title="From modpack: {packChip}"
        >
          📦 {packChip}
        </span>
      {:else if checking}
        <span class="text-xs px-2 py-1 text-neutral-400">Checking…</span>
      {:else if updateState && updateState.kind === 'update_available'}
        <span
          class="text-xs px-2 py-1 rounded bg-amber-50 text-amber-800"
          title="Update available"
        >
          v{installed.version_number ?? '?'} → v{updateState.target.version_number}
        </span>
        <button
          type="button"
          class="text-xs px-2 py-1 border border-amber-300 rounded bg-amber-100 text-amber-900 hover:bg-amber-200"
          onclick={onUpdate}
        >
          Update
        </button>
      {:else if updateState && updateState.kind === 'check_failed'}
        <span class="text-xs px-2 py-1 text-neutral-400" title={updateState.reason}>
          couldn't check
        </span>
      {/if}
      <span
        class="text-xs px-2 py-1 rounded"
        class:bg-green-50={installed.enabled}
        class:text-green-700={installed.enabled}
        class:bg-neutral-100={!installed.enabled}
        class:text-neutral-500={!installed.enabled}
        title={crossPlatform && otherPlatformLabel
          ? `Installed via ${otherPlatformLabel} (v${installed.version_number ?? '?'})`
          : installed.version_number
            ? `Version ${installed.version_number} on disk`
            : 'Installed'}
      >
        {installed.enabled ? 'Installed' : 'Disabled'}{crossPlatform && otherPlatformLabel
          ? ` (${otherPlatformLabel})`
          : installed.version_number
            ? ` · v${installed.version_number}`
            : ''}
      </span>
      <button type="button" class="text-xs px-2 py-1 border rounded" onclick={onToggle}>
        {installed.enabled ? 'Disable' : 'Enable'}
      </button>
      <button
        type="button"
        class="text-xs px-2 py-1 border rounded text-red-700 hover:bg-red-50"
        onclick={onUninstall}
      >
        Uninstall
      </button>
    {:else}
      <button
        type="button"
        class="px-3 py-1 text-sm bg-blue-600 hover:bg-blue-700 text-white rounded"
        onclick={onInstall}
      >
        Install
      </button>
    {/if}
  </div>
</div>
