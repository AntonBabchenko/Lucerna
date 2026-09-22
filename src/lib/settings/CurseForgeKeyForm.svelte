<script lang="ts">
  // CurseForge API key form — Settings → Integrations. Two facts, never one
  // slot for both:
  //
  // Fact A — what is STORED: the status line, read through
  // mods_get_curseforge_key_status. It names the source of the key (your own
  // key, Lucerna's built-in key, none) or says the keyring could not be
  // checked, and it changes only on a re-read: after a successful Save, after
  // Clear, on Check again.
  //
  // Fact B — what HAPPENED to what you just typed: the result under the field.
  // A rejected or unsaved candidate lands here and never relabels Fact A; it
  // is cleared at the start of the next Save or Clear.
  //
  // On a build with its own key the whole own-key path (steps, field, Save)
  // is a closed "Use my own key" disclosure — entering a key is optional
  // there. Clear appears only for your own key. All three IPC calls follow
  // the result-status pattern (typedError) — no try/catch around them.
  import { commands, type KeyStatus } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon } from '$lib/ui/icons';
  import { cfKeyVersion } from './state.svelte';
  import { cfKeyErrorStatus } from './cf-key-status';
  import ApiKeyField from './ApiKeyField.svelte';
  import type { FieldResult, FieldStatus, StatusTone } from './api-key-field';

  type Stored =
    | { kind: 'loading' }
    | { kind: 'status'; status: KeyStatus }
    // The command itself failed — not a status, "could not check".
    | { kind: 'failed'; reason: string };

  let stored = $state<Stored>({ kind: 'loading' });
  let pendingKey = $state('');
  let saving = $state(false);
  let clearing = $state(false);
  let result = $state<FieldResult | null>(null);

  async function refresh() {
    const r = await commands.modsGetCurseforgeKeyStatus();
    stored =
      r.status === 'ok'
        ? { kind: 'status', status: r.data }
        : { kind: 'failed', reason: formatError(r.error) };
  }

  $effect(() => {
    void refresh();
  });

  // One line per variant — a new KeyStatus is a compile error here, not a
  // wrong pill. `set` is the only state that says the key is the user's.
  const STATUS_LINE = {
    set: { key: 'settings.curseforge.statusOwn', tone: 'success' },
    set_builtin: { key: 'settings.curseforge.statusBuiltin', tone: 'secondary' },
    missing: { key: 'settings.curseforge.statusNone', tone: 'secondary' },
    unknown: { key: 'settings.curseforge.statusUnknown', tone: 'warning' },
    unknown_embedded: { key: 'settings.curseforge.statusUnknownBuiltin', tone: 'warning' },
  } satisfies Record<KeyStatus, { key: TranslationKey; tone: StatusTone }>;

  const statusOf = $derived(stored.kind === 'status' ? stored.status : null);
  const hasOwnKey = $derived(statusOf === 'set');
  const builtinServes = $derived(statusOf === 'set_builtin' || statusOf === 'unknown_embedded');
  const couldNotCheck = $derived(
    stored.kind === 'failed' || statusOf === 'unknown' || statusOf === 'unknown_embedded',
  );
  // The steps are for someone without their own key; while loading, nothing
  // presumes either way.
  const showSteps = $derived(stored.kind === 'failed' || (statusOf !== null && !hasOwnKey));

  const statusView = $derived.by((): FieldStatus => {
    if (stored.kind === 'loading') {
      return { text: $t('settings.curseforge.statusChecking'), tone: 'placeholder', busy: true };
    }
    if (stored.kind === 'failed') {
      return { text: $t('settings.curseforge.statusUnknown'), tone: 'warning' };
    }
    const line = STATUS_LINE[stored.status];
    return { text: $t(line.key), tone: line.tone };
  });

  async function save() {
    const trimmed = pendingKey.trim();
    if (trimmed === '') return;
    saving = true;
    result = null;
    const r = await commands.modsSetCurseforgeKey(trimmed);
    if (r.status === 'ok') {
      pendingKey = '';
      result = { tone: 'info', text: $t('settings.curseforge.resultSaved') };
      await refresh();
      // The stored state changed: the banners re-read it.
      cfKeyVersion.value++;
    } else {
      const outcome = cfKeyErrorStatus(r.error);
      // Fact B only. The stored key is whatever it was — mods_set_curseforge_key
      // persists only after CurseForge accepted the candidate, and a keyring
      // that refused to keep an accepted one did not change what is stored.
      result =
        outcome === 'invalid'
          ? { tone: 'danger', text: $t('settings.curseforge.resultRejected') }
          : outcome === 'not_saved'
            ? {
                tone: 'warning',
                text: `${$t('settings.curseforge.resultNotSaved')} ${formatError(r.error)}`,
              }
            : {
                tone: 'warning',
                text: `${$t('settings.curseforge.resultUnreachable')} ${formatError(r.error)}`,
              };
    }
    saving = false;
  }

  async function clear() {
    clearing = true;
    result = null;
    try {
      const r = await commands.modsClearCurseforgeKey();
      if (r.status === 'ok') {
        await refresh();
        cfKeyVersion.value++;
      } else {
        result = { tone: 'danger', text: formatError(r.error) };
      }
    } finally {
      clearing = false;
    }
  }

  function openConsoleHome() {
    // Land on the console homepage so the login flow has a sane
    // redirect target. Deep-linking to /#/api-keys before login throws
    // the user into the wrong section after sign-in.
    void import('@tauri-apps/plugin-opener').then((m) =>
      m.openUrl('https://console.curseforge.com/'),
    );
  }

  function openApiKeysPage() {
    void import('@tauri-apps/plugin-opener').then((m) =>
      m.openUrl('https://console.curseforge.com/#/api-keys'),
    );
  }
</script>

{#snippet guide()}
  {#if showSteps}
    <ol class="text-sm text-secondary list-decimal pl-5 space-y-1 mb-3">
      <li>
        {$t('settings.curseforge.step1Before')}
        <button
          type="button"
          class="btn-tertiary font-mono inline-flex items-center gap-1"
          onclick={openConsoleHome}
        >
          console.curseforge.com
          <Icon name="externalLink" size={12} />
        </button>
        {$t('settings.curseforge.step1After')}
      </li>
      <li>
        {$t('settings.curseforge.step2Before')}
        <button
          type="button"
          class="btn-tertiary font-mono inline-flex items-center gap-1"
          onclick={openApiKeysPage}
        >
          API Keys
          <Icon name="externalLink" size={12} />
        </button>
        {$t('settings.curseforge.step2After')}
      </li>
      <li>{$t('settings.curseforge.step3')}</li>
      <li>{$t('settings.curseforge.step4')}</li>
    </ol>
  {:else if hasOwnKey}
    <p class="text-xs text-secondary mb-3">
      {$t('settings.curseforge.getOneAt')}
      <button
        type="button"
        class="btn-tertiary font-mono inline-flex items-center gap-1"
        onclick={openApiKeysPage}
      >
        console.curseforge.com → API Keys
        <Icon name="externalLink" size={12} />
      </button>.
    </p>
  {/if}
{/snippet}

<div>
  <p class="text-sm text-secondary mb-3">{$t('settings.curseforge.aboutBody')}</p>
  <ApiKeyField
    heading={$t('settings.curseforge.title')}
    statusLabel={$t('settings.curseforge.statusLabel')}
    status={statusView}
    statusDetail={stored.kind === 'failed' ? stored.reason : undefined}
    statusAction={couldNotCheck
      ? { label: $t('settings.curseforge.retryBtn'), onClick: () => void refresh() }
      : undefined}
    collapsed={builtinServes
      ? { summary: $t('settings.curseforge.useOwnKey'), testId: 'cf-key-own-key' }
      : undefined}
    {guide}
    inputLabel={hasOwnKey
      ? $t('settings.curseforge.inputLabelReplace')
      : $t('settings.curseforge.inputLabelNew')}
    placeholder="$2a$10$..."
    bind:value={pendingKey}
    disabled={stored.kind === 'loading'}
    saveLabel={hasOwnKey ? $t('settings.curseforge.updateKey') : $t('settings.curseforge.saveKey')}
    onSave={save}
    {saving}
    clearLabel={hasOwnKey ? $t('settings.curseforge.clearKey') : undefined}
    onClear={hasOwnKey ? clear : undefined}
    {clearing}
    clearCaption={hasOwnKey ? $t('settings.curseforge.clearCaption') : undefined}
    {result}
    resultTestId="cf-key-result"
    note={$t('settings.curseforge.keyringNote')}
    testIdPrefix="cf-key"
  />
</div>
