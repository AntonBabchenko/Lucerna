<script lang="ts">
  // Settings → Updates. Startup-check toggle + manual check / update-now,
  // followed by the "What's new" changelog (moved here from About — it
  // pairs naturally with keeping the app current). Owns only the
  // check_updates_on_startup GeneralSettings field via a fresh RMW.
  //
  // Lucerna CLOSES to install an update, and the exit hook force-kills every
  // running game and server — so the action sits behind the same gate as the
  // data-folder move (`createRestartGate`), says what it will do, and follows
  // the real stage instead of saying "Installing…" during a download.
  import { onMount, untrack } from 'svelte';
  import pkg from '../../../package.json' with { type: 'json' };
  import { commands } from '$lib/ipc/bindings';
  import { describeStoreError, formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { isActiveTask, taskList } from '$lib/tasks/registry.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import {
    runUpdate,
    skipUpdate,
    stopSkipping,
    updateInstalling,
    updatePhase,
    type UpdatePhase,
    updateState,
  } from '$lib/update/state.svelte';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { CHANGELOG } from '$lib/changelog/source';
  import {
    appSettings,
    generalDisplayed,
    loadAppSettings,
    patchGeneral,
    saveFailure,
  } from './app-settings.svelte';
  import { createRestartGate, type RestartGate } from './restart-gate.svelte';
  import SettingsField from './SettingsField.svelte';

  // The startup-check toggle is one field on the one settings contract: no
  // value until the settings are read, disabled + "couldn't read" + Retry
  // after a failed read, and a failed save said right here.
  const general = $derived(generalDisplayed());
  const loaded = $derived(general !== null);
  const checked = $derived(general?.check_updates_on_startup ?? false);
  const loadError = $derived(
    appSettings.loaded.kind === 'failed' ? appSettings.loaded.error : null,
  );
  async function toggle(next: boolean) {
    await patchGeneral({ check_updates_on_startup: next });
  }

  // Manual update check — mirrors the startup check but reports inline.
  type CheckResult =
    | { kind: 'idle' }
    | { kind: 'uptodate'; current: string }
    | { kind: 'available'; version: string; current: string; releaseUrl: string | null }
    // `framed`: whether the "Couldn't check:" wrapper still has to supply the
    // headline. update_check_failed formats to "Couldn't check for updates: …"
    // on its own; a network failure, a refused host or a thrown Error does not.
    | { kind: 'error'; message: string; framed: boolean };
  let checking = $state(false);
  // UPD-03: what the startup check already found is on the page the moment it
  // opens — the toast is gone after a few seconds, this page is the durable place.
  function offerOf(info: typeof updateState.value): CheckResult {
    return info?.available
      ? {
          kind: 'available',
          version: info.latest,
          current: info.current,
          releaseUrl: info.release_url || null,
        }
      : { kind: 'idle' };
  }
  let checkResult = $state<CheckResult>(offerOf(updateState.value));
  $effect(() => {
    const info = updateState.value;
    if (info?.available && untrack(() => checkResult.kind === 'idle')) {
      checkResult = offerOf(info);
    }
  });

  // UPD-04: skipping is an explicit choice, and the page shows a skip that is
  // in force. The backend says which skip is worth mentioning (only one newer
  // than what runs), so the page never compares versions or trusts a stale copy.
  // Could not tell → nothing is claimed; skipping again is harmless.
  let skipped = $state<string | null>(null);
  let skipError = $state<string | null>(null);
  let skipUnknown = $state<string | null>(null);
  let skipBusy = $state(false);
  onMount(() => {
    void (async () => {
      // Could not tell → say so. No skip line is claimed; Skip still works.
      try {
        const r = await commands.updateSkippedVersion();
        if (r.status === 'ok') skipped = r.data;
        else
          skipUnknown = $t('settings.general.updates.skipUnknown', {
            error: formatError(r.error),
          });
      } catch (e) {
        skipUnknown = $t('settings.general.updates.skipUnknown', {
          error: describeStoreError(e),
        });
      }
    })();
  });
  async function skip(version: string) {
    skipBusy = true;
    skipError = null;
    const r = await skipUpdate(version);
    skipBusy = false;
    if (r.ok) skipped = r.skipped;
    else skipError = $t('settings.general.updates.skipFailed', { error: r.error });
  }
  async function unskip() {
    skipBusy = true;
    skipError = null;
    const r = await stopSkipping();
    skipBusy = false;
    if (r.ok) skipped = r.skipped;
    else skipError = $t('settings.general.updates.stopSkippingFailed', { error: r.error });
  }

  // On platforms without in-app install (macOS, .deb / .rpm) the backend returns
  // a null installer and runUpdate() opens the release page instead of
  // installing. Surface that honestly — an "Open release page" label plus a
  // manual-install hint — so "Update" never implies auto-update where there is
  // none. Derived from the live UpdateInfo, not the build target.
  const notifyOnly = $derived(updateState.value?.installer === null);

  async function checkForUpdates() {
    checking = true;
    checkResult = { kind: 'idle' };
    try {
      const r = await commands.updateCheck();
      if (r.status !== 'ok') {
        checkResult = {
          kind: 'error',
          message: formatError(r.error),
          framed: r.error.kind !== 'update_check_failed',
        };
        return;
      }
      if (r.data.available) {
        updateState.value = r.data;
        checkResult = offerOf(r.data);
      } else {
        // A stale offer from an earlier check must not come back on the next open.
        updateState.value = null;
        checkResult = { kind: 'uptodate', current: r.data.current };
      }
    } catch (e) {
      // typedError rethrows real Error instances; without this the button stayed on "Checking…".
      checkResult = { kind: 'error', message: describeStoreError(e), framed: true };
    } finally {
      checking = false;
    }
  }

  // The gate. Asked once an update is offered (the button does not exist before),
  // after an install that came back (refused, failed, or nothing newer), and on
  // Check again. The sentences are this panel's own: they speak of closing to
  // update, not of moving the data folder.
  const gate = createRestartGate();
  const GATE_KEYS: Record<Exclude<RestartGate, 'none'>, TranslationKey> = {
    checking: 'settings.general.updates.blocked.checking',
    running: 'settings.general.updates.blocked.running',
    busy: 'settings.general.updates.blocked.busy',
    unknown: 'settings.general.updates.blocked.unknown',
  };
  const offered = $derived(checkResult.kind === 'available' && !notifyOnly);
  // The visible gate is the gate `runUpdate` applies: a task this window started (an instance
  // being created — the backend's observer cannot see it) blocks before the backend is asked.
  const block = $derived.by((): RestartGate => {
    // Read the backend answer BEFORE the short-circuit: a derived tracks only what it read, and
    // one that never read `gate.block` while a task was active would not wake when it changes.
    const backend = gate.block;
    return taskList().some(isActiveTask) ? 'busy' : backend;
  });
  const gateReason = $derived(block === 'none' ? null : $t(GATE_KEYS[block]));
  const updateBlocked = $derived(offered && block !== 'none');
  $effect(() => {
    if (offered) void gate.recheck();
  });

  async function update() {
    const before = updateState.value;
    await runUpdate();
    // The install came back without an exit and dropped the offer (the re-check found nothing
    // newer): the panel must not keep showing "Update now" next to the toast that said so.
    if (before && updateState.value === null && checkResult.kind === 'available') {
      checkResult = { kind: 'uptodate', current: before.current };
      return;
    }
    // Back here means no exit happened: re-ask, so the inline reason matches
    // whatever the backend just refused on.
    if (offered && !updateInstalling.value) void gate.recheck();
  }

  const PHASE_KEYS: Record<UpdatePhase, TranslationKey> = {
    downloading: 'page.update.phase.downloading',
    verifying: 'page.update.phase.verifying',
    launching: 'page.update.phase.launching',
  };
  const actionLabel = $derived(
    notifyOnly
      ? $t('settings.general.updates.openReleasePage')
      : updateInstalling.value
        ? updatePhase.value
          ? $t(PHASE_KEYS[updatePhase.value])
          : $t('settings.general.updates.updating')
        : $t('settings.general.updates.updateNow'),
  );
  // One line, one live region: the tone follows the state (info / danger / info).
  const checkText = $derived.by((): string | null => {
    switch (checkResult.kind) {
      case 'uptodate':
        return $t('settings.general.updates.uptodate', { version: checkResult.current });
      case 'error':
        return checkResult.framed
          ? $t('settings.general.updates.error', { message: checkResult.message })
          : checkResult.message;
      case 'available':
        return $t('settings.general.updates.available', { version: checkResult.version });
      default:
        return null;
    }
  });
</script>

<section class="flex flex-col gap-6">
  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.updates.title')}</h3>
    <SettingsField anchor="updates.startupCheck">
      <label class="flex items-start gap-2 cursor-pointer">
        <input
          type="checkbox"
          class="mt-0.5"
          {checked}
          disabled={!loaded}
          onchange={(e) => void toggle((e.currentTarget as HTMLInputElement).checked)}
          data-testid="updates-toggle"
        />
        <span class="flex-1">
          <span class="text-sm text-primary">{$t('settings.general.updates.startupLabel')}</span>
          <span class="block text-xs text-muted">
            {$t('settings.general.updates.startupDescription')}
          </span>
        </span>
      </label>
    </SettingsField>
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
    <div data-testid="save-failure-check_updates_on_startup">
      <StatusMessage message={saveFailure('check_updates_on_startup')} tone="danger" />
    </div>
    <div class="flex items-center gap-3 flex-wrap">
      <BusyButton
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1.5"
        onclick={() => void checkForUpdates()}
        busy={checking}
        data-testid="check-updates-btn"
      >
        {#if !checking}<Icon name="refresh" class="icon-spin-hover" />{/if}{checking
          ? $t('settings.general.updates.checking')
          : $t('settings.general.updates.checkBtn')}
      </BusyButton>
      <!-- The wrapper renders only once a check has settled, so
           `findByTestId('update-status')` still waits for the result. -->
      {#if checkResult.kind !== 'idle'}
        <div data-testid="update-status">
          <StatusMessage
            message={checkText}
            tone={checkResult.kind === 'error' ? 'danger' : 'info'}
          />
        </div>
      {/if}
      {#if checkResult.kind === 'available'}
        <p class="basis-full text-xs text-muted" data-testid="update-you-have">
          {$t('settings.general.updates.youHave', { version: checkResult.current })}
        </p>
        <BusyButton
          type="button"
          class="btn-primary btn-sm"
          busy={updateInstalling.value}
          disabled={updateBlocked}
          onclick={() => void update()}
          data-testid="update-now-btn"
        >
          {actionLabel}
        </BusyButton>
        {#if checkResult.releaseUrl}
          {@const notes = checkResult.releaseUrl}
          <button
            type="button"
            class="btn-link btn-sm inline-flex items-center gap-1"
            use:tooltip={notes}
            onclick={() => void openExternalHttps(notes)}
          >
            {$t('settings.general.updates.releaseNotes')}
            <Icon name="externalLink" size={12} />
          </button>
        {/if}
        {#if skipped !== checkResult.version}
          {@const offered = checkResult.version}
          <button
            type="button"
            class="btn-secondary btn-sm"
            disabled={skipBusy}
            onclick={() => void skip(offered)}
          >
            {$t('settings.general.updates.skip')}
          </button>
        {/if}
        {#if notifyOnly}
          <p class="basis-full text-xs text-muted" data-testid="update-manual-hint">
            {$t('settings.general.updates.manualHint')}
          </p>
        {:else}
          <p class="basis-full text-xs text-muted" data-testid="update-explain">
            {$t('settings.general.updates.explain')}
          </p>
          <p class="basis-full text-xs text-muted" data-testid="update-verification">
            {$t('settings.general.updates.verification')}
          </p>
          {#if block !== 'none' && block !== 'checking'}
            <button
              type="button"
              class="btn-secondary btn-sm inline-flex items-center gap-1.5"
              onclick={() => void gate.recheck()}
            >
              <Icon name="refresh" class="icon-spin-hover" />
              {$t('settings.storage.dataLocation.blocked.recheckBtn')}
            </button>
          {/if}
          <!-- ONE inline reason, never a tooltip: it has to reach a keyboard user (DESIGN §8). -->
          <StatusMessage
            message={gateReason}
            tone={gate.block === 'checking' ? 'info' : 'warning'}
            withIcon={gate.block !== 'checking'}
          />
        {/if}
      {/if}
    </div>
    {#if skipped}
      <div class="flex flex-wrap items-center gap-2" data-testid="update-skipped">
        <p class="text-xs text-muted">
          {$t('settings.general.updates.skipped', { version: skipped })}
        </p>
        <button
          type="button"
          class="btn-tertiary btn-sm"
          disabled={skipBusy}
          onclick={() => void unskip()}
        >
          {$t('settings.general.updates.stopSkipping')}
        </button>
      </div>
    {/if}
    <StatusMessage message={skipError} tone="danger" />
    {#if skipped === null}
      <StatusMessage message={skipUnknown} tone="warning" />
    {/if}
  </div>

  <SettingsField anchor="updates.changelog">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.changelog.title')}</h3>
      <p class="text-xs text-muted">
        {$t('settings.changelog.intro', { version: pkg.version })}
      </p>
      <ChangelogPanel entries={CHANGELOG} collapseOlder={pkg.version} />
    </div>
  </SettingsField>
</section>
