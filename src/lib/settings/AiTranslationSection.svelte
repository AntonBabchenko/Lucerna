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
  import { onMount } from 'svelte';
  import { commands, type AiProvider, type GeneralSettings } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';

  // Ollama's default. Matches `default_ai_local_port()` in instances/schema.rs.
  const DEFAULT_LOCAL_PORT = 11434;
  const MIN_PORT = 1;
  const MAX_PORT = 65535;

  let general = $state<GeneralSettings>({
    allow_ai_translation: false,
    ai_provider: 'anthropic',
    ai_model: '',
    ai_local_port: DEFAULT_LOCAL_PORT,
  });
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);

  let pendingKey = $state('');
  // `null` while the status read is still in flight — distinct from a known
  // "no key stored", which is what the user sees before they paste one.
  let keyStored = $state<boolean | null>(null);
  let keyError = $state<string | null>(null);
  let savingKey = $state(false);

  let testing = $state(false);
  let testOk = $state(false);
  let testError = $state<string | null>(null);

  const provider = $derived<AiProvider>(general.ai_provider ?? 'anthropic');
  const isLocal = $derived(provider === 'local');
  const allowed = $derived(general.allow_ai_translation ?? false);
  const providerOptions = $derived([
    { value: 'anthropic', label: $t('settings.aiTranslation.providerAnthropic') },
    { value: 'gemini', label: $t('settings.aiTranslation.providerGemini') },
    { value: 'groq', label: $t('settings.aiTranslation.providerGroq') },
    { value: 'local', label: $t('settings.aiTranslation.providerLocal') },
  ]);

  onMount(async () => {
    const r = await commands.appSettingsGet();
    if (r.status !== 'ok') {
      loadError = formatError(r.error);
      return;
    }
    // `#[serde(default)]` fields are optional in the generated type: an
    // app.json written before this feature has none of them at all. Normalise
    // once here so neither the markup nor `save` has to keep guessing.
    general = {
      ...r.data.general,
      allow_ai_translation: r.data.general.allow_ai_translation ?? false,
      ai_provider: r.data.general.ai_provider ?? 'anthropic',
      ai_model: r.data.general.ai_model ?? '',
      ai_local_port: r.data.general.ai_local_port ?? DEFAULT_LOCAL_PORT,
    };
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
      else keyError = formatError(r.error);
    })();
    return () => {
      stale = true;
    };
  });

  async function save() {
    saveError = null;
    // Snapshot the owned fields before any await, so an in-flight onMount or a
    // second change cannot land between the snapshot and the write.
    const allow = general.allow_ai_translation;
    const prov = general.ai_provider;
    const model = general.ai_model;
    const port = general.ai_local_port;
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      saveError = formatError(cur.error);
      return;
    }
    const next = {
      ...cur.data.general,
      allow_ai_translation: allow,
      ai_provider: prov,
      ai_model: model,
      ai_local_port: port,
    };
    const r = await commands.appSettingsSetGeneral(next);
    if (r.status !== 'ok') saveError = formatError(r.error);
  }

  // Clamping happens here rather than in the markup, and the field is written
  // back either way: a number input left empty (or clamped) would otherwise
  // sit there disagreeing with the port that is actually saved.
  function setPort(input: HTMLInputElement) {
    const parsed = Number.parseInt(input.value, 10);
    if (Number.isNaN(parsed)) {
      input.value = String(general.ai_local_port ?? DEFAULT_LOCAL_PORT);
      return;
    }
    const clamped = Math.min(MAX_PORT, Math.max(MIN_PORT, parsed));
    input.value = String(clamped);
    general.ai_local_port = clamped;
    void save();
  }

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
    savingKey = true;
    keyError = null;
    testOk = false;
    testError = null;
    try {
      // An empty key clears the keyring slot — the command's own convention.
      const r = await commands.l10nPrefillSetKey(provider, '');
      if (r.status === 'ok') keyStored = false;
      else keyError = formatError(r.error);
    } finally {
      savingKey = false;
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
      else testError = formatError(r.error);
    } finally {
      testing = false;
    }
  }
</script>

<div class="flex flex-col gap-3">
  <h3 class="font-medium text-sm text-primary">{$t('settings.aiTranslation.title')}</h3>

  <p class="text-sm text-secondary">{$t('settings.aiTranslation.aboutBody')}</p>

  <p class="text-xs text-muted" data-testid="ai-translation-scope">
    {$t('settings.aiTranslation.scopeNote')}
  </p>

  {#if loadError}
    <p class="text-xs text-danger">{loadError}</p>
  {/if}

  <label class="flex items-start gap-2 cursor-pointer">
    <input
      type="checkbox"
      class="mt-0.5"
      checked={allowed}
      onchange={(e) => {
        general.allow_ai_translation = e.currentTarget.checked;
        void save();
      }}
      data-testid="ai-translation-toggle"
    />
    <span class="flex-1">
      <span class="text-sm text-primary">{$t('settings.aiTranslation.consentLabel')}</span>
      <span class="block text-xs text-muted">
        {$t('settings.aiTranslation.consentDescription')}
      </span>
      <span class="block text-xs text-warning-text">
        {$t('settings.aiTranslation.privacyNote')}
      </span>
    </span>
  </label>

  {#if saveError}
    <p class="text-xs text-danger">{saveError}</p>
  {/if}

  <div class="flex flex-col gap-1">
    <span class="text-sm text-primary">{$t('settings.aiTranslation.providerLabel')}</span>
    <Select
      class="text-sm w-full"
      dataTestid="ai-provider-select"
      ariaLabel={$t('settings.aiTranslation.providerLabel')}
      value={provider}
      options={providerOptions}
      onChange={(v) => {
        general.ai_provider = v as AiProvider;
        void save();
      }}
    />
  </div>

  <label class="flex flex-col gap-1">
    <span class="text-sm text-primary">{$t('settings.aiTranslation.modelLabel')}</span>
    <input
      type="text"
      class="w-full border border-border-emphasis rounded px-3 py-1.5 text-sm font-mono"
      placeholder={isLocal
        ? $t('settings.aiTranslation.modelPlaceholderLocal')
        : $t('settings.aiTranslation.modelPlaceholderCloud')}
      value={general.ai_model ?? ''}
      onchange={(e) => {
        general.ai_model = e.currentTarget.value.trim();
        void save();
      }}
      data-testid="ai-model-input"
    />
    <span class="text-xs text-muted">
      {isLocal
        ? $t('settings.aiTranslation.modelHintLocal')
        : $t('settings.aiTranslation.modelHintCloud')}
    </span>
  </label>

  {#if isLocal}
    <label class="flex flex-col gap-1">
      <span class="text-sm text-primary">{$t('settings.aiTranslation.localPortLabel')}</span>
      <input
        type="number"
        min={MIN_PORT}
        max={MAX_PORT}
        class="w-full border border-border-emphasis rounded px-3 py-1.5 text-sm font-mono"
        value={general.ai_local_port ?? DEFAULT_LOCAL_PORT}
        onchange={(e) => setPort(e.currentTarget)}
        data-testid="ai-local-port-input"
      />
      <span class="text-xs text-muted">{$t('settings.aiTranslation.localPortHint')}</span>
    </label>
    <p class="text-xs text-warning-text" data-testid="ai-local-note">
      {$t('settings.aiTranslation.localNote')}
    </p>
  {:else}
    <div class="flex flex-col gap-2">
      <p class="text-sm">
        <span class="text-muted">{$t('settings.aiTranslation.keyStatusLabel')} </span>
        {#if keyStored === true}
          <span class="text-success font-medium" data-testid="ai-key-status">
            {$t('settings.aiTranslation.keyStatusStored')}
          </span>
        {:else if keyStored === false}
          <span class="text-secondary" data-testid="ai-key-status">
            {$t('settings.aiTranslation.keyStatusMissing')}
          </span>
        {:else}
          <span class="text-placeholder" data-testid="ai-key-status">
            {$t('settings.aiTranslation.keyStatusChecking')}
          </span>
        {/if}
      </p>

      <label class="flex flex-col gap-1">
        <span class="text-xs text-muted">{$t('settings.aiTranslation.keyLabel')}</span>
        <input
          type="password"
          class="w-full border border-border-emphasis rounded px-3 py-1.5 text-sm font-mono"
          placeholder={$t('settings.aiTranslation.keyPlaceholder')}
          bind:value={pendingKey}
          disabled={savingKey}
          data-testid="ai-key-input"
        />
      </label>

      {#if keyError}
        <p class="text-xs text-danger" role="alert" data-testid="ai-key-error">{keyError}</p>
      {/if}

      <div class="flex gap-2">
        <BusyButton
          type="button"
          class="btn-primary btn-sm"
          busy={savingKey}
          disabled={pendingKey.trim() === ''}
          onclick={saveKey}
          data-testid="ai-key-save"
        >
          {$t('settings.aiTranslation.saveKey')}
        </BusyButton>
        {#if keyStored === true}
          <BusyButton
            type="button"
            class="btn-secondary btn-sm"
            busy={savingKey}
            onclick={clearKey}
            data-testid="ai-key-clear"
          >
            {$t('settings.aiTranslation.clearKey')}
          </BusyButton>
        {/if}
      </div>

      <p class="text-xs text-muted">{$t('settings.aiTranslation.keyringNote')}</p>
    </div>
  {/if}

  <div class="flex flex-col gap-1">
    <div class="flex items-center gap-2">
      <BusyButton
        type="button"
        class="btn-secondary btn-sm"
        busy={testing}
        disabled={!allowed}
        onclick={testConnection}
        data-testid="ai-test-connection"
      >
        {$t('settings.aiTranslation.testButton')}
      </BusyButton>
      {#if testOk}
        <span class="text-xs text-success" data-testid="ai-test-ok">
          {$t('settings.aiTranslation.testOk')}
        </span>
      {/if}
    </div>
    <!-- A disabled control has to say why. Without the permission the button
         is off because the test is a real outbound request, and the normal
         hint ("save a new key first") points at the wrong thing entirely. -->
    <span class="text-xs text-muted" data-testid="ai-test-hint">
      {$t(allowed ? 'settings.aiTranslation.testHint' : 'settings.aiTranslation.testHintDisabled')}
    </span>
    {#if testError}
      <p class="text-xs text-danger" role="alert" data-testid="ai-test-error">{testError}</p>
    {/if}
  </div>
</div>
