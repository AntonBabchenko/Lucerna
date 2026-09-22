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
  import { onMount } from 'svelte';
  import { commands, type GeneralSettings } from '$lib/ipc/bindings';
  import { describeStoreError, formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon } from '$lib/ui/icons';
  import {
    runUpdate,
    updateInstalling,
    updatePhase,
    type UpdatePhase,
    updateState,
  } from '$lib/update/state.svelte';
  import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { CHANGELOG } from '$lib/changelog/source';
  import { createRestartGate, type RestartGate } from './restart-gate.svelte';
  import SettingsField from './SettingsField.svelte';

  // The setting is tri-state on purpose: a value is shown only once the backend
  // has confirmed one. Before the read settles, and after it failed, the box is
  // disabled and shows NO value — a hard-coded default next to a red line
  // would be an unknown shown as a confident "on".
  type Setting =
    | { kind: 'pending' }
    | { kind: 'ok'; general: GeneralSettings; value: boolean }
    | { kind: 'failed'; error: string };
  let setting = $state<Setting>({ kind: 'pending' });
  let saveError = $state<string | null>(null);
  const loaded = $derived(setting.kind === 'ok');
  const checked = $derived(setting.kind === 'ok' ? setting.value : false);

  async function load() {
    setting = { kind: 'pending' };
    saveError = null;
    const r = await commands.appSettingsGet();
    setting =
      r.status === 'ok'
        ? { kind: 'ok', general: r.data.general, value: r.data.general.check_updates_on_startup }
        : { kind: 'failed', error: formatError(r.error) };
  }
  onMount(() => void load());

  /** Optimistic flip; a failed read-modify-write reverts to the value the
   *  backend last confirmed and says the change was not saved. */
  async function toggle(next: boolean) {
    if (setting.kind !== 'ok') return;
    saveError = null;
    const confirmed = setting.value;
    setting = { ...setting, value: next };
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      revert(confirmed, formatError(cur.error));
      return;
    }
    const r = await commands.appSettingsSetGeneral({
      ...cur.data.general,
      check_updates_on_startup: next,
    });
    if (r.status !== 'ok') revert(confirmed, formatError(r.error));
  }
  function revert(confirmed: boolean, error: string) {
    if (setting.kind === 'ok') setting = { ...setting, value: confirmed };
    saveError = $t('settings.general.updates.notSaved', { error });
  }

  // Manual update check — mirrors the startup check but reports inline.
  type CheckResult =
    | { kind: 'idle' }
    | { kind: 'uptodate'; current: string }
    | { kind: 'available'; version: string }
    | { kind: 'error'; message: string };
  let checking = $state(false);
  let checkResult = $state<CheckResult>({ kind: 'idle' });

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
        checkResult = { kind: 'error', message: formatError(r.error) };
        return;
      }
      if (r.data.available) {
        updateState.value = r.data;
        checkResult = { kind: 'available', version: r.data.latest };
      } else {
        checkResult = { kind: 'uptodate', current: r.data.current };
      }
    } catch (e) {
      // typedError rethrows real Error instances; without this the button stayed on "Checking…".
      checkResult = { kind: 'error', message: describeStoreError(e) };
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
  const gateReason = $derived(gate.block === 'none' ? null : $t(GATE_KEYS[gate.block]));
  const updateBlocked = $derived(offered && gate.block !== 'none');
  $effect(() => {
    if (offered) void gate.recheck();
  });

  async function update() {
    await runUpdate();
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
</script>

<section class="flex flex-col gap-6">
  <div class="flex flex-col gap-3">
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
    {#if setting.kind === 'failed'}
      <div class="flex flex-wrap items-center gap-2">
        <StatusMessage
          message={$t('settings.general.updates.loadFailed', { error: setting.error })}
          tone="danger"
        />
        <button type="button" class="btn-secondary btn-sm" onclick={() => void load()}>
          {$t('settings.general.updates.retryBtn')}
        </button>
      </div>
    {/if}
    <StatusMessage message={saveError} tone="danger" />
    <div class="flex items-center gap-3 flex-wrap">
      <BusyButton
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1.5"
        onclick={() => void checkForUpdates()}
        busy={checking}
        data-testid="check-updates-btn"
      >
        <Icon name="refresh" class="icon-spin-hover" />{checking
          ? $t('settings.general.updates.checking')
          : $t('settings.general.updates.checkBtn')}
      </BusyButton>
      {#if checkResult.kind === 'uptodate'}
        <p class="text-xs text-muted" data-testid="update-status">
          {$t('settings.general.updates.uptodate', { version: checkResult.current })}
        </p>
      {:else if checkResult.kind === 'error'}
        <p class="text-xs text-danger" data-testid="update-status">
          {$t('settings.general.updates.error', { message: checkResult.message })}
        </p>
      {:else if checkResult.kind === 'available'}
        <p class="text-xs text-primary" data-testid="update-status">
          {$t('settings.general.updates.available', { version: checkResult.version })}
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
        {#if notifyOnly}
          <p class="basis-full text-xs text-muted" data-testid="update-manual-hint">
            {$t('settings.general.updates.manualHint')}
          </p>
        {:else}
          <p class="basis-full text-xs text-muted" data-testid="update-explain">
            {$t('settings.general.updates.explain')}
          </p>
          {#if gate.block !== 'none' && gate.block !== 'checking'}
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
  </div>

  <SettingsField anchor="updates.changelog">
    <div class="flex flex-col gap-3 border-t pt-4">
      <h3 class="font-medium text-sm text-primary">{$t('settings.changelog.title')}</h3>
      <ChangelogPanel entries={CHANGELOG} />
    </div>
  </SettingsField>
</section>
