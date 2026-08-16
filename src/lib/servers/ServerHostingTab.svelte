<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { formatError, isIpcError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { commands } from '$lib/ipc/bindings';
  import type { UploadConfig, UploadAuthMethod, UploadPreflight } from '$lib/ipc/bindings';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { formatSize } from '$lib/format/size';
  import { formatLastUpload, preflightLevel } from '$lib/servers/upload-summary';
  import {
    advanceProgressDisplay,
    emptyProgressDisplay,
    formatUploadProgress,
    type ProgressDisplay,
  } from '$lib/servers/upload-progress-format';

  let { serverId }: { serverId: string } = $props();

  // ── seed form from existing config ─────────────────────────────────────────
  const existing = $derived(serverState.list.find((s) => s.id === serverId));
  const existingUpload = $derived(existing?.upload ?? null);

  // svelte-ignore state_referenced_locally
  let host = $state(existingUpload?.host ?? '');
  // svelte-ignore state_referenced_locally
  let port = $state(existingUpload?.port ?? 22);
  // svelte-ignore state_referenced_locally
  let user = $state(existingUpload?.user ?? '');
  let password = $state('');
  let passwordRevealed = $state(false);
  let savePassword = $state(true);
  // svelte-ignore state_referenced_locally
  let remotePath = $state(existingUpload?.remote_path ?? '');

  // ── SFTP auth method (#28) — loaded from the S4 sidecar ─────────────────────
  let authMethod = $state<UploadAuthMethod>('password');
  let privateKeyPath = $state('');
  // A FAILED sidecar read is not "password auth, no key file" -- it is "we could
  // not tell". Seeding the form from that guess and leaving Save enabled means
  // one click writes the guess over the user's real stored method. So the read's
  // outcome is tracked and gates Save, rather than being discarded. Same
  // absent-vs-unknown discrimination as AiTranslationSection's
  // `keyStored: boolean | null`.
  let authLoaded = $state(false);
  let authLoadError = $state<string | null>(null);
  let busyAuthLoad = $state(false);

  // ── automatic-backup policy (#29) ───────────────────────────────────────────
  let backupEnabled = $state(false);
  let backupIntervalMinutes = $state(60);
  let busyBackupPolicy = $state(false);
  let backupPolicySaved = $state(false);
  let backupPolicyError = $state<string | null>(null);
  // Same rule for the schedule: a failed read leaves the form on its defaults
  // (off, 60 min), and Apply would then switch OFF a schedule the user has.
  let backupPolicyLoaded = $state(false);
  let backupPolicyLoadError = $state<string | null>(null);
  let busyBackupPolicyLoad = $state(false);

  // ── resume (Section B): a previously interrupted upload to the same target ──
  let resumeInfo = $state<{
    resumable: boolean;
    filesTotal: number;
    filesDone: number;
    bytesTotal: number;
  } | null>(null);

  /** Read the stored SFTP auth method. Retryable: the button below calls it
   *  again, so a transient failure does not strand the form for the session. */
  async function loadUploadAuth() {
    busyAuthLoad = true;
    try {
      const auth = await commands.serverGetUploadAuth(serverId);
      if (auth.status === 'ok') {
        authMethod = auth.data.method ?? 'password';
        privateKeyPath = auth.data.private_key_path ?? '';
        authLoadError = null;
        authLoaded = true;
      } else {
        authLoadError = formatError(auth.error);
      }
    } finally {
      busyAuthLoad = false;
    }
  }

  /** Read the stored automatic-backup schedule. Retryable, same reason. */
  async function loadBackupPolicy() {
    busyBackupPolicyLoad = true;
    try {
      const policy = await commands.serverBackupPolicyGet(serverId);
      if (policy.status === 'ok') {
        backupEnabled = policy.data.enabled ?? false;
        const interval = policy.data.interval_minutes ?? 0;
        if (interval > 0) backupIntervalMinutes = interval;
        backupPolicyLoadError = null;
        backupPolicyLoaded = true;
      } else {
        backupPolicyLoadError = formatError(policy.error);
      }
    } finally {
      busyBackupPolicyLoad = false;
    }
  }

  onMount(async () => {
    await loadUploadAuth();
    await loadBackupPolicy();
    resumeInfo = await serverState.uploadResumeState(serverId);
  });

  async function handleSaveBackupPolicy() {
    if (!backupPolicyLoaded) return;
    busyBackupPolicy = true;
    backupPolicyError = null;
    backupPolicySaved = false;
    try {
      const r = await commands.serverBackupPolicySet(serverId, {
        enabled: backupEnabled,
        interval_minutes: Math.max(1, Math.round(backupIntervalMinutes)),
        last_run_unix_ms: 0,
      });
      if (r.status === 'ok') {
        backupPolicySaved = true;
      } else {
        backupPolicyError = formatError(r.error);
      }
    } finally {
      busyBackupPolicy = false;
    }
  }

  // ── UI feedback ─────────────────────────────────────────────────────────────
  let savedVisible = $state(false);
  let saveError = $state<string | null>(null);
  let busySave = $state(false);

  let busyExport = $state(false);
  let exportError = $state<string | null>(null);
  let exportedVisible = $state(false);

  // host-key confirm dialog (#24): shown on first connect (with the fingerprint
  // to verify) AND on a later mismatch.
  let showHostKeyConfirm = $state(false);
  let busyHostKeyTrust = $state(false);
  let hostKeyFingerprint = $state<string | null>(null);
  let hostKeyIsFirstConnect = $state(false);

  // ── derived — upload state from store ────────────────────────────────────────
  const running = $derived(serverState.running(serverId));
  const uploadState = $derived(serverState.uploadStateFor(serverId));
  const uploading = $derived(serverState.isUploading(serverId));
  const uploadedVisible = $derived(uploadState?.phase === 'done');
  const storeUploadError = $derived(
    uploadState?.phase === 'error' ? (uploadState.error ?? null) : null,
  );
  // Legacy progress shape (file count + current file) read from upload state.
  const progress = $derived(
    uploading && uploadState
      ? {
          done: uploadState.filesDone,
          total: uploadState.filesTotal,
          file: uploadState.currentFile,
        }
      : undefined,
  );

  // ── Byte-level progress: speed + ETA, throttled to ~1 Hz ─────────────────────
  // Plan-4's parallel upload emits a progress event per finished file (~100/s on
  // many-small-file sets). Deriving the speed/ETA text straight off that stream
  // makes the numbers flicker. So we hold a snapshot and refresh it at most once
  // per second (advanceProgressDisplay returns the SAME reference within the
  // window → no re-render). The progress BAR (bytePct) stays live and smooth.
  const DISPLAY_REFRESH_MS = 1000;
  let display = $state<ProgressDisplay>(emptyProgressDisplay());

  $effect(() => {
    const s = uploadState; // tracked: this effect re-runs on every progress event…
    if (!s || s.phase !== 'uploading') {
      // …but every `display` read/write is untracked, so the effect's only
      // dependency is `uploadState` and it never self-loops on the snapshot.
      untrack(() => {
        if (display.lastRefreshMs !== 0 || display.bytesDone !== 0) {
          display = emptyProgressDisplay();
        }
      });
      return;
    }
    const bytesDone = s.bytesDone;
    const bytesTotal = s.bytesTotal;
    untrack(() => {
      display = advanceProgressDisplay(
        display,
        bytesDone,
        bytesTotal,
        Date.now(),
        DISPLAY_REFRESH_MS,
      );
    });
  });

  const bytePct = $derived(
    uploadState && uploadState.bytesTotal > 0
      ? Math.min(1, uploadState.bytesDone / uploadState.bytesTotal)
      : 0,
  );
  const progressLine = $derived(
    uploadState
      ? formatUploadProgress($t, {
          bytesDone: display.bytesDone,
          bytesTotal: uploadState.bytesTotal,
          speedBytesPerSec: display.speedBytesPerSec,
          etaSecondsValue: display.etaSecondsValue,
        })
      : '',
  );

  // Shared label for the password reveal toggle (aria-label + tooltip).
  const passwordToggleLabel = $derived(
    passwordRevealed ? $t('servers.hosting.hidePassword') : $t('servers.hosting.revealPassword'),
  );

  // ── validation (#25) ─────────────────────────────────────────────────────────
  const hostValid = $derived(host.trim().length > 0);
  const userValid = $derived(user.trim().length > 0);
  const keyValid = $derived(authMethod !== 'key' || privateKeyPath.trim().length > 0);
  // Port must be a whole number in the TCP range. A cleared field parses to NaN
  // (Number.isInteger is false → invalid), so Save is blocked until it's fixed.
  const portValid = $derived(Number.isInteger(port) && port >= 1 && port <= 65535);
  const formValid = $derived(hostValid && userValid && keyValid && portValid);
  // Save ALSO writes the auth method (serverSetUploadAuth), so it needs a method
  // we actually read. Upload and Check size use the STORED auth server-side and
  // are deliberately left on `formValid` -- blocking them would punish the user
  // for a failure that does not affect them.
  const canSaveConfig = $derived(formValid && authLoaded);

  // When savePassword is on, persist the typed password to the keyring on Save.
  // When off, the password stays transient (sent only for this upload, never stored).
  const secretToStore = $derived(savePassword && password !== '' ? password : null);
  // Transient secret: sent with upload but never persisted.
  const transientSecret = $derived(!savePassword && password !== '' ? password : null);
  // Upload is gated when password auth is chosen, save is off, and nothing is stored.
  const needsTransientPassword = $derived(
    authMethod === 'password' && !savePassword && !existing?.upload_password_set,
  );
  const uploadReady = $derived(!needsTransientPassword || password !== '');

  // ── selective upload + size preflight + last-upload (J / K / L) ──────────────
  let skipWorlds = $state(false);
  let preflight = $state<UploadPreflight | null>(null);
  let busyPreflight = $state(false);
  // preflightError is ONLY for a hard query failure (the command errored / returned
  // no preflight). It is mutually exclusive with `preflight`: a server that simply
  // doesn't report free space is NOT an error — that is the summary's 'unknown' state.
  let preflightError = $state<string | null>(null);

  const lastUpload = $derived(serverState.lastUploadFor(serverId));
  const lastUploadLine = $derived(formatLastUpload($t, lastUpload));

  // ── actions ─────────────────────────────────────────────────────────────────
  async function pickPrivateKey() {
    const picked = await open({ multiple: false, directory: false });
    if (typeof picked === 'string') privateKeyPath = picked;
  }

  async function handleSave() {
    // Guard the handler too, not only the button: the disabled attribute is a
    // UI affordance, and this write is the one that would clobber the real
    // stored method with the constructed default.
    if (!canSaveConfig) return;
    busySave = true;
    saveError = null;
    savedVisible = false;
    try {
      const cfg: UploadConfig = {
        host: host.trim(),
        port,
        user: user.trim(),
        remote_path: remotePath.trim(),
        known_host_fp: existingUpload?.known_host_fp ?? null,
      };
      const auth = await commands.serverSetUploadAuth(serverId, {
        method: authMethod,
        private_key_path: authMethod === 'key' ? privateKeyPath.trim() : null,
      });
      if (auth.status !== 'ok') {
        saveError = formatError(auth.error);
        return;
      }
      const r = await serverState.setUploadConfig(serverId, cfg, secretToStore);
      if (r.status === 'ok') {
        savedVisible = true;
        await serverState.refresh();
      } else {
        saveError = formatError(r.error);
      }
    } finally {
      busySave = false;
    }
  }

  /** Run the actual upload; `acceptNewHostKey` trusts a new/changed host key. */
  async function doUpload(acceptNewHostKey: boolean) {
    // The store's upload() owns all phase transitions. We only handle the
    // sftp_host_key_mismatch branch here (re-trust dialog) because the store
    // treats that kind as 'cancelled' so no generic error is persisted.
    const r = await serverState.upload(
      serverId,
      acceptNewHostKey,
      skipWorlds,
      transientSecret || null,
    );
    if (r.status === 'ok') {
      showHostKeyConfirm = false;
      await serverState.refresh();
    } else if (isIpcError(r.error) && r.error.kind === 'sftp_host_key_mismatch') {
      // Changed key: surface the new fingerprint for the user to weigh. `got` is
      // a required string on the typed variant, so there is ALWAYS a fingerprint
      // to show -- the old `?? null` came from a hand-written
      // `{ kind: string; got?: string }`, not from the backend's contract, and
      // made the dialog look like it could open with nothing to verify.
      //
      // `isIpcError` rather than a cast because upload() is the one store
      // wrapper whose failure can also be a THROWN transport error (UploadResult).
      hostKeyFingerprint = r.error.got;
      hostKeyIsFirstConnect = false;
      showHostKeyConfirm = true;
    }
    // All other errors are already persisted in the store as storeUploadError.
  }

  /** Continue an interrupted upload: skips already-uploaded files, re-uploads
   *  the rest (and the previously in-flight file). Reuses the trusted host key,
   *  so no host-key dialog — pass acceptNewHostKey = false. Mirrors doUpload's
   *  skipWorlds + transient-secret arguments, appending resume = true. */
  async function handleResume() {
    resumeInfo = null; // hide the affordance once we start
    saveError = null;
    previewError = null;
    await serverState.upload(serverId, false, skipWorlds, transientSecret || null, true);
    await serverState.refresh();
    // Refresh the resume snapshot (cleared on full success; non-null if cancelled).
    resumeInfo = await serverState.uploadResumeState(serverId);
  }

  // busyHostKeyPreview: tracks the host-key fetch before the first upload.
  // Separate from the in-flight upload indicator so the Upload button shows
  // busy while we fetch the fingerprint, before the real upload starts.
  let busyHostKeyPreview = $state(false);
  // previewError: surfaced when serverHostKeyPreview fails. upload() was never
  // called in this path so the store has no error to show — keep a local slot.
  let previewError = $state<string | null>(null);

  /** First click: on first connect, fetch + show the fingerprint to verify
   *  BEFORE uploading (#24). Once a host key is trusted, upload directly. */
  async function handleUpload() {
    if (existingUpload?.known_host_fp) {
      await doUpload(false);
      return;
    }
    previewError = null;
    busyHostKeyPreview = true;
    try {
      const r = await commands.serverHostKeyPreview(serverId);
      if (r.status === 'ok') {
        hostKeyFingerprint = r.data.fingerprint;
        hostKeyIsFirstConnect = true;
        showHostKeyConfirm = true;
      } else {
        previewError = formatError(r.error);
      }
    } finally {
      busyHostKeyPreview = false;
    }
  }

  async function handleTrustAndUpload() {
    busyHostKeyTrust = true;
    try {
      await doUpload(true);
    } finally {
      busyHostKeyTrust = false;
    }
  }

  /** Size/free-space preflight (#K). The result and a hard-failure line are
   *  mutually exclusive: clear one whenever the other is set. A returned
   *  preflight with `free_bytes == null` is a SUCCESS (the summary's 'unknown'
   *  state), not a failure — only a null return uses `preflightFailed`. */
  async function handleCheckSize() {
    busyPreflight = true;
    preflightError = null;
    preflight = null;
    try {
      const acceptKey = !existingUpload?.known_host_fp;
      const result = await serverState.uploadPreflight(serverId, acceptKey, skipWorlds);
      if (result === null) {
        preflightError = $t('servers.hosting.preflightFailed');
      } else {
        preflight = result;
      }
    } finally {
      busyPreflight = false;
    }
  }

  async function handleExport() {
    if (running) return;
    exportError = null;
    exportedVisible = false;
    const serverName = existing?.name ?? 'server';
    const dest = await save({
      defaultPath: `${serverName}.zip`,
      filters: [{ name: $t('common.fileFilter.zip'), extensions: ['zip'] }],
    });
    if (!dest) return;

    busyExport = true;
    try {
      const r = await serverState.exportZip(serverId, dest);
      if (r.status === 'ok') {
        exportedVisible = true;
      } else {
        exportError = formatError(r.error);
      }
    } finally {
      busyExport = false;
    }
  }
</script>

<div class="flex flex-col gap-6">
  <!-- ── SFTP config form ─────────────────────────────────────────────────── -->
  <section class="flex flex-col gap-3">
    <div class="grid grid-cols-[1fr_auto] gap-3">
      <div class="flex flex-col gap-1">
        <label class="text-xs text-muted" for="hosting-host">{$t('servers.hosting.host')}</label>
        <input
          id="hosting-host"
          class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
          class:border-danger={!hostValid && host.length > 0}
          bind:value={host}
          placeholder="example.com"
          autocomplete="off"
        />
        {#if !hostValid}
          <p class="text-xs text-muted">{$t('servers.hosting.hostRequired')}</p>
        {/if}
      </div>
      <div class="flex flex-col gap-1">
        <label class="text-xs text-muted" for="hosting-port">{$t('servers.hosting.port')}</label>
        <input
          id="hosting-port"
          class="h-8 w-20 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
          class:border-danger={!portValid}
          type="number"
          min={1}
          max={65535}
          aria-invalid={!portValid}
          bind:value={port}
        />
      </div>
    </div>

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-user">{$t('servers.hosting.user')}</label>
      <input
        id="hosting-user"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        class:border-danger={!userValid && user.length > 0}
        bind:value={user}
        autocomplete="username"
      />
      {#if !userValid}
        <p class="text-xs text-muted">{$t('servers.hosting.userRequired')}</p>
      {/if}
    </div>

    <!-- ── Auth method (#28) ─────────────────────────────────────────────── -->
    <div class="flex flex-col gap-1">
      <span class="text-xs text-muted">{$t('servers.hosting.authMethod')}</span>
      <div class="flex gap-4 text-sm">
        <label class="flex items-center gap-1.5">
          <input type="radio" value="password" bind:group={authMethod} />
          {$t('servers.hosting.authPassword')}
        </label>
        <label class="flex items-center gap-1.5">
          <input type="radio" value="key" bind:group={authMethod} />
          {$t('servers.hosting.authKey')}
        </label>
      </div>
    </div>

    {#if authMethod === 'key'}
      <div class="flex flex-col gap-1">
        <label class="text-xs text-muted" for="hosting-key-path"
          >{$t('servers.hosting.privateKeyPath')}</label
        >
        <div class="flex gap-2">
          <input
            id="hosting-key-path"
            class="h-8 flex-1 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
            class:border-danger={!keyValid && privateKeyPath.length > 0}
            bind:value={privateKeyPath}
            placeholder="~/.ssh/id_ed25519"
            autocomplete="off"
          />
          <button type="button" class="btn-secondary btn-sm" onclick={() => void pickPrivateKey()}>
            {$t('servers.hosting.browse')}
          </button>
        </div>
        {#if !keyValid}
          <p class="text-xs text-muted">{$t('servers.hosting.keyPathRequired')}</p>
        {/if}
      </div>
    {/if}

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-password">
        {authMethod === 'key' ? $t('servers.hosting.passphrase') : $t('servers.hosting.password')}
      </label>
      <div class="relative flex items-center">
        <input
          id="hosting-password"
          class="h-8 flex-1 rounded border border-border-emphasis bg-surface px-3 pr-9 text-sm text-primary"
          type={passwordRevealed ? 'text' : 'password'}
          bind:value={password}
          autocomplete="current-password"
        />
        <button
          type="button"
          class="btn-icon btn-icon-sm absolute right-1"
          aria-pressed={passwordRevealed}
          aria-label={passwordToggleLabel}
          use:tooltip={passwordToggleLabel}
          onclick={() => (passwordRevealed = !passwordRevealed)}
        >
          <Icon name={passwordRevealed ? 'eyeOff' : 'eye'} size={16} />
        </button>
      </div>
      {#if existing?.upload_password_set && !password}
        <p class="text-xs text-muted">
          {authMethod === 'key'
            ? $t('servers.hosting.passphraseStored')
            : $t('servers.hosting.passwordStored')}
        </p>
      {/if}
      {#if authMethod === 'password'}
        <label class="mt-1 flex items-center gap-2 text-sm">
          <input type="checkbox" bind:checked={savePassword} />
          {$t('servers.hosting.savePassword')}
        </label>
        {#if savePassword}
          <p class="text-xs text-muted">{$t('servers.hosting.savePasswordHint')}</p>
        {/if}
        {#if needsTransientPassword && password === ''}
          <p class="text-xs text-warning-text">{$t('servers.hosting.passwordNeededEachUpload')}</p>
        {/if}
      {/if}
    </div>

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-remote-path"
        >{$t('servers.hosting.remotePath')}</label
      >
      <input
        id="hosting-remote-path"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={remotePath}
        placeholder="/home/mc/server"
        autocomplete="off"
      />
    </div>

    {#if authLoadError}
      <p class="text-sm text-danger" role="alert" data-testid="hosting-auth-load-error">
        {$t('servers.hosting.authLoadFailed', { error: authLoadError })}
      </p>
      <div>
        <BusyButton
          class="btn-secondary btn-sm"
          busy={busyAuthLoad}
          onclick={() => void loadUploadAuth()}
        >
          {$t('servers.retry')}
        </BusyButton>
      </div>
    {/if}
    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-primary btn-sm"
        busy={busySave || busyAuthLoad}
        disabled={!canSaveConfig}
        onclick={() => void handleSave()}
      >
        {$t('servers.hosting.save')}
      </BusyButton>
      {#if savedVisible}
        <span class="text-sm text-success">{$t('servers.hosting.saved')}</span>
      {/if}
    </div>
    {#if saveError}
      <p class="text-sm text-danger">{saveError}</p>
    {/if}
  </section>

  <!-- ── Upload ───────────────────────────────────────────────────────────── -->
  <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
    {#if lastUploadLine}
      <p class="text-xs text-muted" data-testid="last-upload-line">{lastUploadLine}</p>
    {/if}

    <label class="flex items-center gap-2 text-sm">
      <input type="checkbox" bind:checked={skipWorlds} />
      {$t('servers.hosting.skipWorlds')}
    </label>
    <p class="text-xs text-muted">{$t('servers.hosting.skipWorldsHint')}</p>

    <div class="flex items-center gap-3 flex-wrap">
      <BusyButton
        class="btn-primary btn-sm"
        busy={uploading || busyHostKeyPreview}
        disabled={running || !formValid || uploading || !uploadReady}
        onclick={() => void handleUpload()}
      >
        {#if uploading && uploadState && uploadState.bytesTotal > 0}
          {progressLine}
        {:else if uploading && progress}
          {$t('servers.hosting.uploading', { done: progress.done, total: progress.total })}
        {:else}
          {$t('servers.hosting.upload')}
        {/if}
      </BusyButton>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={running || !formValid || busyPreflight}
        onclick={() => void handleCheckSize()}
      >
        {$t('servers.hosting.checkSize')}
      </button>
      {#if uploading}
        <button
          type="button"
          class="btn-secondary btn-sm"
          onclick={() => void serverState.cancelUpload(serverId)}
        >
          {$t('servers.hosting.cancelUpload')}
        </button>
      {/if}
      {#if running}
        <span class="text-xs text-muted">{$t('servers.hosting.runningBlock')}</span>
      {/if}
      {#if uploadedVisible}
        <span class="text-sm text-success">{$t('servers.hosting.uploaded')}</span>
      {/if}
    </div>

    {#if resumeInfo?.resumable && !uploading}
      <div class="flex flex-col gap-1" data-testid="resume-available">
        <p class="text-xs text-muted">
          {$t('servers.hosting.resumeAvailable', {
            done: resumeInfo.filesDone,
            total: resumeInfo.filesTotal,
          })}
        </p>
        <div class="flex items-center gap-2">
          <button type="button" class="btn-secondary btn-sm" onclick={() => void handleResume()}>
            {$t('servers.hosting.resumeUpload')}
          </button>
          <span class="text-xs text-muted">{$t('servers.hosting.resumeHint')}</span>
        </div>
      </div>
    {/if}

    {#if preflight}
      {@const level = preflightLevel(preflight)}
      <div class="flex flex-col gap-1 text-xs">
        <span class="text-muted"
          >{$t('servers.hosting.preflightTotal', {
            size: formatSize($t, preflight.total_bytes),
          })}</span
        >
        {#if preflight.free_bytes != null}
          <span class="text-muted"
            >{$t('servers.hosting.preflightFree', {
              size: formatSize($t, preflight.free_bytes),
            })}</span
          >
        {/if}
        {#if level === 'over'}
          <span class="text-danger" data-testid="preflight-over">
            {$t('servers.hosting.preflightOver', {
              total: formatSize($t, preflight.total_bytes),
              free: formatSize($t, preflight.free_bytes ?? 0),
            })}
          </span>
        {:else if level === 'unknown'}
          <span class="text-muted">{$t('servers.hosting.preflightUnknown')}</span>
        {/if}
      </div>
    {/if}
    {#if preflightError && !preflight}
      <p class="text-xs text-danger" data-testid="preflight-failed">{preflightError}</p>
    {/if}

    {#if uploading}
      <div
        class="w-full bg-muted/20 rounded-full h-1.5 overflow-hidden"
        role="progressbar"
        aria-valuenow={Math.round(bytePct * 100)}
        aria-valuemin={0}
        aria-valuemax={100}
        data-testid="upload-progress-bar"
      >
        <div
          class="bg-accent h-full w-full origin-left transition-transform"
          style="transform: scaleX({bytePct})"
        ></div>
      </div>
      <p class="text-xs text-muted" data-testid="upload-progress-line">{progressLine}</p>
      {#if progress}
        <p class="text-xs text-muted truncate">{progress.file}</p>
      {/if}
    {/if}

    {#if storeUploadError && !showHostKeyConfirm}
      <p class="text-sm text-danger">{storeUploadError}</p>
    {/if}
    {#if previewError}
      <p class="text-sm text-danger">{previewError}</p>
    {/if}
  </section>

  <!-- ── Host-key confirm inline (#24) ───────────────────────────────────── -->
  {#if showHostKeyConfirm}
    <section
      class="flex items-start gap-2 border border-warning-text rounded-xl p-4 bg-warning-bg"
      data-testid="host-key-confirm"
    >
      <Icon name="warning" size={16} class="mt-0.5 shrink-0 text-warning-text" />
      <div class="flex-1 flex flex-col gap-3">
        <p class="text-sm font-semibold text-primary">
          {hostKeyIsFirstConnect
            ? $t('servers.hosting.hostKeyFirstTitle')
            : $t('servers.hosting.hostKeyTitle')}
        </p>
        <p class="text-sm text-secondary">
          {hostKeyIsFirstConnect
            ? $t('servers.hosting.hostKeyFirstBody')
            : $t('servers.hosting.hostKeyBody')}
        </p>
        {#if hostKeyFingerprint}
          <div class="flex flex-col gap-1">
            <span class="text-xs text-muted">{$t('servers.hosting.hostKeyFingerprint')}</span>
            <code class="text-xs break-all font-mono bg-muted/20 rounded px-2 py-1"
              >{hostKeyFingerprint}</code
            >
          </div>
        {/if}
        <div class="flex gap-2">
          <BusyButton
            class="btn-primary btn-sm"
            busy={busyHostKeyTrust}
            onclick={() => void handleTrustAndUpload()}
          >
            {$t('servers.hosting.hostKeyTrust')}
          </BusyButton>
          <button
            type="button"
            class="btn-secondary btn-sm"
            onclick={() => (showHostKeyConfirm = false)}
          >
            {$t('servers.hosting.cancel')}
          </button>
        </div>
      </div>
    </section>
  {/if}

  <!-- ── Automatic backups (#29) ──────────────────────────────────────────── -->
  <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
    <p class="text-sm font-semibold text-primary">{$t('servers.backups.autoTitle')}</p>
    <p class="text-xs text-muted">{$t('servers.backups.autoHint')}</p>
    <label class="flex items-center gap-2 text-sm">
      <input type="checkbox" bind:checked={backupEnabled} />
      {$t('servers.backups.autoEnable')}
    </label>
    {#if backupEnabled}
      <div class="flex items-center gap-2 text-sm">
        <label for="hosting-backup-interval">{$t('servers.backups.autoInterval')}</label>
        <input
          id="hosting-backup-interval"
          class="h-8 w-20 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
          type="number"
          min={1}
          max={1440}
          bind:value={backupIntervalMinutes}
        />
        <span>{$t('servers.backups.autoIntervalUnit')}</span>
      </div>
    {/if}
    {#if backupPolicyLoadError}
      <p class="text-sm text-danger" role="alert" data-testid="hosting-backup-load-error">
        {$t('servers.backups.autoLoadFailed', { error: backupPolicyLoadError })}
      </p>
      <div>
        <BusyButton
          class="btn-secondary btn-sm"
          busy={busyBackupPolicyLoad}
          onclick={() => void loadBackupPolicy()}
        >
          {$t('servers.retry')}
        </BusyButton>
      </div>
    {/if}
    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-secondary btn-sm"
        busy={busyBackupPolicy || busyBackupPolicyLoad}
        disabled={!backupPolicyLoaded}
        onclick={() => void handleSaveBackupPolicy()}
      >
        {$t('servers.backups.autoSave')}
      </BusyButton>
      {#if backupPolicySaved}
        <span class="text-sm text-success">{$t('servers.backups.autoSaved')}</span>
      {/if}
    </div>
    {#if backupPolicyError}
      <p class="text-sm text-danger">{backupPolicyError}</p>
    {/if}
  </section>

  <!-- ── Export ZIP ───────────────────────────────────────────────────────── -->
  <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-secondary btn-sm"
        busy={busyExport}
        disabled={running}
        onclick={() => void handleExport()}
      >
        {$t('servers.hosting.exportZip')}
      </BusyButton>
      {#if running}
        <span class="text-xs text-muted">{$t('servers.hosting.exportRunningBlock')}</span>
      {/if}
      {#if exportedVisible}
        <span class="text-sm text-success">{$t('servers.hosting.exported')}</span>
      {/if}
    </div>
    {#if exportError}
      <p class="text-sm text-danger">{exportError}</p>
    {/if}
  </section>
</div>
