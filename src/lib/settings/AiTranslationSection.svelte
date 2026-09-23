<script lang="ts">
  // Settings → Integrations: the AI translation pre-fill.
  //
  // Owns exactly four GeneralSettings fields — `allow_ai_translation`,
  // `ai_provider`, `ai_model`, `ai_local_port` — and persists them with the
  // same fresh read-modify-write GamePanel uses, so it never clobbers a
  // sibling panel's field.
  //
  // The API key does NOT live in app.json: it goes straight to the OS keyring
  // through `l10n_prefill_set_key`, and the only thing that ever comes back is
  // the boolean from `l10n_prefill_key_status`. There is deliberately no
  // command anywhere that reads a stored key out to the UI, so this field
  // starts empty on every open — a key can be replaced but never displayed.
  //
  // The scope caveat sits above the toggle rather than below it: what this
  // feature cannot translate (FTB Quests, quest books, config-driven text) is
  // the thing a user most needs to know BEFORE they spend money on a run, not
  // after.
  //
  // `allow_ai_translation` gates the ENTIRE section, not merely the button that
  // makes the request. Every control below the checkbox — provider, model,
  // local port, key field, Save / Clear key — describes a request the copy
  // right above it has just promised will not be sent, so while the permission
  // is off they are all disabled and one muted line says why. Consent first,
  // configuration second: a user cannot arrange a provider and file a
  // credential in the OS keyring and only then decide whether they meant it.
  // The stored-key STATUS is not a control and stays readable throughout — it
  // is a local keyring read, and hiding it would tell a returning user nothing
  // about a key they already have.
  import { type AiProvider, commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import {
    appSettings,
    generalDisplayed,
    loadAppSettings,
    patchGeneral,
    saveFailure,
    saveFailureKind,
  } from '$lib/settings/app-settings.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import NumberField from '$lib/ui/NumberField.svelte';
  import { Icon } from '$lib/ui/icons';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import { providerFailureOrNull } from '$lib/l10n/provider-failure';
  import ApiKeyField from './ApiKeyField.svelte';
  import type { FieldStatus } from './api-key-field';

  // Ollama's default. Matches `default_ai_local_port()` in instances/schema.rs.
  const DEFAULT_LOCAL_PORT = 11434;
  const MIN_PORT = 1;
  const MAX_PORT = 65535;

  // Four fields on the one settings contract: no value and no live control
  // until the settings are read; each change patches only its field; a
  // failed save is said next to the control that failed.
  const general = $derived(generalDisplayed());
  const settingsLoaded = $derived(general !== null);
  const loadError = $derived(
    appSettings.loaded.kind === 'failed' ? appSettings.loaded.error : null,
  );
  // The consent says the restrictive truth only when it is true: a REFUSED
  // revoke leaves the permission on (the file is unchanged); one the transport
  // lost is decided by the re-read.
  const consentFailure = $derived.by((): string | null => {
    const kind = saveFailureKind('allow_ai_translation');
    if (kind === 'refused' && general?.allow_ai_translation) {
      return $t('settings.aiTranslation.revokeFailed', {
        error: appSettings.failures.allow_ai_translation?.error ?? '',
      });
    }
    return saveFailure('allow_ai_translation');
  });

  let pendingKey = $state('');
  // `null` while the status read is still in flight — distinct from a known
  // "no key stored", which is what the user sees before they paste one.
  // null = not read yet; { unknown } = the keyring could not be read (the
  // formatted reason) — shown as "couldn't check", never as "Checking…" or
  // "no key", either of which would be a claim about a keyring nobody saw.
  let keyStored = $state<boolean | null | { unknown: string }>(null);
  let keyError = $state<string | null>(null);
  let savingKey = $state(false);
  let clearingKey = $state(false);
  // The provider's default model, from Rust (the one place the names live).
  // Empty until the command answers; on failure it stays empty and the
  // generic "Provider's default" placeholder — still true — is all there is.
  let defaults = $state<Partial<Record<AiProvider, string>>>({});

  let testing = $state(false);
  let testOk = $state(false);
  let testError = $state<string | null>(null);

  const provider = $derived<AiProvider>(general?.ai_provider ?? 'anthropic');
  const isLocal = $derived(provider === 'local');
  const allowed = $derived(general?.allow_ai_translation ?? false);
  const providerOptions = $derived([
    { value: 'anthropic', label: $t('settings.aiTranslation.providerAnthropic') },
    { value: 'gemini', label: $t('settings.aiTranslation.providerGemini') },
    { value: 'groq', label: $t('settings.aiTranslation.providerGroq') },
    { value: 'local', label: $t('settings.aiTranslation.providerLocal') },
  ]);

  // Where each hosted provider hands out keys. Opened through the opener
  // plugin, like the CurseForge console links — not a network call.
  const KEY_URLS: Record<Exclude<AiProvider, 'local'>, string> = {
    anthropic: 'https://console.anthropic.com/settings/keys',
    gemini: 'https://aistudio.google.com/app/apikey',
    groq: 'https://console.groq.com/keys',
  };
  const keyUrl = $derived(isLocal ? null : KEY_URLS[provider as Exclude<AiProvider, 'local'>]);
  const keyHost = $derived(keyUrl ? new URL(keyUrl).host : '');
  function openKeyPage() {
    const url = keyUrl;
    if (!url) return;
    void openExternalHttps(url);
  }

  const defaultModel = $derived(isLocal ? undefined : defaults[provider]);
  const modelPlaceholder = $derived(
    isLocal
      ? $t('settings.aiTranslation.modelPlaceholderLocal')
      : defaultModel
        ? $t('settings.aiTranslation.modelPlaceholderCloudNamed', { model: defaultModel })
        : $t('settings.aiTranslation.modelPlaceholderCloud'),
  );
  const modelHint = $derived(
    isLocal
      ? $t('settings.aiTranslation.modelHintLocal')
      : defaultModel
        ? $t('settings.aiTranslation.modelHintCloudNamed', { model: defaultModel })
        : $t('settings.aiTranslation.modelHintCloud'),
  );

  // Fact A for the key field: three words, or "couldn't check" with the
  // keyring's reason as the detail line.
  const keyStatus = $derived.by((): FieldStatus => {
    if (keyStored === true) {
      return { text: $t('settings.aiTranslation.keyStatusStored'), tone: 'success' };
    }
    if (keyStored === false) {
      return { text: $t('settings.aiTranslation.keyStatusMissing'), tone: 'secondary' };
    }
    if (keyStored === null) {
      return {
        text: $t('settings.aiTranslation.keyStatusChecking'),
        tone: 'placeholder',
        busy: true,
      };
    }
    return { text: $t('settings.aiTranslation.keyStatusUnknown'), tone: 'warning' };
  });
  const keyStatusDetail = $derived(
    keyStored !== null && typeof keyStored === 'object' ? keyStored.unknown : undefined,
  );

  $effect(() => {
    // A bare invoke (no Result): a rejection is an IPC failure, and the
    // answer is cosmetic — see `defaults`.
    void (async () => {
      try {
        const list = await commands.l10nPrefillProviderDefaults();
        const next: Partial<Record<AiProvider, string>> = {};
        for (const d of list) next[d.provider] = d.model;
        defaults = next;
      } catch {
        // Deliberately nothing: the generic placeholder is still true, and
        // the settings-load error line is not this read's to borrow.
      }
    })();
  });

  // Whether a key is stored is a fact about the provider, so it is re-read
  // whenever the provider changes. `provider` is a $derived, which only
  // notifies on an actual change — reloading `general` with the same provider
  // does not fire a second read.
  $effect(() => {
    const p = provider;
    // Everything below the picker describes ONE provider, so none of it may
    // survive a switch: a stale "Stored" badge claims a credential that may not
    // exist, and a key typed for the old provider would be filed under the new
    // one — i.e. sent to a different third party than the user pasted it for.
    keyStored = null;
    keyError = null;
    pendingKey = '';
    testOk = false;
    testError = null;
    if (p === 'local') {
      // Local needs no credential; there is nothing to report.
      return;
    }
    let stale = false;
    void (async () => {
      const r = await commands.l10nPrefillKeyStatus(p);
      if (stale) return;
      if (r.status === 'ok') keyStored = r.data;
      else keyStored = { unknown: formatError(r.error) };
    })();
    return () => {
      stale = true;
    };
  });

  async function saveKey() {
    const trimmed = pendingKey.trim();
    if (trimmed === '') return;
    savingKey = true;
    keyError = null;
    testOk = false;
    testError = null;
    try {
      const r = await commands.l10nPrefillSetKey(provider, trimmed);
      if (r.status === 'ok') {
        pendingKey = '';
        keyStored = true;
      } else {
        keyError = formatError(r.error);
      }
    } finally {
      savingKey = false;
    }
  }

  async function clearKey() {
    clearingKey = true;
    keyError = null;
    testOk = false;
    testError = null;
    try {
      // An empty key clears the keyring slot — the command's own convention.
      const r = await commands.l10nPrefillSetKey(provider, '');
      if (r.status === 'ok') keyStored = false;
      else keyError = formatError(r.error);
    } finally {
      clearingKey = false;
    }
  }

  async function testConnection() {
    testing = true;
    testOk = false;
    testError = null;
    try {
      // Takes no provider argument: it tests whatever is configured, which is
      // why the copy tells the user to save first.
      const r = await commands.l10nPrefillTestKey();
      if (r.status === 'ok') testOk = true;
      // A provider's answer is described by what it means (a bad key, a wrong
      // model, a rate limit…) and names the test request; anything else is
      // the usual formatting.
      else testError = providerFailureOrNull(r.error, 'test') ?? formatError(r.error);
    } finally {
      testing = false;
    }
  }
</script>

{#snippet keyGuide()}
  {#if keyUrl}
    <p class="text-xs text-secondary mb-2">
      <button
        type="button"
        class="btn-link inline-flex items-center gap-1"
        onclick={openKeyPage}
        data-testid="ai-key-get"
      >
        {$t('settings.aiTranslation.getKey')} — {keyHost}
        <Icon name="externalLink" size={12} />
      </button>
    </p>
  {/if}
{/snippet}

<div class="flex flex-col gap-3">
  <h3 class="font-medium text-sm text-primary">{$t('settings.aiTranslation.title')}</h3>

  <p class="text-sm text-secondary">{$t('settings.aiTranslation.aboutBody')}</p>

  <p class="text-xs text-muted" data-testid="ai-translation-scope">
    {$t('settings.aiTranslation.scopeNote')}
  </p>

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

  <label class="flex items-start gap-2 cursor-pointer">
    <input
      type="checkbox"
      class="mt-0.5"
      checked={allowed}
      disabled={!settingsLoaded}
      onchange={(e) => void patchGeneral({ allow_ai_translation: e.currentTarget.checked })}
      data-testid="ai-translation-toggle"
    />
    <span class="flex-1">
      <span class="text-sm text-primary">{$t('settings.aiTranslation.consentLabel')}</span>
      <span class="block text-xs text-muted">
        {$t('settings.aiTranslation.consentDescription')}
      </span>
      <span class="block text-xs text-muted" data-testid="ai-cost-note">
        {$t('settings.aiTranslation.costNote')}
      </span>
      <span class="block text-xs text-warning-text">
        {$t('settings.aiTranslation.privacyNote')}
      </span>
    </span>
  </label>

  <div data-testid="save-failure-allow_ai_translation">
    <StatusMessage message={consentFailure} tone="danger" />
  </div>

  {#if !allowed}
    <p class="text-xs text-muted" data-testid="ai-gated-note">
      {$t('settings.aiTranslation.gatedNote')}
    </p>
  {/if}

  <div class="flex flex-col gap-1">
    <span class="text-sm text-primary">{$t('settings.aiTranslation.providerLabel')}</span>
    <Select
      class="text-sm w-full"
      dataTestid="ai-provider-select"
      ariaLabel={$t('settings.aiTranslation.providerLabel')}
      value={provider}
      options={providerOptions}
      disabled={!allowed || !settingsLoaded}
      onChange={(v) => {
        void patchGeneral({ ai_provider: v as AiProvider });
      }}
    />
  </div>

  <label class="flex flex-col gap-1">
    <span class="text-sm text-primary">{$t('settings.aiTranslation.modelLabel')}</span>
    <input
      type="text"
      class="w-full border border-border-emphasis rounded px-3 py-1.5 text-sm font-mono disabled:opacity-50 disabled:cursor-not-allowed"
      placeholder={modelPlaceholder}
      value={general?.ai_model ?? ''}
      disabled={!allowed || !settingsLoaded}
      onchange={(e) => {
        void patchGeneral({ ai_model: e.currentTarget.value.trim() });
      }}
      data-testid="ai-model-input"
    />
    <span class="text-xs text-muted">{modelHint}</span>
  </label>

  {#if isLocal}
    <p class="text-xs text-muted" data-testid="ai-local-expects">
      {$t('settings.aiTranslation.localExpects')}
    </p>
    <!-- The primitive refuses an out-of-range port instead of clamping it, and
         says what is accepted; nothing is patched until a value is accepted. -->
    <NumberField
      label={$t('settings.aiTranslation.localPortLabel')}
      value={general?.ai_local_port ?? DEFAULT_LOCAL_PORT}
      min={MIN_PORT}
      max={MAX_PORT}
      hint={$t('settings.aiTranslation.localPortHint')}
      disabled={!allowed || !settingsLoaded}
      onCommit={(n) => void patchGeneral({ ai_local_port: n })}
      testId="ai-local-port-input"
    />
    <p class="text-xs text-warning-text" data-testid="ai-local-note">
      {$t('settings.aiTranslation.localNote')}
    </p>
  {:else}
    <ApiKeyField
      statusLabel={$t('settings.aiTranslation.keyStatusLabel')}
      status={keyStatus}
      statusDetail={keyStatusDetail}
      guide={keyGuide}
      inputLabel={$t('settings.aiTranslation.keyLabel')}
      placeholder={$t('settings.aiTranslation.keyPlaceholder')}
      bind:value={pendingKey}
      disabled={!allowed || !settingsLoaded}
      saveLabel={$t('settings.aiTranslation.saveKey')}
      onSave={saveKey}
      saving={savingKey}
      clearLabel={keyStored === true ? $t('settings.aiTranslation.clearKey') : undefined}
      onClear={keyStored === true ? clearKey : undefined}
      clearing={clearingKey}
      result={keyError ? { tone: 'danger', text: keyError } : null}
      resultTestId="ai-key-error"
      note={$t('settings.aiTranslation.keyringNote')}
      testIdPrefix="ai-key"
    />
  {/if}

  <div class="flex flex-col gap-1">
    <div class="flex items-center gap-2">
      <BusyButton
        type="button"
        class="btn-secondary btn-sm"
        busy={testing}
        disabled={!allowed || !settingsLoaded}
        onclick={testConnection}
        data-testid="ai-test-connection"
      >
        {$t('settings.aiTranslation.testButton')}
      </BusyButton>
      {#if testOk}
        <div data-testid="ai-test-ok">
          <StatusMessage message={$t('settings.aiTranslation.testOk')} tone="success" />
        </div>
      {/if}
    </div>
    <!-- A disabled control has to say why. Without the permission the button
         is off because the test is a real outbound request, and the normal
         hint ("save a new key first") points at the wrong thing entirely. -->
    <span class="text-xs text-muted" data-testid="ai-test-hint">
      {$t(allowed ? 'settings.aiTranslation.testHint' : 'settings.aiTranslation.testHintDisabled')}
    </span>
    {#if testError}
      <div data-testid="ai-test-error">
        <StatusMessage message={testError} tone="danger" />
      </div>
    {/if}
  </div>

  <p class="text-xs text-muted" data-testid="ai-used-from">
    {$t('settings.aiTranslation.usedFrom')}
  </p>
</div>
