<script lang="ts">
  import type { InstanceWithStatus, PlaytimeStats } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { formatDuration } from '$lib/format/duration';
  import { relativeTime } from '$lib/format/relative-time';
  import { isIntegrityStale } from '$lib/instances/integrity-freshness';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { hasDiagnosisIndicator } from '$lib/logs/log-diagnosis.svelte';
  import { t } from '$lib/i18n';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import InstanceHeader from './InstanceHeader.svelte';
  import AttentionPanel from './AttentionPanel.svelte';
  import ModpackCard from './ModpackCard.svelte';
  import { buildAttentionItems, type AttentionKind } from './attention';
  import { classifyExit } from './exit-status';

  type ErrorKey = 'offlineName' | 'listAccounts' | 'remove' | 'instances' | 'versions';

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
    onNavInstalled,
    onNavBrowse,
    onDismissError,
    onRetryError = () => {},
    versionsRetrying = false,
    onDismissInstallError,
    onDismissModsError,
    onOpenLogs,
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
    onManage: () => void;
    onExport: () => void;
    onOpenPackDrawer: () => void;
    onNavInstalled: () => void;
    onNavBrowse: () => void;
    onDismissError: (key: ErrorKey) => void;
    onRetryError?: (key: ErrorKey) => void;
    versionsRetrying?: boolean;
    onDismissInstallError: () => void;
    onDismissModsError: () => void;
    onOpenLogs: () => void;
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
        })
      : [],
  );

  function onAttention(kind: AttentionKind) {
    if (kind === 'log_issue') onOpenLogs();
    else if (kind === 'missing_mods' || kind === 'modpack_update') onOpenPackDrawer();
    else if (kind === 'incompatible') onNavInstalled();
    else onManage(); // pick_version + integrity
  }

  const ERROR_ORDER: ErrorKey[] = [
    'offlineName',
    'listAccounts',
    'remove',
    'instances',
    'versions',
  ];

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
            {$t('page.overview.errorRetry')}
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
    <InstanceHeader instance={activeInstance} {running} {installing} />

    <AttentionPanel items={attentionItems} onAction={onAttention} />

    <div class="grid gap-3" style="grid-template-columns:repeat(2,minmax(0,1fr));">
      <!-- Configuration -->
      <div class="rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col gap-2">
        <div class="text-[10px] uppercase tracking-wider text-muted">
          {$t('page.overview.sectionConfiguration')}
        </div>
        <div class="flex justify-between text-sm">
          <span class="text-muted">{$t('page.overview.labelMinecraft')}</span>
          <span class="font-mono">{activeInstance.mc_version || $t('page.overview.notSet')}</span>
        </div>
        <div class="flex justify-between text-sm">
          <span class="text-muted">{$t('page.overview.labelLoader')}</span>
          <span class="font-mono">
            {displayLoader(activeInstance.loader)}{#if activeInstance.loader_version}
              {' '}{activeInstance.loader_version}{/if}
          </span>
        </div>
        <div class="flex justify-between text-sm">
          <span class="text-muted">{$t('page.overview.labelMemory')}</span>
          <span class="font-mono">{activeInstance.max_heap_mb} {$t('format.unit.megabyte')}</span>
        </div>
        <button type="button" class="btn-tertiary self-start text-xs" onclick={onManage}>
          {$t('page.overview.manageBtn')}
        </button>
      </div>

      <!-- Mods -->
      <div class="rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col gap-2.5">
        <div class="text-[10px] uppercase tracking-wider text-muted">
          {$t('page.overview.sectionMods')}
        </div>
        {#if installedStats.total === 0}
          <p class="text-sm text-muted">
            {$t('page.overview.noModsHint')}
            <button type="button" class="btn-tertiary" onclick={onNavBrowse}>
              {$t('nav.modBrowser')}
            </button>
            {$t('page.overview.noModsHintSuffix')}
          </p>
        {:else}
          <div class="flex gap-4 text-sm">
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
          </div>
          <div class="flex gap-4">
            <button type="button" class="btn-tertiary text-xs" onclick={onNavInstalled}>
              {$t('page.overview.installedTab')}
            </button>
            <button type="button" class="btn-tertiary text-xs" onclick={onNavBrowse}>
              {$t('nav.modBrowser')}
            </button>
          </div>
          {#if installedStats.enabled >= 1}
            <button type="button" class="btn-secondary btn-sm self-start" onclick={onExport}>
              {$t('page.overview.exportModpack')}
            </button>
          {/if}
        {/if}
      </div>

      <!-- Modpack (pack instances only, full width) -->
      {#if activeInstance.mrpack_name}
        <div style="grid-column:1 / -1;">
          <ModpackCard instance={activeInstance} onOpenPack={onOpenPackDrawer} />
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

      <!-- Integrity -->
      <div
        class="rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col gap-2"
        data-testid="overview-integrity"
      >
        <div class="text-[10px] uppercase tracking-wider text-muted">
          {$t('instance.integrity.heading')}
        </div>
        {#if !activeInstance.integrity || isIntegrityStale(activeInstance.integrity)}
          <p class="text-sm text-muted">{$t('instance.integrity.statusNotChecked')}</p>
          <button type="button" class="btn-tertiary self-start text-xs" onclick={onManage}>
            {$t('instance.integrity.overviewOpenManage')}
          </button>
        {:else if activeInstance.integrity.healthy}
          <p class="text-sm text-success flex items-center gap-1.5">
            <Icon name="success" />
            {$t('instance.integrity.statusOk')}
          </p>
          {#if activeInstance.integrity.checked_unix_ms}
            <p class="text-xs text-muted">
              {$t('instance.integrity.checkedAt', {
                date: new Date(activeInstance.integrity.checked_unix_ms).toLocaleString(),
              })}
            </p>
          {/if}
        {:else}
          <p class="text-sm text-warning-text flex items-center gap-1.5">
            <Icon name="warning" class="text-warning-text" />
            {$t('instance.integrity.statusProblems', {
              count: activeInstance.integrity.problem_count,
            })}
          </p>
          <button type="button" class="btn-warning-soft btn-sm self-start" onclick={onManage}>
            {$t('instance.integrity.overviewOpenRepair')}
          </button>
        {/if}
      </div>
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
