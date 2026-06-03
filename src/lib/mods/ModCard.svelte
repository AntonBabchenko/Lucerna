<script lang="ts">
  import type { InstalledMod, ModSummary, ModUpdateState } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';

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
    layout = 'grid',
    dense = false,
    highlighted = false,
    selectable = false,
    selected = false,
    onSelectChange = (_checked: boolean) => {},
  }: {
    summary: ModSummary | null;
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
    layout?: 'grid' | 'list';
    dense?: boolean;
    highlighted?: boolean;
    selectable?: boolean;
    selected?: boolean;
    onSelectChange?: (checked: boolean) => void;
  } = $props();

  // True when the installed record came from a different platform than
  // the card we're rendering (user installed Cloth Config via Modrinth,
  // is now looking at the CurseForge entry for the same mod). We still
  // show the Installed state to avoid a misleading Install button, but
  // hint at the source so the user understands why the version number
  // doesn't match what CF lists.
  const crossPlatform = $derived(
    summary !== null &&
      installed !== null &&
      installed.source !== null &&
      installed.source !== summary.source,
  );
  const otherPlatformLabel = $derived(
    installed?.source === 'modrinth'
      ? 'Modrinth'
      : installed?.source === 'curseforge'
        ? 'CurseForge'
        : null,
  );

  // Degraded row: no platform summary. Either a hand-dropped manual mod
  // (source null), a modpack-bundled jar we couldn't identify (packChip set),
  // or a platform mod whose summary lookup failed transiently (source set).
  const isPlatform = $derived(installed !== null && installed.source !== null);
  const degradedTitle = $derived(
    isPlatform && !packChip ? (installed?.name ?? '') : (installed?.filename ?? ''),
  );
  const sourceLabel = $derived(
    installed?.source === 'curseforge'
      ? 'CurseForge'
      : installed?.source === 'modrinth'
        ? 'Modrinth'
        : null,
  );
</script>

<!--
  The action cluster is identical in both layouts — grid and list differ only
  in form, not function. Defined once as a snippet and rendered in each branch
  so the two can't drift apart.
-->
{#snippet actions()}
  {#if installed}
    {#if packChip}
      <span
        class="text-xs px-2 py-0.5 rounded bg-accent-soft text-accent"
        title={$t('mods.card.fromModpackTitle', { name: packChip })}
      >
        📦 {packChip}
      </span>
    {:else if checking}
      <span class="text-xs px-2 py-0.5 text-placeholder">{$t('mods.card.checking')}</span>
    {:else if updateState && updateState.kind === 'update_available'}
      <span
        class="text-xs px-2 py-0.5 rounded bg-warning-bg text-warning-text"
        title={$t('mods.card.updateAvailableTitle')}
      >
        v{installed.version_number ?? '?'} → v{updateState.target.version_number}
      </span>
      <button type="button" class="btn-warning btn-xs" onclick={onUpdate}
        >{$t('mods.card.update')}</button
      >
    {:else if updateState && updateState.kind === 'check_failed'}
      <span class="text-xs px-2 py-0.5 text-placeholder" title={updateState.reason}>
        {$t('mods.card.checkFailed')}
      </span>
    {/if}
    <span
      class="text-xs px-2 py-0.5 rounded {installed.enabled
        ? 'bg-success-bg text-success'
        : 'bg-subtle text-muted'}"
      title={crossPlatform && otherPlatformLabel
        ? `Installed via ${otherPlatformLabel} (v${installed.version_number ?? '?'})`
        : installed.version_number
          ? `Version ${installed.version_number} on disk`
          : $t('mods.card.installed')}
    >
      {installed.enabled ? $t('mods.card.installed') : $t('mods.card.disabled')}{crossPlatform &&
      otherPlatformLabel
        ? ` (${otherPlatformLabel})`
        : installed.version_number
          ? ` · v${installed.version_number}`
          : ''}
    </span>
    <button type="button" class="btn-secondary btn-xs" onclick={onToggle}>
      {installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
    </button>
    <button type="button" class="btn-ghost-danger btn-xs" onclick={onUninstall}
      >{$t('mods.card.uninstall')}</button
    >
  {:else}
    <button type="button" class="btn-primary btn-xs whitespace-nowrap" onclick={onInstall}>
      {$t('mods.card.install')}
    </button>
  {/if}
{/snippet}

{#if summary === null}
  <!-- Degraded row: no ModSummary. Same shape as the list row but with a
       placeholder icon and a filename/identity-based title. Used by the
       Installed tab for manual mods, pack-bundled jars, and failed lookups. -->
  <div
    class="flex items-center gap-3 px-3 {dense
      ? 'py-1'
      : 'py-2'} border-b border-border-subtle {highlighted
      ? 'bg-highlight'
      : 'bg-surface hover:bg-subtle'} transition-colors"
    data-testid="manual-mod-row"
  >
    {#if selectable && installed}
      <input
        type="checkbox"
        class="flex-shrink-0"
        checked={selected}
        aria-label={$t('mods.installed.selectModAriaLabel', { filename: installed.filename })}
        onclick={(e) => e.stopPropagation()}
        onchange={(e) => onSelectChange((e.currentTarget as HTMLInputElement).checked)}
      />
    {/if}
    <div
      class="w-8 h-8 rounded bg-subtle flex items-center justify-center text-placeholder text-xs flex-shrink-0"
      aria-hidden="true"
    >
      ◆
    </div>
    <div class="flex-1 min-w-0">
      <div class="font-medium text-primary truncate">{degradedTitle}</div>
      {#if installed}
        <div class="text-xs text-muted truncate">
          {packChip
            ? $t('mods.installed.fromModpack')
            : isPlatform
              ? `${sourceLabel ?? ''} · ${$t('mods.installed.detailsUnavailable')}`
              : $t('mods.installed.manualMod')} · {installed.enabled
            ? $t('mods.installed.enabledStatus')
            : $t('mods.installed.disabledStatus')}
        </div>
      {/if}
    </div>
    <div class="flex items-center gap-1 flex-shrink-0">
      {#if packChip}
        <span
          class="text-xs px-2 py-0.5 rounded bg-accent-soft text-accent"
          title={$t('mods.card.fromModpackTitle', { name: packChip })}>📦 {packChip}</span
        >
      {/if}
      {#if installed}
        <button type="button" class="btn-secondary btn-xs" onclick={onToggle}
          >{installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}</button
        >
        <button type="button" class="btn-ghost-danger btn-xs" onclick={onUninstall}
          >{$t('mods.card.uninstall')}</button
        >
      {/if}
    </div>
  </div>
{:else if layout === 'grid'}
  <!--
    Multi-column grid tile. Same actions as the list row (rendered via the
    shared snippet); only the shape differs — icon + title on top, clamped
    summary, action cluster wraps at the bottom.
  -->
  <div class="border border-border-subtle rounded bg-surface p-3 flex flex-col gap-2 h-full">
    <button type="button" class="flex items-start gap-2 text-left min-w-0" onclick={onOpenDetail}>
      {#if summary.icon_url}
        <img src={summary.icon_url} alt="" class="w-10 h-10 rounded flex-shrink-0" />
      {:else}
        <div
          class="w-10 h-10 rounded bg-subtle flex items-center justify-center text-placeholder flex-shrink-0"
        >
          ◆
        </div>
      {/if}
      <span class="min-w-0">
        <span class="block font-medium text-primary truncate">{summary.name}</span>
        <span class="block text-xs text-muted truncate">
          {$t('mods.card.byAuthorDownloads', {
            author: summary.author,
            downloads: (summary.downloads ?? 0).toLocaleString(),
          })}
        </span>
      </span>
    </button>

    <p class="text-sm text-secondary line-clamp-2 flex-1">{summary.summary}</p>

    <div class="flex items-center gap-1 flex-wrap">
      {@render actions()}
    </div>
  </div>
{:else}
  <div
    class="flex items-center gap-3 px-3 {dense
      ? 'py-1'
      : 'py-2'} border-b border-border-subtle {highlighted
      ? 'bg-highlight'
      : 'bg-surface hover:bg-subtle'} transition-colors"
    data-testid="card-list-row"
  >
    {#if selectable}
      <input
        type="checkbox"
        class="flex-shrink-0"
        checked={selected}
        aria-label={$t('mods.card.selectAriaLabel', { name: summary.name })}
        onclick={(e) => e.stopPropagation()}
        onchange={(e) => onSelectChange((e.currentTarget as HTMLInputElement).checked)}
      />
    {/if}
    {#if summary.icon_url}
      <img src={summary.icon_url} alt="" class="w-8 h-8 rounded flex-shrink-0" />
    {:else}
      <div
        class="w-8 h-8 rounded bg-subtle flex items-center justify-center text-placeholder text-xs flex-shrink-0"
      >
        ◆
      </div>
    {/if}

    <button type="button" class="flex-1 text-left min-w-0" onclick={onOpenDetail}>
      <span class="font-medium text-primary truncate">{summary.name}</span>
      <span class="text-xs text-muted ml-2"
        >{$t('mods.card.byAuthorDownloads', {
          author: summary.author,
          downloads: (summary.downloads ?? 0).toLocaleString(),
        })}</span
      >
    </button>

    <div class="flex items-center gap-1 flex-shrink-0">
      {@render actions()}
    </div>
  </div>
{/if}
