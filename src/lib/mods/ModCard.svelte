<script lang="ts">
  import type { InstalledMod, ModSummary, ModUpdateState } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { browserPrefs, type Density } from './browser-prefs.svelte';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import ContextMenu, { type ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';
  import { cardStatusStyle, accentDotClass, type CardStatusKind } from '$lib/ui/cards/card-status';

  // One result/installed card for mods, resource packs, and shaders. Composes
  // the shared card primitives (CardShell / CardMedia / StatusBadge / ContextMenu)
  // and a single card-status mapping so every surface speaks the same language.
  // Three forms: grid tile, list row (comfortable), and compact row (icon
  // actions). A `summary === null` branch renders a degraded/manual row.

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
    density = browserPrefs.density,
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
    density?: Density;
    highlighted?: boolean;
    selectable?: boolean;
    selected?: boolean;
    onSelectChange?: (checked: boolean) => void;
    canToggle?: boolean;
    installing?: boolean;
    placeholderIcon?: IconName;
  } = $props();

  const compact = $derived(layout === 'list' && density === 'compact');

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

  // Pill label preserves the exact text the existing tests assert
  // ("Installed", "Installed · v1.0", "Installed (Modrinth)").
  const pillText = $derived.by(() => {
    if (!installed) return '';
    const base = installed.enabled ? $t('mods.card.installed') : $t('mods.card.disabled');
    if (crossPlatform && otherPlatformLabel) return `${base} (${otherPlatformLabel})`;
    if (installed.version_number) return `${base} · v${installed.version_number}`;
    return base;
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

  // Context menu (compact + degraded rows) — the full action set.
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

{#snippet textActions()}
  {#if installed}
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
      <span class="text-xs px-2 py-0.5 text-placeholder">{$t('mods.card.checking')}</span>
    {:else if hasUpdate && updateState?.kind === 'update_available'}
      <StatusBadge
        variant="warning"
        title={$t('mods.card.updateAvailableTitle')}
        testid="mod-update-badge"
      >
        v{installed.version_number ?? '?'}
        <Icon name="arrowRight" size={12} /> v{updateState.target.version_number}
      </StatusBadge>
      <button type="button" class="btn-warning btn-xs" onclick={onUpdate}
        >{$t('mods.card.update')}</button
      >
    {:else if updateState && updateState.kind === 'check_failed'}
      <span class="text-xs px-2 py-0.5 text-placeholder" use:tooltip={updateState.reason}
        >{$t('mods.card.checkFailed')}</span
      >
    {/if}
    <StatusBadge variant={style.badge} title={pillText}>{pillText}</StatusBadge>
    {#if canToggle}
      <button type="button" class="btn-secondary btn-xs" onclick={onToggle}>
        {installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
      </button>
    {/if}
    <button type="button" class="btn-ghost-danger btn-xs" onclick={onUninstall}
      >{$t('mods.card.uninstall')}</button
    >
  {:else}
    <BusyButton busy={installing} class="btn-primary btn-xs whitespace-nowrap" onclick={onInstall}>
      {$t('mods.card.install')}
    </BusyButton>
  {/if}
{/snippet}

{#snippet iconActions()}
  {#if installed}
    {#if hasUpdate}
      <button
        type="button"
        class="btn-icon !w-7 !h-7 text-warning-text"
        onclick={onUpdate}
        aria-label={$t('mods.card.update')}
        use:tooltip={$t('mods.card.update')}><Icon name="refresh" size={15} /></button
      >
    {/if}
    {#if canToggle}
      <button
        type="button"
        class={`btn-icon !w-7 !h-7 ${installed.enabled ? 'text-success' : 'text-muted'}`}
        onclick={onToggle}
        aria-label={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
        use:tooltip={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
        ><Icon name="power" size={15} /></button
      >
    {/if}
    <button
      type="button"
      class="btn-icon !w-7 !h-7 text-danger"
      onclick={onUninstall}
      aria-label={$t('mods.card.uninstall')}
      use:tooltip={$t('mods.card.uninstall')}><Icon name="trash" size={15} /></button
    >
  {:else}
    <BusyButton busy={installing} class="btn-primary btn-xs whitespace-nowrap" onclick={onInstall}>
      <Icon name="download" size={14} />
      {$t('mods.card.install')}
    </BusyButton>
  {/if}
{/snippet}

{#if summary === null}
  <ContextMenu items={menuItems} ariaLabel={menuLabel}>
    <CardShell
      variant={compact ? 'compact-row' : 'row'}
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
      <CardMedia
        iconUrl={null}
        placeholder={isPlatform ? 'circleX' : placeholderIcon}
        size={compact ? 'sm' : 'md'}
      />
      <div class="flex-1 min-w-0">
        <div class="font-medium text-primary truncate font-mono text-xs">{degradedTitle}</div>
        {#if installed && !compact}
          <div class="text-xs text-muted truncate">
            {(packChip
              ? $t('mods.installed.fromModpack')
              : isPlatform
                ? `${sourceLabel ?? ''} · ${$t('mods.installed.detailsUnavailable')}`
                : $t('mods.installed.manualMod')) +
              ' · ' +
              (installed.enabled
                ? $t('mods.installed.enabledStatus')
                : $t('mods.installed.disabledStatus'))}
          </div>
        {/if}
      </div>
      <div class="flex items-center gap-1 flex-shrink-0">
        {#if packChip}
          <StatusBadge
            variant="info"
            icon="package"
            title={$t('mods.card.fromModpackTitle', { name: packChip })}
          >
            {packChip}
          </StatusBadge>
        {/if}
        {#if installed}
          {#if compact}
            {#if canToggle}
              <button
                type="button"
                class={`btn-icon !w-7 !h-7 ${installed.enabled ? 'text-success' : 'text-muted'}`}
                onclick={onToggle}
                aria-label={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
                use:tooltip={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
                ><Icon name="power" size={15} /></button
              >
            {/if}
            <button
              type="button"
              class="btn-icon !w-7 !h-7 text-danger"
              onclick={onUninstall}
              aria-label={$t('mods.card.uninstall')}
              use:tooltip={$t('mods.card.uninstall')}><Icon name="trash" size={15} /></button
            >
          {:else}
            {#if canToggle}
              <button type="button" class="btn-secondary btn-xs" onclick={onToggle}>
                {installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
              </button>
            {/if}
            <button type="button" class="btn-ghost-danger btn-xs" onclick={onUninstall}
              >{$t('mods.card.uninstall')}</button
            >
          {/if}
        {/if}
      </div>
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
      <CardMedia iconUrl={summary.icon_url} placeholder={placeholderIcon} size="lg" />
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
    <p class="text-sm text-secondary line-clamp-2 flex-1 mt-2">{summary.summary}</p>
    <div class="flex items-center gap-1 flex-wrap mt-2">{@render textActions()}</div>
  </CardShell>
{:else}
  <ContextMenu items={menuItems} ariaLabel={menuLabel}>
    <CardShell
      variant={compact ? 'compact-row' : 'row'}
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
      <CardMedia
        iconUrl={summary.icon_url}
        placeholder={placeholderIcon}
        size={compact ? 'sm' : 'md'}
      />
      <button type="button" class="flex-1 text-left min-w-0 truncate" onclick={onOpenDetail}>
        <span class="font-medium text-primary">{summary.name}</span>
        {#if compact}
          {#if installed}
            <span class="text-xs text-muted ml-2">v{installed.version_number ?? '?'}</span>
          {:else}
            <span class="text-xs text-muted ml-2 inline-flex items-center gap-1">
              <Icon name="user" size={12} />
              {summary.author}
              <Icon name="download" size={12} class="ml-1.5" />
              {(summary.downloads ?? 0).toLocaleString()}
            </span>
          {/if}
        {:else}
          <span class="text-xs text-muted ml-2"
            >{$t('mods.card.byAuthorDownloads', {
              author: summary.author,
              downloads: (summary.downloads ?? 0).toLocaleString(),
            })}</span
          >
        {/if}
      </button>
      <div class="flex items-center gap-1 flex-shrink-0">
        {#if compact}{@render iconActions()}{:else}{@render textActions()}{/if}
      </div>
    </CardShell>
  </ContextMenu>
{/if}
