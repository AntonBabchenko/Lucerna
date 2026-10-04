<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { InstalledMod, ModSummary, ModUpdateState } from '$lib/ipc/bindings';
  import { locale, t } from '$lib/i18n';
  import { formatCount } from '$lib/format/count';
  import { displayVersion } from '$lib/format/version';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import SelectRowCheckbox from '$lib/ui/SelectRowCheckbox.svelte';
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
    onShowChangelog = null,
    held = false,
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
    installedLabel = null,
    actionsBlockedReason = null,
    relation,
    below,
    onRevealFile = null,
    onOpenProjectPage = null,
    hold = null,
  }: {
    summary: ModSummary | null;
    installed: InstalledMod | null;
    onInstall: () => void;
    onOpenDetail: () => void;
    onToggle: () => void;
    onUninstall: () => void;
    updateState?: ModUpdateState | null;
    onUpdate?: () => void;
    /** Set = the update badge opens the changelog (the caller owns the "supported" gate). */
    onShowChangelog?: (() => void) | null;
    /** Updates for this project are held («Не обновлять»): a pin next to the version. */
    held?: boolean;
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
    // When set, REPLACES the whole installed meta line. Exists for kinds whose
    // installed state is not "live in the game": a datapack in the library is
    // inert until placed into a world, so its card must read «В библиотеке ·
    // vX», never the bare `vX` this card renders for installed mods/assets
    // (the #4083 wrong-belief hazard — slice-2 design §6).
    installedLabel?: string | null;
    // Why no action on this card can run right now, or null. Set, every
    // action button is disabled under one wrapper whose tooltip gives the
    // reason (a disabled button fires no pointer events — DESIGN.md §5), and
    // the context menu shows it under each item. Details stays open. The
    // server data pack catalog sets it for a world with only level.dat_old,
    // which refuses every add, switch and removal.
    actionsBlockedReason?: string | null;
    /** Installed list rows: the relation cell's content — the row's disclosure, in a fixed-width
     *  column after the checkbox and before the icon (DESIGN.md §9). Pass it on every installed
     *  row: an empty cell keeps the icons and names in line with the rows that have figures. */
    relation?: Snippet;
    /** List rows: a second line inside the card, under the row (the installed mod's problem
     *  line) — CardShell's `below`. */
    below?: Snippet;
    /** Installed rows: show the jar in the OS file manager (row menu) — the keyboard path to the
     *  file name the version's tooltip shows on hover. */
    onRevealFile?: (() => void) | null;
    /** Mods with a known project page only: open it in the browser (row menu). */
    onOpenProjectPage?: (() => void) | null;
    /** Mods whose hold state is known and can matter: «Не обновлять» / «Разрешить обновления». */
    hold?: { held: boolean; onToggle: () => void } | null;
  } = $props();

  const blocked = $derived(actionsBlockedReason !== null);
  const blockedMenu = $derived(
    blocked ? { disabled: true, disabledReason: actionsBlockedReason ?? undefined } : {},
  );

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
  // A list row's badges keep their width — except a modpack's name, which can be any length: it
  // gives way before the mod's own name does, capped and cut with «…» (its tooltip names the pack
  // in full). Uncapped, a long pack name squeezed the mod's name to one letter in the default
  // 820 px window. The pack chip never shares the slot with another badge.
  const listBadgesClass = $derived(
    `flex items-center gap-1 flex-shrink-0${packChip ? ' min-w-0 max-w-[30%]' : ''}`,
  );

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

  // The installed meta, split (spec §6.7, audit C-Q13): the version as its own node — its tooltip
  // is the jar's file name, the held pin sits beside it — or a state / cross-platform note
  // (cross-platform explains the version mismatch; otherwise the install state). Exactly one of
  // the two is set for an installed mod, so the visible text is what the one line said before.
  // An explicit `installedLabel` wins outright — see its prop doc.
  const meta = $derived.by((): { version: string | null; note: string | null } => {
    if (!installed) return { version: null, note: null };
    if (installedLabel) return { version: null, note: installedLabel };
    const stateWord = installed.enabled ? $t('mods.card.installed') : $t('mods.card.disabled');
    if (crossPlatform && otherPlatformLabel)
      return { version: null, note: `${stateWord} (${otherPlatformLabel})` };
    if (installed.version_number)
      return { version: displayVersion(installed.version_number), note: null };
    return { version: null, note: stateWord };
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
      return [
        { label: $t('common.install'), icon: 'download', onSelect: onInstall, ...blockedMenu },
      ];
    const out: ContextMenuItem[] = [];
    if (hasUpdate)
      out.push({
        label: $t('mods.card.update'),
        icon: 'refresh',
        onSelect: onUpdate,
        ...blockedMenu,
      });
    if (canToggle)
      out.push({
        label: installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable'),
        icon: 'power',
        onSelect: onToggle,
        ...blockedMenu,
      });
    if (summary) out.push({ label: $t('mods.card.details'), icon: 'info', onSelect: onOpenDetail });
    // Per-row conventions (DESIGN.md §8): an item that mirrors a control reuses its key («Открыть
    // страницу мода» is the pre-flight panel's); looking is never blocked, changing is; the
    // destructive item stays last, behind a separator.
    if (onOpenProjectPage)
      out.push({
        label: $t('mods.preflight.openModPage'),
        icon: 'externalLink',
        onSelect: onOpenProjectPage,
      });
    if (onRevealFile)
      out.push({ label: $t('mods.card.revealFile'), icon: 'folderOpen', onSelect: onRevealFile });
    if (hold)
      out.push({
        label: hold.held ? $t('mods.updates.unhold') : $t('mods.updates.hold'),
        icon: 'pin',
        onSelect: hold.onToggle,
        ...blockedMenu,
      });
    out.push({
      label: $t('mods.card.uninstall'),
      icon: 'trash',
      danger: true,
      separatorBefore: out.length > 0,
      onSelect: onUninstall,
      ...blockedMenu,
    });
    return out;
  });

  const menuLabel = $derived(
    $t('mods.card.menuAriaLabel', { name: summary?.name ?? degradedTitle }),
  );

  // The list row's text line and its version node. Short of room, the version node wraps onto a
  // second line that the one-line row clips (plan §5d M1) — then the name's tooltip carries it.
  let textLine = $state<HTMLElement | undefined>();
  let versionNode = $state<HTMLElement | undefined>();
  /** The version node sits below the row's one line: wrapped away, out of sight. A box with no
   *  layout (a hidden view) measures 0 tall and says no. */
  function versionWrappedAway(): boolean {
    if (!textLine || !versionNode) return false;
    const line = textLine.getBoundingClientRect();
    return line.height > 0 && versionNode.getBoundingClientRect().top >= line.bottom;
  }
  // What the version's own tooltip says, for the name to say while the version is away.
  const versionTip = $derived(
    installed && (meta.version ?? meta.note)
      ? `${meta.version ?? meta.note} · ${installed.filename}`
      : null,
  );
</script>

{#snippet iconActions()}
  {#if blocked}
    <!-- One focusable wrapper names why every action is off; the buttons keep
         their own aria-labels. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <span
      class="inline-flex items-center gap-1"
      tabindex="0"
      data-testid="card-actions-blocked"
      use:tooltip={{ text: actionsBlockedReason ?? '', describe: false }}
    >
      {@render actionButtons()}
    </span>
  {:else}
    {@render actionButtons()}
  {/if}
{/snippet}

{#snippet actionButtons()}
  {#if installed}
    {#if hasUpdate}
      <button
        type="button"
        class="btn-icon btn-icon-sm btn-icon-warning"
        disabled={blocked}
        onclick={onUpdate}
        aria-label={$t('mods.card.update')}
        use:tooltip={$t('mods.card.update')}><Icon name="refresh" size={15} /></button
      >
    {/if}
    {#if canToggle}
      <button
        type="button"
        class={`btn-icon btn-icon-sm ${installed.enabled ? 'btn-icon-success' : '!text-muted'}`}
        disabled={blocked}
        onclick={onToggle}
        aria-label={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
        use:tooltip={installed.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
        ><Icon name="power" size={15} /></button
      >
    {/if}
    <button
      type="button"
      class="btn-icon btn-icon-sm btn-icon-danger"
      disabled={blocked}
      onclick={onUninstall}
      aria-label={$t('mods.card.uninstall')}
      use:tooltip={$t('mods.card.uninstall')}><Icon name="trash" size={15} /></button
    >
  {:else}
    <button
      type="button"
      class="btn-icon btn-icon-sm !text-accent"
      onclick={onInstall}
      disabled={installing || blocked}
      aria-label={$t('common.install')}
      use:tooltip={$t('common.install')}
    >
      {#if installing}<Spinner size="sm" />{:else}<Icon name="download" size={15} />{/if}
    </button>
  {/if}
{/snippet}

<!-- The degraded row's two actions (a jar with no platform identity: no update to offer). Under
     `actionsBlockedReason` they are off behind the same one wrapper as the list row's, so a manual
     jar is never the one row a block forgot. -->
{#snippet degradedButtons(m: InstalledMod)}
  {#if canToggle}
    <button
      type="button"
      class={`btn-icon btn-icon-sm ${m.enabled ? 'btn-icon-success' : '!text-muted'}`}
      disabled={blocked}
      onclick={onToggle}
      aria-label={m.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
      use:tooltip={m.enabled ? $t('mods.card.disable') : $t('mods.card.enable')}
      ><Icon name="power" size={15} /></button
    >
  {/if}
  <button
    type="button"
    class="btn-icon btn-icon-sm btn-icon-danger"
    disabled={blocked}
    onclick={onUninstall}
    aria-label={$t('mods.card.uninstall')}
    use:tooltip={$t('mods.card.uninstall')}><Icon name="trash" size={15} /></button
  >
{/snippet}

{#snippet heldPin(klass: string)}
  {#if held}
    <!-- Beside the version it keeps: updates for this project are held (row menu). -->
    <span
      class="inline-flex text-secondary flex-shrink-0 {klass}"
      data-testid="mod-held-pin"
      use:tooltip={{ text: $t('mods.updates.heldTooltip'), describe: false }}
    >
      <Icon name="pin" size={12} label={$t('mods.updates.heldTooltip')} />
    </span>
  {/if}
{/snippet}

{#snippet relationCell()}
  <!-- The relation column (DESIGN.md §9): one width on every installed row, empty or not, so the
       icons and names after it line up. The list sets the width; see `.relation-col`. -->
  <div
    class="relation-col flex items-center flex-shrink-0 text-xs tabular-nums"
    data-testid="relation-col"
  >
    {@render relation?.()}
  </div>
{/snippet}

{#snippet badges()}
  {#if packChip}
    <StatusBadge
      variant="info"
      icon="package"
      title={$t('mods.card.fromModpackTitle', { name: packChip })}
      testid="mod-pack-chip"
      truncate
    >
      {packChip}
    </StatusBadge>
  {:else if checking}
    <span class="text-xs text-placeholder">{$t('mods.card.checking')}</span>
  {:else if hasUpdate && updateState?.kind === 'update_available'}
    {@const from = displayVersion(installed?.version_number ?? '?')}
    {@const to = displayVersion(updateState.target.version_number)}
    {#if onShowChangelog}
      <!-- The badge opens what changed (spec §6.6); its name keeps the versions it shows. -->
      <button
        type="button"
        class="rounded"
        aria-label={$t('mods.updates.badgeChangelogAria', { from, to })}
        use:tooltip={{ text: $t('mods.changelog.view'), describe: false }}
        onclick={onShowChangelog}
      >
        <StatusBadge variant="warning" icon="scrollText" testid="mod-update-badge">
          {from}
          <Icon name="arrowRight" size={12} />
          {to}
        </StatusBadge>
      </button>
    {:else}
      <StatusBadge
        variant="warning"
        title={$t('mods.card.updateAvailableTitle')}
        testid="mod-update-badge"
      >
        {from}
        <Icon name="arrowRight" size={12} />
        {to}
      </StatusBadge>
    {/if}
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
      {below}
    >
      {#if selectable && installed}
        <SelectRowCheckbox checked={selected} name={installed.filename} onChange={onSelectChange} />
      {/if}
      {#if relation}{@render relationCell()}{/if}
      <CardMedia iconUrl={null} placeholder={isPlatform ? 'circleX' : placeholderIcon} size="sm" />
      <div class="flex-1 min-w-0">
        <span class="font-medium text-primary truncate font-mono text-xs">{degradedTitle}</span>
        {#if installed}
          <span class="text-xs text-muted ml-2">{degradedMeta}</span>
          {@render heldPin('align-middle')}
        {/if}
      </div>
      <div class={listBadgesClass}>{@render badges()}</div>
      {#if installed}
        <div class="flex items-center gap-1 flex-shrink-0">
          {#if blocked}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <span
              class="inline-flex items-center gap-1"
              tabindex="0"
              data-testid="card-actions-blocked"
              use:tooltip={{ text: actionsBlockedReason ?? '', describe: false }}
            >
              {@render degradedButtons(installed)}
            </span>
          {:else}
            {@render degradedButtons(installed)}
          {/if}
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
            {meta.version ?? meta.note}
          {:else}
            <!-- Raw count, never pre-formatted: `{downloads, number}` groups the
                 digits using the UI locale, while a bare toLocaleString() uses
                 the OS one. Same rule as ModpackCard's download badge. -->
            {$t('mods.card.byAuthorDownloads', {
              author: summary.author,
              downloads: summary.downloads ?? 0,
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
      {below}
    >
      {#if selectable}
        <SelectRowCheckbox checked={selected} name={summary.name} onChange={onSelectChange} />
      {/if}
      {#if relation}{@render relationCell()}{/if}
      <CardMedia iconUrl={summary.icon_url} placeholder={placeholderIcon} size="sm" />
      <!-- An installed row's line is one line tall, and what wraps off it is clipped: short of room,
           the description goes first, then the version — which takes only the room the whole name
           leaves: from a few characters up (`basis-[4ch]`) it grows to its full width before the
           description gets any (`grow-[1000]`, `max-w-max`), ending in «…» while cut; with less it
           wraps onto the clipped line, never a sliver (plan §5d M1, screenshot n01j). The name is
           cut, ending in «…», only once it alone does not fit. Shrinking in proportion cut the name
           a fraction of a pixel while the version kept 2.7 px. A catalogue row keeps shrinking. -->
      <button
        bind:this={textLine}
        type="button"
        class="flex flex-1 items-center gap-2 text-left min-w-0 {installed
          ? 'h-5 flex-wrap overflow-hidden'
          : ''}"
        onclick={onOpenDetail}
      >
        <!-- Cut short, the name shows whole in its tooltip; with the version wrapped away, the
             tooltip carries the version too. -->
        <span
          class="min-w-0 truncate font-medium text-primary"
          use:tooltip={{
            text: summary.name,
            whenOverflowing: true,
            clippedText: versionTip ? `${summary.name} · ${versionTip}` : undefined,
            alsoClipped: versionTip ? versionWrappedAway : undefined,
          }}>{summary.name}</span
        >
        {#if installed}
          <!-- «Name · version · description» (spec D12): the file name is the version's tooltip —
               with the version whole before it while the version is cut short — and the pin sits
               beside the version it keeps, inside the version's room (`+1rem`). -->
          <span
            bind:this={versionNode}
            class="min-w-0 max-w-max grow-[1000] {held
              ? 'basis-[calc(4ch+1rem)]'
              : 'basis-[4ch]'} text-xs text-muted inline-flex items-center gap-1"
          >
            {#if meta.version}
              <span
                class="min-w-0 truncate"
                data-testid="mod-version"
                use:tooltip={{
                  text: installed.filename,
                  clippedText: `${meta.version} · ${installed.filename}`,
                }}>{meta.version}</span
              >
            {:else}
              <span
                class="min-w-0 truncate"
                data-testid="mod-state-note"
                use:tooltip={{
                  text: installed.filename,
                  clippedText: `${meta.note} · ${installed.filename}`,
                }}>{meta.note}</span
              >
            {/if}
            {@render heldPin('')}
          </span>
        {:else}
          <span class="text-xs text-muted flex-shrink-0 inline-flex items-center gap-1">
            <Icon name="user" size={12} />
            {summary.author}
            <Icon name="download" size={12} class="ml-1.5" />
            {formatCount($locale, summary.downloads ?? 0)}
          </span>
        {/if}
        {#if summary.summary}
          <span
            class="text-xs text-secondary flex-1 min-w-0 truncate border-l border-border-subtle pl-2"
            >{summary.summary}</span
          >
        {/if}
      </button>
      <div class={listBadgesClass}>{@render badges()}</div>
      <div class="flex items-center gap-1 flex-shrink-0">{@render iconActions()}</div>
    </CardShell>
  </ContextMenu>
{/if}
