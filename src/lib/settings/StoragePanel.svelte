<script lang="ts">
  // Storage settings panel — Task 20 of the v0.5.0 mod browser plan.
  // Rendered inside SettingsModal's Storage tab.
  //
  // Shows the current on-disk size of the shared mod cache (under the
  // launcher's app-data dir) and offers a Clear button. Clearing only
  // wipes the cache jars — installed mods inside instances are not
  // touched (re-installs will re-download from CurseForge / Modrinth).
  //
  // Both IPC calls (modsCacheSizeBytes / modsClearCache) follow the
  // result-status pattern (typedError) — no try/catch around them.
  //
  // The "Data location" block (bottom) lets the user relocate the WHOLE data root (instances,
  // caches, accounts — everything under the app-data dir) to a folder of their choice. This panel
  // only PLANS (backend classification), CONFIRMS (DataLocationConfirmDialog) and STARTS a move.
  // The progress / final dialog is owned by the app-level DataMoveHost, driven by the shared
  // data-location.svelte.ts rune (also read by the fallback banner and the create/Play gating in
  // +page.svelte), so it survives a reload.
  import { open as openDirectory } from '@tauri-apps/plugin-dialog';
  import { tick } from 'svelte';
  import { commands, type LogRetentionPolicy } from '$lib/ipc/bindings';
  import { describeStoreError, formatError } from '$lib/ipc/format-error';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import {
    appSettings,
    generalDisplayed,
    loadAppSettings,
    patchGeneral,
    saveFailure,
  } from '$lib/settings/app-settings.svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { createRestartGate, type RestartGate } from './restart-gate.svelte';
  import { fallbackDetails } from './fallback-message';
  import DataLocationConfirmDialog from '$lib/settings/DataLocationConfirmDialog.svelte';
  import SettingsField from './SettingsField.svelte';

  let bytes = $state<number | null>(null);
  let clearing = $state(false);
  let error = $state<string | null>(null);

  const DEFAULT_RETENTION: Required<LogRetentionPolicy> = {
    enabled: false,
    max_files: 10,
    max_total_mb: 100,
  };
  // Retention and the cache TTL are two fields on the one settings contract.
  // The editable copies are seeded once from the confirmed block — not on every
  // patch, or a number being typed would be overwritten by its own echo — and
  // go back to the confirmed value when a save is refused.
  const general = $derived(generalDisplayed());
  const settingsLoaded = $derived(general !== null);
  const loadError = $derived(
    appSettings.loaded.kind === 'failed' ? appSettings.loaded.error : null,
  );
  let retention = $state<Required<LogRetentionPolicy>>({ ...DEFAULT_RETENTION });
  let retentionSaving = $state(false);
  const retentionError = $derived(saveFailure('log_retention'));

  const DEFAULT_TTL_DAYS = 7;
  let modTtlDays = $state<number>(DEFAULT_TTL_DAYS);
  let ttlSaving = $state(false);
  const ttlError = $derived(saveFailure('mod_metadata_ttl_days'));

  let seeded = $state(false);
  $effect(() => {
    if (general && !seeded) {
      retention = { ...DEFAULT_RETENTION, ...general.log_retention };
      modTtlDays = general.mod_metadata_ttl_days ?? DEFAULT_TTL_DAYS;
      seeded = true;
    }
  });

  async function saveRetentionTracked() {
    retentionSaving = true;
    try {
      await saveRetention();
    } finally {
      retentionSaving = false;
    }
  }

  async function saveRetention() {
    const snap = { ...retention }; // snapshot before await
    const r = await patchGeneral({
      log_retention: {
        enabled: snap.enabled,
        max_files: Number.isFinite(snap.max_files as number)
          ? Math.max(0, Math.trunc(snap.max_files as number))
          : DEFAULT_RETENTION.max_files,
        max_total_mb: Number.isFinite(snap.max_total_mb as number)
          ? Math.max(1, Math.trunc(snap.max_total_mb as number))
          : DEFAULT_RETENTION.max_total_mb,
      },
    });
    // Back to what is saved: the message under the field says why.
    if (!r.ok && general) retention = { ...DEFAULT_RETENTION, ...general.log_retention };
  }

  async function saveTtlTracked() {
    ttlSaving = true;
    try {
      await saveTtl();
    } finally {
      ttlSaving = false;
    }
  }

  async function saveTtl() {
    const snap = Number.isFinite(modTtlDays)
      ? Math.max(0, Math.trunc(modTtlDays))
      : DEFAULT_TTL_DAYS;
    const r = await patchGeneral({ mod_metadata_ttl_days: snap });
    if (!r.ok && general) modTtlDays = general.mod_metadata_ttl_days ?? DEFAULT_TTL_DAYS;
  }

  async function refresh() {
    const result = await commands.modsCacheSizeBytes();
    if (result.status === 'ok') {
      // The IPC contract types this as `number | null`. Treat null as
      // "unknown" so the UI still has a sensible fallback rather than
      // rendering "null B".
      bytes = result.data ?? 0;
    } else {
      error = formatError(result.error);
    }
  }

  // Data-root size, loaded lazily through its own async command — split from getDataLocation so
  // the full-tree walk (seconds on a cold FS cache) never runs on the startup path, only when
  // this panel is open. Three states: loading, known (a measured 0 is a real "0 B"), unknown
  // (null — the call failed or answered null). Unknown is never rendered as a size.
  let dataRootSize = $state<number | null>(null);
  let dataRootSizeLoading = $state(true);
  let dataRootSizeError = $state<string | null>(null);

  async function refreshDataRootSize() {
    dataRootSizeLoading = true;
    dataRootSizeError = null;
    const result = await commands.dataRootSizeBytes();
    dataRootSizeLoading = false;
    if (result.status === 'ok') {
      dataRootSize = bytesOrNull(result.data);
    } else {
      dataRootSize = null;
      dataRootSizeError = formatError(result.error);
    }
  }

  $effect(() => {
    void refresh();
    void dataLocation.init();
    void refreshDataRootSize();
    void recheckBlocked();
  });

  // ── Data-root relocation ────────────────────────────────────────────────
  type Bytes = number | null;
  type PendingTarget =
    | { kind: 'adopt'; path: string }
    | { kind: 'move'; path: string; requiredBytes: Bytes; freeBytes: Bytes }
    | { kind: 'reset'; path: string; pointerOnly: boolean; requiredBytes: Bytes; freeBytes: Bytes };

  // null while no confirm dialog is open; otherwise the backend's plan.
  let pendingTarget = $state<PendingTarget | null>(null);
  // True from picker open / reset click until the plan settles. Guards the window where the OS
  // dialog is gone but the plan command (fs probes that can stall on a flaky drive) hasn't
  // resolved: without it a second pick could race the first and silently swap an open confirm
  // dialog's target between the user reading it and clicking confirm. Shared by both buttons.
  let planning = $state(false);
  /** Which button shows the spinner while `planning`. */
  let planningReset = $state(false);
  /** A redirect-only commit (adopt, pointer-only reset) is in flight: the confirm dialog stays up
   *  with its busy spinner — a sub-second redirect write has no copy and no progress to show. */
  let committing = $state(false);
  let migrationError = $state<string | null>(null);
  let moveNotice = $state<string | null>(null);
  let resetBlockers = $state<{ path: string; entries: string[] } | null>(null);
  let sectionEl = $state<HTMLDivElement | null>(null);

  // The move is offered only when the backend said exactly 'none' (spec §4.7): a pending call, a
  // rejection, null or a token this build does not know all count as "could not tell", which is
  // the blocked answer.
  // The gate's core (checking / exact 'none' / newer answer wins) is shared with
  // Settings → Updates through createRestartGate; the sentences and the moments
  // to ask stay here.
  const gate = createRestartGate();
  const restartBlock = $derived(gate.block);
  const BLOCK_REASON_KEYS: Record<Exclude<RestartGate, 'none'>, TranslationKey> = {
    checking: 'settings.storage.dataLocation.blocked.checking',
    running: 'settings.storage.dataLocation.blocked.running',
    busy: 'settings.storage.dataLocation.blocked.busy',
    unknown: 'settings.storage.dataLocation.blocked.unknown',
  };
  const moveBlocked = $derived(restartBlock !== 'none');
  const blockedReason = $derived(
    restartBlock === 'none' ? null : $t(BLOCK_REASON_KEYS[restartBlock]),
  );

  async function recheckBlocked() {
    await gate.recheck();
  }

  /** specta renders f64 as `number | null`; treat anything else as unknown. */
  function bytesOrNull(v: number | null | undefined): Bytes {
    return typeof v === 'number' && Number.isFinite(v) && v >= 0 ? v : null;
  }

  /** With nothing seeded in a recovery session there is no instance, so the Logs popover has
   *  nothing to list — and the launcher log is the one document that says WHY. */
  async function openLogFolder() {
    const r = await commands.openLauncherLogFolder();
    if (r.status === 'error') migrationError = formatError(r.error);
  }

  function clearMoveMessages() {
    migrationError = null;
    moveNotice = null;
    resetBlockers = null;
  }

  async function pickLocation() {
    if (planning || moveBlocked) return;
    planning = true;
    clearMoveMessages();
    try {
      // Open the picker at the current data root's PARENT rather than wherever
      // the last OS dialog left off (which could be an unrelated folder such
      // as .minecraft/saves from an earlier world import).
      // In a recovery session `effective` is the throwaway session root: start from the
      // configured folder's parent instead — the drive it moved from is the likeliest pick.
      const current = dataLocation.fellBack
        ? dataLocation.status?.configured
        : dataLocation.status?.effective;
      const defaultPath = current?.replace(/[\\/][^\\/]+[\\/]?$/, '') || undefined;
      const picked = await openDirectory({ directory: true, defaultPath });
      if (!picked || typeof picked !== 'string') return;
      // The backend classifies the pick: an existing Lucerna root (the picked
      // folder itself or its LucernaData child) becomes an adopt offer;
      // anything else resolves to the effective migration target with the
      // LucernaData subfolder applied exactly once. The frontend builds no
      // paths — frontend-side appending is how picking an existing root used
      // to nest LucernaData\LucernaData and abandon the real data.
      const plan = await commands.planDataLocationChange(picked);
      if (plan.status !== 'ok') {
        // Includes the links refusal — shown here, BEFORE any confirm dialog.
        migrationError = formatError(plan.error);
        void recheckBlocked();
        return;
      }
      if (plan.data.kind === 'already_current') {
        migrationError = $t('settings.storage.dataLocation.alreadyCurrent');
        return;
      }
      pendingTarget =
        plan.data.kind === 'adopt'
          ? { kind: 'adopt', path: plan.data.path }
          : {
              kind: 'move',
              path: plan.data.path,
              requiredBytes: bytesOrNull(plan.data.required_bytes),
              freeBytes: bytesOrNull(plan.data.free_bytes),
            };
    } finally {
      planning = false;
    }
  }

  async function requestReset() {
    if (planning || moveBlocked) return;
    planning = true;
    planningReset = true;
    clearMoveMessages();
    try {
      const plan = await commands.planDataLocationReset();
      if (plan.status !== 'ok') {
        migrationError = formatError(plan.error);
        void recheckBlocked();
        return;
      }
      if (plan.data.blocking_entries.length > 0) {
        // Leftovers of an earlier move still sit in the default folder. A reset cannot succeed
        // until the user removes them — list them and open no dialog that would promise otherwise.
        resetBlockers = { path: plan.data.path, entries: plan.data.blocking_entries };
        return;
      }
      const pointerOnly = plan.data.pointer_only;
      pendingTarget = {
        kind: 'reset',
        path: plan.data.path,
        pointerOnly,
        requiredBytes: pointerOnly ? null : bytesOrNull(plan.data.required_bytes),
        freeBytes: pointerOnly ? null : bytesOrNull(plan.data.free_bytes),
      };
    } finally {
      planning = false;
      planningReset = false;
    }
  }

  function cancelPending() {
    pendingTarget = null;
  }

  async function confirmPending() {
    const target = pendingTarget;
    if (target === null || committing) return;
    migrationError = null;
    moveNotice = null;
    if (target.kind === 'adopt' || (target.kind === 'reset' && target.pointerOnly)) {
      committing = true;
    } else {
      // A real copy: hand the screen to the app-level dialog ("Preparing…") now, before the first
      // progress tick can arrive.
      dataLocation.moveStarted();
      pendingTarget = null;
    }
    let result:
      | Awaited<ReturnType<typeof commands.setDataLocation>>
      | Awaited<ReturnType<typeof commands.adoptDataLocation>>
      | null = null;
    let thrown: string | null = null;
    try {
      result =
        target.kind === 'adopt'
          ? await commands.adoptDataLocation(target.path)
          : await commands.setDataLocation(target.kind === 'reset' ? null : target.path);
    } catch (e) {
      // typedError rethrows real Error instances (a transport-level IPC failure). Swallowing the
      // settle below would leave the store "owned" forever: a bare "Preparing…" dialog with no
      // Cancel and no Restart. `moveSettled` re-reads the backend, so if the move DID start or
      // finish behind the failed call, the app-level dialog shows that instead.
      thrown = describeStoreError(e);
    }
    // A clean move or adopt never returns — the backend restarts the app. Reaching here means it
    // failed, was cancelled, or switched folders and now needs a restart. Read the outcome BEFORE
    // resetting anything: `restart_required` must leave the app-level final dialog standing.
    const outcome = target.kind !== 'adopt' && result?.status === 'ok' ? result.data : null;
    await dataLocation.moveSettled(outcome);
    committing = false;
    pendingTarget = null;
    if (outcome?.kind === 'restart_required') return;
    if (thrown !== null) migrationError = thrown;
    else if (result?.status === 'error') migrationError = formatError(result.error);
    else if (outcome?.kind === 'cancelled')
      moveNotice = $t('settings.storage.dataLocation.moveCancelled');
    // The attempt may have touched disk, and what blocks a move may have changed while it ran —
    // re-measure and re-ask.
    void refreshDataRootSize();
    await recheckBlocked();
    // The blocking dialog's opener (the confirm button) is gone, so focus fell to <body>, outside
    // SettingsModal's panel-scoped Tab handler. Give it back.
    await tick();
    sectionEl?.focus();
  }

  async function clear() {
    clearing = true;
    error = null;
    const result = await commands.modsClearCache();
    if (result.status === 'ok') {
      const freed = result.data ?? 0;
      // Route through the global toast system (auto-dismiss + live region)
      // instead of the old hand-rolled inline box that never went away.
      pushSuccess(
        $t('settings.storage.cleared', {
          freed: formatSize($t, freed) || $t('format.size.bytes', { n: 0 }),
        }),
      );
      await refresh();
    } else {
      error = formatError(result.error);
    }
    clearing = false;
  }
</script>

<div>
  <SettingsField anchor="storage.cache">
    <div class="text-sm mb-2">
      {$t('settings.storage.cacheLabel')}
      <span class="font-medium"
        >{bytes === null ? '…' : formatSize($t, bytes) || $t('format.size.bytes', { n: 0 })}</span
      >
    </div>
    <p class="text-xs text-muted mb-3">
      {$t('settings.storage.cacheDescription')}
    </p>

    {#if error}
      <div
        class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2"
        role="alert"
      >
        {error}
      </div>
    {/if}
    <BusyButton
      type="button"
      class="btn-secondary btn-sm"
      busy={clearing}
      disabled={bytes === 0 || bytes === null}
      onclick={clear}
    >
      {$t('settings.storage.clearBtn')}
    </BusyButton>
  </SettingsField>

  <SettingsField anchor="storage.logRetention">
    <div class="flex flex-col gap-3 border-t mt-4 pt-4">
      <h3 class="font-medium text-sm text-primary">
        {$t('settings.general.logRetention.title')}
      </h3>
      {#if loadError !== null}
        <div class="flex flex-wrap items-center gap-2" data-testid="settings-load-failed">
          <StatusMessage
            message={$t('settings.general.loadFailed', { error: loadError })}
            tone="danger"
          />
          <button type="button" class="btn-secondary btn-sm" onclick={() => void loadAppSettings()}>
            {$t('settings.general.retryBtn')}
          </button>
        </div>
      {/if}
      <div data-testid="save-failure-log_retention">
        <StatusMessage message={retentionError} tone="danger" />
      </div>
      <label class="flex items-start gap-2 cursor-pointer">
        <input
          type="checkbox"
          class="mt-0.5"
          bind:checked={retention.enabled}
          disabled={!settingsLoaded}
          onchange={() => void saveRetentionTracked()}
          data-testid="log-retention-toggle"
        />
        <span class="flex-1">
          <span class="text-sm text-primary">{$t('settings.general.logRetention.enableLabel')}</span
          >
          <span class="block text-xs text-muted">
            {$t('settings.general.logRetention.enableDescription')}
          </span>
        </span>
      </label>
      <div class="flex flex-wrap items-end gap-4 pl-6">
        {#if retentionSaving}
          <div class="flex items-center text-xs text-secondary">
            <Spinner size="sm" />
          </div>
        {/if}
        <label class="flex flex-col gap-1">
          <span class="text-xs text-primary">{$t('settings.general.logRetention.keepLabel')}</span>
          <input
            type="number"
            min="0"
            class="border rounded px-2 py-1 text-sm w-28"
            bind:value={retention.max_files}
            disabled={!settingsLoaded || !retention.enabled}
            onchange={() => void saveRetentionTracked()}
            data-testid="log-retention-max-files"
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-xs text-primary">{$t('settings.general.logRetention.sizeLabel')}</span>
          <input
            type="number"
            min="1"
            class="border rounded px-2 py-1 text-sm w-28"
            bind:value={retention.max_total_mb}
            disabled={!settingsLoaded || !retention.enabled}
            onchange={() => void saveRetentionTracked()}
            data-testid="log-retention-max-mb"
          />
        </label>
      </div>
    </div>
  </SettingsField>

  <SettingsField anchor="storage.modMetadataCache">
    <div class="flex flex-col gap-3 border-t mt-4 pt-4">
      <h3 class="font-medium text-sm text-primary">
        {$t('settings.general.modMetadataCache.title')}
      </h3>
      <p class="text-xs text-muted">{$t('settings.general.modMetadataCache.description')}</p>
      <div data-testid="save-failure-mod_metadata_ttl_days">
        <StatusMessage message={ttlError} tone="danger" />
      </div>
      <div class="flex flex-wrap items-end gap-4">
        {#if ttlSaving}
          <div class="flex items-center text-xs text-secondary">
            <Spinner size="sm" />
          </div>
        {/if}
        <label class="flex flex-col gap-1">
          <span class="text-xs text-primary">
            {$t('settings.general.modMetadataCache.ttlLabel')}
          </span>
          <input
            type="number"
            min="0"
            step="1"
            class="border rounded px-2 py-1 text-sm w-28"
            bind:value={modTtlDays}
            disabled={!settingsLoaded}
            onchange={() => void saveTtlTracked()}
            data-testid="mod-metadata-ttl-days"
          />
        </label>
      </div>
      <p class="text-xs text-muted">{$t('settings.general.modMetadataCache.ttlHint')}</p>
    </div>
  </SettingsField>

  <SettingsField anchor="storage.dataLocation">
    <div
      bind:this={sectionEl}
      tabindex="-1"
      class="flex flex-col gap-3 border-t mt-4 pt-4 outline-none"
      data-testid="data-location-section"
    >
      <h3 class="font-medium text-sm text-primary">
        {$t('settings.storage.dataLocation.heading')}
      </h3>
      <p class="text-xs text-muted">{$t('settings.storage.dataLocation.description')}</p>

      {#if dataLocation.error}
        <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2">
          {dataLocation.error}
        </div>
      {/if}
      {#if migrationError}
        <div
          class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 selectable"
          role="alert"
        >
          {migrationError}
        </div>
      {/if}

      {#if dataLocation.fallback}
        <!-- Only the ways back: the banner already said WHY (and the modal-level notice that
             settings are temporary). The OS error, when there is one, belongs here — not in a
             banner read at a glance. -->
        <div
          class="rounded-xl border border-warning-text bg-warning-bg p-3 text-sm text-warning-text flex flex-col gap-2"
          role="alert"
          data-testid="data-location-recovery-notice"
        >
          <p>{$t('settings.storage.dataLocation.fallbackNotice')}</p>
          {#if fallbackDetails(dataLocation.fallback)}
            <p class="font-mono text-xs selectable">
              {$t('settings.storage.dataLocation.fallbackDetails', {
                details: fallbackDetails(dataLocation.fallback),
              })}
            </p>
          {/if}
          <div>
            <button
              type="button"
              class="btn-secondary btn-sm inline-flex items-center gap-1.5"
              onclick={() => void openLogFolder()}
            >
              <Icon name="folderOpen" size={14} />
              {$t('settings.storage.dataLocation.openLogFolderBtn')}
            </button>
          </div>
        </div>
      {/if}

      {#if resetBlockers}
        <div
          class="rounded border border-warning-text/40 bg-warning-bg p-3 text-sm text-warning-text"
          role="alert"
          data-testid="data-reset-blockers"
        >
          <p>
            {$t('settings.storage.dataLocation.resetBlocked.intro', {
              count: resetBlockers.entries.length,
              path: resetBlockers.path,
            })}
          </p>
          <ul class="mt-1 list-disc pl-5 font-mono text-xs selectable">
            {#each resetBlockers.entries as name (name)}
              <li>{name}</li>
            {/each}
          </ul>
          <p class="mt-2 font-medium">{$t('settings.storage.dataLocation.keepRedirectNote')}</p>
        </div>
      {/if}

      {#if dataLocation.status}
        <div class="text-sm">
          <span class="text-muted">{$t('settings.storage.dataLocation.currentLabel')}</span>
          <!-- A recovery session's root is a throwaway dir under the cache: an implementation
               detail, never a path to show or measure. -->
          <span class="font-mono text-xs selectable ml-1"
            >{dataLocation.fellBack
              ? $t('settings.storage.dataLocation.temporarySession')
              : dataLocation.status.effective}</span
          >
        </div>
        {#if !dataLocation.fellBack}
          <div class="text-sm flex items-center gap-1">
            <span class="text-muted">{$t('settings.storage.dataLocation.sizeLabel')}</span>
            {#if dataRootSizeLoading}
              <Spinner size="sm" class="text-muted" />
            {:else if dataRootSize === null}
              <span class="ml-1 text-warning-text"
                >{$t('settings.storage.dataLocation.sizeUnknown')}</span
              >
            {:else}
              <span class="font-medium ml-1"
                >{formatSize($t, dataRootSize) || $t('format.size.bytes', { n: 0 })}</span
              >
            {/if}
          </div>
          {#if dataRootSizeError}
            <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2">
              {dataRootSizeError}
            </div>
          {/if}
        {/if}
      {:else}
        <p class="text-xs text-muted">…</p>
      {/if}

      <div class="flex flex-col gap-2">
        <div class="flex flex-wrap gap-2">
          <BusyButton
            type="button"
            class="btn-secondary btn-sm"
            busy={planning && !planningReset}
            disabled={planning || moveBlocked}
            onclick={() => void pickLocation()}
          >
            {$t('settings.storage.dataLocation.changeBtn')}
          </BusyButton>
          {#if dataLocation.status?.configured || dataLocation.fellBack}
            <!-- Deliberately NOT disabled while fallen back: a reset in that state is pointer-only
                 (the pointer is removed, nothing is copied) and is the way out for a configured
                 folder that will never come back. Rendered for a corrupt pointer too — it names no
                 folder, but detaching it is exactly what the copy offers. -->
            <BusyButton
              type="button"
              class="btn-secondary btn-sm"
              busy={planningReset}
              disabled={planning || moveBlocked}
              onclick={() => void requestReset()}
            >
              {$t('settings.storage.dataLocation.resetBtn')}
            </BusyButton>
          {/if}
          {#if restartBlock !== 'none' && restartBlock !== 'checking'}
            <button
              type="button"
              class="btn-secondary btn-sm inline-flex items-center gap-1.5"
              onclick={() => void recheckBlocked()}
            >
              <Icon name="refresh" class="icon-spin-hover" />
              {$t('settings.storage.dataLocation.blocked.recheckBtn')}
            </button>
          {/if}
        </div>
        <!-- ONE inline reason for both disabled buttons: a tooltip would not reach a keyboard
             user (docs/DESIGN.md §8). -->
        <StatusMessage
          message={blockedReason}
          tone={restartBlock === 'checking' ? 'info' : 'warning'}
          withIcon={restartBlock !== 'checking'}
        />
        <StatusMessage message={moveNotice} tone="info" />
      </div>
    </div>
  </SettingsField>
</div>

{#if pendingTarget !== null}
  <!-- Stays up with its busy spinner while a redirect-only commit (adopt, pointer-only reset) is
       in flight. A real copy closes it at once: the app-level DataMoveHost dialog takes over. -->
  <DataLocationConfirmDialog
    mode={pendingTarget.kind}
    fromPath={dataLocation.status?.effective ?? ''}
    toPath={pendingTarget.path}
    detachedPath={dataLocation.status?.configured ?? null}
    recoverySession={dataLocation.fellBack}
    pointerOnly={pendingTarget.kind === 'reset' && pendingTarget.pointerOnly}
    requiredBytes={pendingTarget.kind === 'adopt' ? null : pendingTarget.requiredBytes}
    freeBytes={pendingTarget.kind === 'adopt' ? null : pendingTarget.freeBytes}
    currentSizeBytes={dataRootSize}
    busy={committing}
    onCancel={cancelPending}
    onConfirm={() => void confirmPending()}
  />
{/if}
