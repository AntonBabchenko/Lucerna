<script lang="ts">
  import type { InstalledMod, ModSummary, ModUpdateState } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import ContextMenu, { type ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';
  import { cardStatusStyle, accentDotClass, type CardStatusKind } from '$lib/ui/cards/card-status';

  // One card for mods, resource packs, and shaders, in a single compact style.
  // `layout` only switches the shape (list row vs grid tile) — there is no
  // separate "comfortable" density. Actions are always icon buttons with
  // tooltips; the full set is also on the right-click ContextMenu. A
  // `summary === null` branch renders a degraded/manual row.

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
    attention = null,
    layout = 'grid',
    highlighted = false,
    selectable = false,
    selected = false,
    onSelectChange = (_checked: boolean) => {},
    canToggle = true,
    installing = false,
    placeholderIcon = 'puzzle',
  }: {
    summary: ModSummary | null;
    installed: InstalledMod | null;
    onInstall: () => void;
    onOpenDetail: () => void;
    onToggle: () => void;
    onUninstall: () => void;
    updateState?: ModUpdateState | null;
    onUpdate?: () => void;
    checking?: boolean;
    packChip?: string | null;
    // Installed-tab attention state that outranks enabled/disabled for the accent
    // strip (InstalledModRow passes it; browse leaves it null).
    attention?: 'incompatible' | 'missing-deps' | null;
    layout?: 'grid' | 'list';
    highlighted?: boolean;
    selectable?: boolean;
    selected?: boolean;
    onSelectChange?: (checked: boolean) => void;
    canToggle?: boolean;
    installing?: boolean;
    placeholderIcon?: IconName;
  } = $props();

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
  const hasUpdate = $derived(!packChip && !!updateState && updateState.kind === 'update_available');

  const statusKind = $derived.by((): CardStatusKind => {
    if (!installed) return 'none';
    if (attention === 'incompatible') return 'incompatible';
    if (attention === 'missing-deps') return 'missing-deps';
    if (packChip) return 'from-pack';
    if (hasUpdate) return 'update';
    if (crossPlatform) return 'cross-platform';
    return installed.enabled ? 'enabled' : 'disabled';
  });
  const style = $derived(cardStatusStyle(statusKind));

  // The single muted secondary line for an installed mod (version is the norm;
  // cross-platform explains the version mismatch; otherwise the install state).
  const installedMeta = $derived.by(() => {
    if (!installed) return '';
    const stateWord = installed.enabled ? $t('mods.card.installed') : $t('mods.card.disabled');
    if (crossPlatform && otherPlatformLabel) return `${stateWord} (${otherPlatformLabel})`;
    if (installed.version_number) return `v${installed.version_number}`;
    return stateWord;
  });

  // Degraded-row identity (summary null).
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
  const degradedMeta = $derived.by(() => {
    if (!installed) return '';
    const base = packChip
      ? $t('mods.installed.fromModpack')
      : isPlatform
        ? `${sourceLabel ?? ''} · ${$t('mods.installed.detailsUnavailable')}`
        : $t('mods.installed.manualMod');
    const stateWord = installed.enabled
      ? $t('mods.installed.enabledStatus')
      : $t('mods.installed.disabledStatus');
    return `${base} · ${stateWord}`;
  });

  // Context menu (right-click / Shift+F10) — the full action set.
  const menuItems = $derived.by((): ContextMenuItem[] => {
    if (!installed)
      return [{ label: $t('mods.card.install'), icon: 'download', onSelect: onInstall }];
    const out: ContextMenuItem[] = [];
    if (hasUpdate) out.push({ label: $t('mods.card.update'), icon: 'refresh', onSelect: onUpdate });
    if (canToggle)
      out.push({
        label: installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable'),
        icon: 'power',
        onSelect: onToggle,
      });
    if (summary) out.push({ label: $t('mods.card.details'), icon: 'info', onSelect: onOpenDetail });
    out.push({
      label: $t('mods.card.uninstall'),
      icon: 'trash',
      danger: true,
      separatorBefore: out.length > 0,
      onSelect: onUninstall,
    });
    return out;
  });

  const menuLabel = $derived(
    $t('mods.card.menuAriaLabel', { name: summary?.name ?? degradedTitle }),
  );
</script>

{#snippet iconActions()}
  {#if installed}
    {#if hasUpdate}
      <button
        type="button"
        class="btn-icon btn-icon-sm btn-icon-warning"
        onclick={onUpdate}
        aria-label={$t('mods.card.update')}
        use:tooltip={$t('mods.card.update')}><Icon name="refresh" size={15} /></button
      >
    {/if}
    {#if canToggle}
      <button
        type="button"
        class={`btn-icon btn-icon-sm ${installed.enabled ? 'btn-icon-success' : '!text-muted'}`}
        onclick={onToggle}
        aria-label={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
        use:tooltip={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
        ><Icon name="power" size={15} /></button
      >
    {/if}
    <button
      type="button"
      class="btn-icon btn-icon-sm btn-icon-danger"
      onclick={onUninstall}
      aria-label={$t('mods.card.uninstall')}
      use:tooltip={$t('mods.card.uninstall')}><Icon name="trash" size={15} /></button
    >
  {:else}
    <button
      type="button"
      class="btn-icon btn-icon-sm !text-accent"
      onclick={onInstall}
      disabled={installing}
      aria-label={$t('mods.card.install')}
      use:tooltip={$t('mods.card.install')}
    >
      {#if installing}<Spinner size="sm" />{:else}<Icon name="download" size={15} />{/if}
    </button>
  {/if}
{/snippet}

{#snippet badges()}
  {#if packChip}
    <StatusBadge
      variant="info"
      icon="package"
      title={$t('mods.card.fromModpackTitle', { name: packChip })}
      testid="mod-pack-chip"
    >
      {packChip}
    </StatusBadge>
  {:else if checking}
    <span class="text-xs text-placeholder">{$t('mods.card.checking')}</span>
  {:else if hasUpdate && updateState?.kind === 'update_available'}
    <StatusBadge
      variant="warning"
      title={$t('mods.card.updateAvailableTitle')}
      testid="mod-update-badge"
    >
      v{installed?.version_number ?? '?'}
      <Icon name="arrowRight" size={12} /> v{updateState.target.version_number}
    </StatusBadge>
  {:else if updateState && updateState.kind === 'check_failed'}
    <span class="text-xs text-placeholder" use:tooltip={updateState.reason}
      >{$t('mods.card.checkFailed')}</span
    >
  {/if}
{/snippet}

{#if summary === null}
  <ContextMenu items={menuItems} ariaLabel={menuLabel}>
    <CardShell
      variant="compact-row"
      accent={style.accent}
      dim={style.dim}
      {highlighted}
      testid="manual-mod-row"
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
      <CardMedia iconUrl={null} placeholder={isPlatform ? 'circleX' : placeholderIcon} size="sm" />
      <div class="flex-1 min-w-0">
        <span class="font-medium text-primary truncate font-mono text-xs">{degradedTitle}</span>
        {#if installed}<span class="text-xs text-muted ml-2">{degradedMeta}</span>{/if}
      </div>
      <div class="flex items-center gap-1 flex-shrink-0">{@render badges()}</div>
      {#if installed}
        <div class="flex items-center gap-1 flex-shrink-0">
          {#if canToggle}
            <button
              type="button"
              class={`btn-icon btn-icon-sm ${installed.enabled ? 'btn-icon-success' : '!text-muted'}`}
              onclick={onToggle}
              aria-label={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
              use:tooltip={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
              ><Icon name="power" size={15} /></button
            >
          {/if}
          <button
            type="button"
            class="btn-icon btn-icon-sm btn-icon-danger"
            onclick={onUninstall}
            aria-label={$t('mods.card.uninstall')}
            use:tooltip={$t('mods.card.uninstall')}><Icon name="trash" size={15} /></button
          >
        </div>
      {/if}
    </CardShell>
  </ContextMenu>
{:else if layout === 'grid'}
  <CardShell variant="tile" accent={style.accent} dim={style.dim}>
    {#if installed}
      <span
        class="absolute top-2.5 right-2.5 w-2 h-2 rounded-full {accentDotClass(style.accent)}"
        aria-hidden="true"
      ></span>
    {/if}
    <button
      type="button"
      class="flex items-start gap-2 text-left min-w-0 w-full"
      onclick={onOpenDetail}
    >
      <CardMedia iconUrl={summary.icon_url} placeholder={placeholderIcon} size="md" />
      <span class="min-w-0">
        <span class="block font-medium text-primary truncate">{summary.name}</span>
        <span class="block text-xs text-muted truncate">
          {#if installed}
            {installedMeta}
          {:else}
            {$t('mods.card.byAuthorDownloads', {
              author: summary.author,
              downloads: (summary.downloads ?? 0).toLocaleString(),
            })}
          {/if}
        </span>
      </span>
    </button>
    <p class="text-xs text-secondary line-clamp-2 flex-1 mt-1.5">{summary.summary}</p>
    <div class="flex items-center justify-between gap-1 mt-2">
      <div class="flex items-center gap-1 flex-wrap min-w-0">{@render badges()}</div>
      <div class="flex items-center gap-1 flex-shrink-0">{@render iconActions()}</div>
    </div>
  </CardShell>
{:else}
  <ContextMenu items={menuItems} ariaLabel={menuLabel}>
    <CardShell
      variant="compact-row"
      accent={style.accent}
      dim={style.dim}
      {highlighted}
      testid="card-list-row"
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
      <CardMedia iconUrl={summary.icon_url} placeholder={placeholderIcon} size="sm" />
      <button
        type="button"
        class="flex flex-1 items-center gap-2 text-left min-w-0"
        onclick={onOpenDetail}
      >
        <span class="font-medium text-primary flex-shrink-0">{summary.name}</span>
        {#if installed}
          <span class="text-xs text-muted flex-shrink-0">{installedMeta}</span>
        {:else}
          <span class="text-xs text-muted flex-shrink-0 inline-flex items-center gap-1">
            <Icon name="user" size={12} />
            {summary.author}
            <Icon name="download" size={12} class="ml-1.5" />
            {(summary.downloads ?? 0).toLocaleString()}
          </span>
        {/if}
        {#if summary.summary}
          <span
            class="text-xs text-secondary flex-1 min-w-0 truncate border-l border-border-subtle pl-2"
            >{summary.summary}</span
          >
        {/if}
      </button>
      <div class="flex items-center gap-1 flex-shrink-0">{@render badges()}</div>
      <div class="flex items-center gap-1 flex-shrink-0">{@render iconActions()}</div>
    </CardShell>
  </ContextMenu>
{/if}
