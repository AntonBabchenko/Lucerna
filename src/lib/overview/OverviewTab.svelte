<script lang="ts">
  import type { InstanceWithStatus, PlaytimeStats } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { formatDuration } from '$lib/format/duration';
  import { relativeTime } from '$lib/format/relative-time';
  import { isIntegrityStale } from '$lib/instances/integrity-freshness';
  import type { ManageFocusField } from '$lib/instances/manage-focus';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { hasDiagnosisIndicator, diagnosisStatus } from '$lib/logs/log-diagnosis.svelte';
  import { t } from '$lib/i18n';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import InstanceHeader from './InstanceHeader.svelte';
  import AttentionPanel from './AttentionPanel.svelte';
  import ModpackCard from './ModpackCard.svelte';
  import { buildAttentionItems, type AttentionKind } from './attention';
  import { attentionCollapse } from './attention-collapse.svelte';
  import { classifyExit } from './exit-status';

  type ErrorKey = 'listAccounts' | 'remove' | 'instances' | 'versions';

  let {
    activeInstance,
    installedStats,
    playtime,
    incompatibleCount,
    missingModsCount,
    running,
    installing,
    exited,
    installError,
    modsError,
    errors,
    onManage,
    onExport,
    onOpenPackDrawer,
    onPackUpdated,
    onNavInstalled,
    onNavBrowse,
    onDismissError,
    onRetryError = () => {},
    versionsRetrying = false,
    onDismissInstallError,
    onDismissModsError,
    onOpenLogs,
    onOpenServers,
    onOptimise = () => {},
    optimiseResolving = false,
    onOpenLocalization,
    l10nPercent = null,
    l10nLang,
    l10nBadge = null,
    preflightUnknown = false,
  }: {
    activeInstance: InstanceWithStatus | null;
    installedStats: { total: number; enabled: number; disabled: number };
    playtime: PlaytimeStats;
    incompatibleCount: number;
    missingModsCount: number;
    running: boolean;
    installing: boolean;
    exited: { code: number; user_requested: boolean; log_path: string } | null;
    installError: string | null;
    modsError: string | null;
    errors: Record<ErrorKey, string | null>;
    onManage: (field?: ManageFocusField | null) => void;
    onExport: () => void;
    onOpenPackDrawer: () => void;
    onPackUpdated?: () => void;
    onNavInstalled: (filter?: 'incompatible') => void;
    onNavBrowse: () => void;
    onDismissError: (key: ErrorKey) => void;
    onRetryError?: (key: ErrorKey) => void;
    versionsRetrying?: boolean;
    onDismissInstallError: () => void;
    onDismissModsError: () => void;
    onOpenLogs: () => void;
    onOpenServers: () => void;
    onOptimise?: () => void;
    optimiseResolving?: boolean;
    onOpenLocalization: () => void;
    l10nPercent?: number | null;
    /** Target language the coverage percent was measured against, e.g.
     *  `"ru_ru"` — the shared selection owned by +page.svelte (see
     *  LocalizationModal's `lang` prop). Always shown so the number can
     *  never be mistaken for "coverage into whatever the UI happens to be
     *  in right now". */
    l10nLang: string;
    /** `'not_applied'` / `'outdated'` when this instance's translation pack
     *  needs attention, `null` when there is nothing to say. Fed from the same
     *  apply-targets data the "apply in other instances" offer uses, so the two
     *  surfaces can never disagree about an instance's state. */
    l10nBadge?: 'not_applied' | 'outdated' | null;
    /** The dependency pre-flight could not be run for this instance. Owned by
     *  the page, which is where the launch gate runs it. Never blocks a launch
     *  — it only stops "could not check" from reading as "checked and clean". */
    preflightUnknown?: boolean;
  } = $props();

  // An unhealthy integrity result is always an actionable problem (never
  // "stale"); a healthy-but-stale or absent result is shown as "not checked"
  // in the Integrity card, not as an attention problem.
  const integrityProblemCount = $derived(
    activeInstance?.integrity && !activeInstance.integrity.healthy
      ? activeInstance.integrity.problem_count
      : 0,
  );

  const attentionItems = $derived(
    activeInstance
      ? buildAttentionItems({
          mcVersionMissing: activeInstance.mc_version === '',
          missingModsCount,
          incompatibleCount,
          integrityProblemCount,
          hasModpackUpdate: modpackUpdates.hasUpdate(activeInstance.id),
          hasLogIssue: hasDiagnosisIndicator(),
          logFixAvailable: diagnosisStatus() === 'actionable',
          serverFixAvailable: serverState.anyDiagnosisActionable,
          preflightUnknown,
        })
      : [],
  );

  // The loader text of the Configuration row, rendered by the visible span and
  // spoken by the row's aria-label. One source of truth so the accessible name
  // can never drift from what a sighted user reads.
  const loaderDisplay = $derived(
    activeInstance
      ? displayLoader(activeInstance.loader) +
          (activeInstance.loader_version ? ` ${activeInstance.loader_version}` : '')
      : '',
  );

  // Sticky per-instance "I dismissed the panel" flag (persisted). Stays
  // collapsed until the user restores it via the header triangle, even if the
  // set of warnings changes.
  const attentionCollapsed = $derived(
    activeInstance ? attentionCollapse.isCollapsed(activeInstance.id) : false,
  );

  function onAttention(kind: AttentionKind) {
    if (kind === 'log_issue' || kind === 'log_fix') onOpenLogs();
    else if (kind === 'missing_mods' || kind === 'modpack_update') onOpenPackDrawer();
    else if (kind === 'incompatible') onNavInstalled('incompatible');
    // The dependency panel and «Перепроверить зависимости» both live on the
    // Installed tab, and opening it re-attempts the pre-flight.
    else if (kind === 'preflight_unknown') onNavInstalled();
    else if (kind === 'server_log_fix') onOpenServers();
    else if (kind === 'integrity') onManage('integrity');
    else onManage('mc'); // pick_version
  }

  const ERROR_ORDER: ErrorKey[] = ['listAccounts', 'remove', 'instances', 'versions'];

  // Only network-backed errors are worth a Reload button; the rest are local
  // (filesystem) reads with nothing to refetch.
  const RETRYABLE: ErrorKey[] = ['versions'];
</script>

<div class="p-6 flex flex-col gap-4">
  {#each ERROR_ORDER as key (key)}
    {#if errors[key]}
      <p class="text-xs text-danger flex items-center gap-1">
        {errors[key]}
        {#if RETRYABLE.includes(key)}
          <button
            type="button"
            class="btn-tertiary text-xs"
            disabled={versionsRetrying}
            onclick={() => onRetryError(key)}
          >
            {#if versionsRetrying}
              <Spinner size="sm" labelPlacement="right" label={$t('page.overview.errorRetry')} />
            {:else}
              {$t('page.overview.errorRetry')}
            {/if}
          </button>
        {/if}
        <CloseButton
          onClick={() => onDismissError(key)}
          ariaLabel={$t('page.overview.dismissError')}
        />
      </p>
    {/if}
  {/each}

  {#if activeInstance}
    <InstanceHeader
      instance={activeInstance}
      {running}
      {installing}
      {attentionCollapsed}
      attentionCount={attentionItems.length}
      onShowAttention={() =>
        activeInstance && attentionCollapse.setCollapsed(activeInstance.id, false)}
    />

    {#if !attentionCollapsed}
      <AttentionPanel
        items={attentionItems}
        onAction={onAttention}
        onDismiss={() => activeInstance && attentionCollapse.setCollapsed(activeInstance.id, true)}
      />
    {/if}

    <div class="grid gap-3" style="grid-template-columns:repeat(2,minmax(0,1fr));">
      <!--
        Configuration. The card carries no padding of its own: each zone
        button carries it, so the buttons tile the whole card surface and any
        point the user aims at is live. A <button> cannot nest another, so
        "the whole card is clickable" has to be expressed this way.
      -->
      <div class="rounded-xl border border-border-subtle bg-surface overflow-hidden flex flex-col">
        <button
          type="button"
          class="card-zone px-3.5 pt-3.5 pb-2 text-[10px] uppercase tracking-wider text-muted"
          aria-label={$t('page.overview.openManageAria')}
          data-testid="overview-config-header"
          onclick={() => onManage(null)}
        >
          {$t('page.overview.sectionConfiguration')}
        </button>
        <button
          type="button"
          class="card-zone px-3.5 py-1 flex justify-between gap-3 text-sm"
          aria-label={$t('page.overview.editFieldAria', {
            field: $t('page.overview.labelMinecraft'),
            value: activeInstance.mc_version || $t('page.overview.notSet'),
          })}
          data-testid="overview-config-mc"
          onclick={() => onManage('mc')}
        >
          <span class="text-muted">{$t('page.overview.labelMinecraft')}</span>
          <span class="font-mono">{activeInstance.mc_version || $t('page.overview.notSet')}</span>
        </button>
        <button
          type="button"
          class="card-zone px-3.5 py-1 flex justify-between gap-3 text-sm"
          aria-label={$t('page.overview.editFieldAria', {
            field: $t('page.overview.labelLoader'),
            value: loaderDisplay,
          })}
          data-testid="overview-config-loader"
          onclick={() => onManage('loader')}
        >
          <span class="text-muted">{$t('page.overview.labelLoader')}</span>
          <span class="font-mono">{loaderDisplay}</span>
        </button>
        <button
          type="button"
          class="card-zone px-3.5 pt-1 pb-3.5 flex justify-between gap-3 text-sm"
          aria-label={$t('page.overview.editFieldAria', {
            field: $t('page.overview.labelMemory'),
            value: `${activeInstance.max_heap_mb} ${$t('format.unit.megabyte')}`,
          })}
          data-testid="overview-config-memory"
          onclick={() => onManage('memory')}
        >
          <span class="text-muted">{$t('page.overview.labelMemory')}</span>
          <span class="font-mono">{activeInstance.max_heap_mb} {$t('format.unit.megabyte')}</span>
        </button>
      </div>

      <!--
        Mods. Header + body are navigation zones (Installed, or the browser
        when nothing is installed yet); Optimise and Export are real actions
        and stay as buttons in their own row below the clickable surface.

        Only the eyebrow header carries an aria-label — on its own the bare word
        "MODS" is a poor accessible name. The body zones must not: an aria-label
        replaces the content, and their content (the live Total/Enabled/Disabled
        counts, or the empty-state sentence) is what a screen-reader user needs
        to hear.
      -->
      <div class="rounded-xl border border-border-subtle bg-surface overflow-hidden flex flex-col">
        <button
          type="button"
          class="card-zone px-3.5 pt-3.5 pb-2 text-[10px] uppercase tracking-wider text-muted"
          aria-label={installedStats.total === 0
            ? $t('page.overview.openBrowseAria')
            : $t('page.overview.openInstalledAria')}
          data-testid="overview-mods-header"
          onclick={() => (installedStats.total === 0 ? onNavBrowse() : onNavInstalled())}
        >
          {$t('page.overview.sectionMods')}
        </button>
        {#if installedStats.total === 0}
          <button
            type="button"
            class="card-zone px-3.5 pb-2 text-sm text-muted"
            data-testid="overview-mods-empty"
            onclick={onNavBrowse}
          >
            {$t('page.overview.noModsHint')}
          </button>
        {:else}
          <button
            type="button"
            class="card-zone px-3.5 pb-2 flex gap-4 text-sm"
            data-testid="overview-mods-stats"
            onclick={() => onNavInstalled()}
          >
            <span
              >{$t('page.overview.statsTotal')}
              <span class="font-medium text-secondary">{installedStats.total}</span></span
            >
            <span
              >{$t('page.overview.statsEnabled')}
              <span class="font-medium text-success">{installedStats.enabled}</span></span
            >
            <span
              >{$t('page.overview.statsDisabled')}
              <span class="font-medium text-secondary">{installedStats.disabled}</span></span
            >
          </button>
        {/if}
        {#if activeInstance?.loader === 'vanilla' && installedStats.enabled > 0}
          <!-- Instance-level condition (spec D9): Vanilla loads no mods at
               all, and per-jar verdicts correctly stay silent on it — so the
               card says it outright. Click-through to the Installed tab,
               which carries the same banner. -->
          <button
            type="button"
            class="card-zone px-3.5 pb-2 text-left text-sm text-warning-text"
            data-testid="overview-vanilla-mods-warning"
            onclick={() => onNavInstalled()}
          >
            {$t('instance.integrity.vanillaModsWarning', { count: installedStats.enabled })}
          </button>
        {/if}
        <!-- Translation coverage. A third card-zone row rather than a fourth
             Overview card: localization is a property of the mods, and this
             card is already a stack of zones. -->
        {#if installedStats.total > 0}
          <button
            type="button"
            class="card-zone px-3.5 pb-2 flex justify-between gap-3 text-sm"
            data-testid="overview-localization"
            onclick={onOpenLocalization}
          >
            <span>{$t('page.overview.localization', { lang: l10nLang })}</span>
            <span class="flex items-center gap-2">
              <span class="font-medium text-secondary"
                >{l10nPercent === null || l10nPercent === undefined
                  ? '—'
                  : $t('page.overview.localizationValue', { percent: l10nPercent })}</span
              >
              <!-- The percent says how much is translated; this says whether any
                   of it is actually live in this instance. Amber because it is a
                   standing "you are missing something", not an error. -->
              {#if l10nBadge}
                <StatusBadge variant="warning" testid="l10n-badge"
                  >{$t(
                    l10nBadge === 'outdated'
                      ? 'instance.l10n.targets.stateOutdated'
                      : 'instance.l10n.targets.stateNotApplied',
                  )}</StatusBadge
                >
              {/if}
            </span>
          </button>
        {/if}
        <div class="px-3.5 pt-1 pb-3.5 flex flex-wrap gap-2">
          {#if installedStats.enabled >= 1}
            <button type="button" class="btn-secondary btn-sm" onclick={onExport}>
              {$t('page.overview.exportModpack')}
            </button>
          {/if}
          <!-- One-click Optimise: install a curated performance-mod set. Disabled
               on vanilla (no loader to run the mods). -->
          <button
            type="button"
            class="btn-secondary btn-sm"
            disabled={activeInstance.loader === 'vanilla' || optimiseResolving}
            title={activeInstance.loader === 'vanilla' ? $t('optimise.vanillaTooltip') : undefined}
            data-testid="optimise-btn"
            onclick={onOptimise}
          >
            {optimiseResolving ? $t('optimise.resolving') : $t('optimise.button')}
          </button>
        </div>
      </div>

      <!-- Modpack (pack instances only, full width) -->
      {#if activeInstance.mrpack_name}
        <div style="grid-column:1 / -1;">
          <ModpackCard
            instance={activeInstance}
            onOpenPack={onOpenPackDrawer}
            onUpdated={onPackUpdated}
          />
        </div>
      {/if}

      <!-- Playtime -->
      <div
        class="rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col gap-2"
        data-testid="overview-playtime"
      >
        <div class="text-[10px] uppercase tracking-wider text-muted">
          {$t('page.overview.sectionPlaytime')}
        </div>
        {#if playtime.last_session_unix_ms == null}
          <p class="text-sm text-muted">{$t('page.overview.notYetPlayed')}</p>
        {:else}
          <div class="text-sm">
            {$t('page.overview.playtimeTotal')}
            <span class="font-medium text-primary"
              >{formatDuration($t, playtime.total_seconds)}</span
            >
            · <span>{$t('page.overview.playtimeSession', { count: playtime.session_count })}</span>
          </div>
          <p class="text-xs text-muted">
            {$t('page.overview.playtimeLastSession', {
              duration: formatDuration($t, playtime.last_session_seconds),
              when: relativeTime($t, playtime.last_session_unix_ms),
            })}
          </p>
        {/if}
      </div>

      <!--
        Integrity. One zone for the whole card in all three states — the repair
        action itself lives in IntegritySection inside Manage, and unhealthy
        instances are additionally surfaced by AttentionPanel. Children are
        <span>s: a <button> may only contain phrasing content.

        Deliberately no aria-label: the heading plus the current status is the
        accessible name, so all three states stay audible. A fixed label would
        have announced "check integrity" even on a healthy, already-checked
        instance.
      -->
      <button
        type="button"
        class="card-zone rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col items-start gap-2"
        data-testid="overview-integrity"
        onclick={() => onManage('integrity')}
      >
        <span class="text-[10px] uppercase tracking-wider text-muted">
          {$t('instance.integrity.heading')}
        </span>
        {#if !activeInstance.integrity || isIntegrityStale(activeInstance.integrity)}
          <span class="text-sm text-muted">{$t('instance.integrity.statusNotChecked')}</span>
        {:else if activeInstance.integrity.healthy}
          <span class="text-sm text-success flex items-center gap-1.5">
            <Icon name="success" />
            {$t('instance.integrity.statusOk')}
          </span>
          {#if activeInstance.integrity.checked_unix_ms}
            <span class="text-xs text-muted">
              {$t('instance.integrity.checkedAt', {
                date: new Date(activeInstance.integrity.checked_unix_ms).toLocaleString(),
              })}
            </span>
          {/if}
        {:else}
          <span class="text-sm text-warning-text flex items-center gap-1.5">
            <Icon name="warning" class="text-warning-text" />
            {$t('instance.integrity.statusProblems', {
              count: activeInstance.integrity.problem_count,
            })}
          </span>
        {/if}
      </button>
    </div>

    <!-- Footer notices: install/mods errors + last exit code -->
    {#if installError || modsError || (exited && !running)}
      <div class="flex flex-col gap-1">
        {#if installError}
          <span class="text-xs text-danger flex items-center gap-1">
            {installError}
            <CloseButton
              onClick={onDismissInstallError}
              ariaLabel={$t('page.overview.dismissError')}
            />
          </span>
        {/if}
        {#if modsError}
          <span class="text-xs text-danger flex items-center gap-1">
            {modsError}
            <CloseButton
              onClick={onDismissModsError}
              ariaLabel={$t('page.overview.dismissError')}
            />
          </span>
        {/if}
        {#if exited && !running}
          {@const status = classifyExit(exited)}
          <span class="text-xs text-secondary">
            {#if status.kind === 'stopped'}
              {$t('page.overview.statusStopped')}
            {:else if status.kind === 'clean'}
              {$t('page.overview.statusClean')}
            {:else}
              {$t('page.overview.statusExited', { code: status.code })}
            {/if}
          </span>
        {/if}
      </div>
    {/if}
  {:else}
    <p class="text-sm text-muted">{$t('page.overview.noInstanceSelected')}</p>
  {/if}
</div>
