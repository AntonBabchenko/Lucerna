<script lang="ts">
  // Settings → Privacy & network: one place that says what Lucerna contacts on
  // the user's say-so and whether they have allowed it. It owns no setting —
  // each row states the saved value and Change jumps to the control where it
  // lives, so a consent is only ever given next to its full explanation.
  // Nothing is claimed while the settings are unread: a default is not a fact.
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { t } from '$lib/i18n';
  import { providerDisplayName } from '$lib/l10n/provider-failure';
  import { Icon } from '$lib/ui/icons';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { appSettings, generalDisplayed, loadAppSettings } from './app-settings.svelte';
  import { dataLocation } from './data-location.svelte';
  import { PRIVACY_POLICY_URL } from './disclaimer';
  import type { SettingsAnchor } from './search-index';
  import SettingsField from './SettingsField.svelte';
  import { jumpInSettings } from './state.svelte';

  type Row = {
    id: 'serverPing' | 'ai' | 'updates';
    anchor: SettingsAnchor;
    nameKey: TranslationKey;
    body: string;
    state: string;
    tone: 'known' | 'checking' | 'unknown';
    note: string | null;
  };

  const general = $derived(generalDisplayed());
  const loadError = $derived(
    appSettings.loaded.kind === 'failed' ? appSettings.loaded.error : null,
  );

  /** The saved value in words, or why there is none to show. */
  function stateOf(value: boolean | undefined, on: TranslationKey, off: TranslationKey) {
    if (general !== null && value !== undefined) {
      return { state: $t(value ? on : off), tone: 'known' as const };
    }
    // Still reading → say so. A failed read, or a finished one without the
    // value, is "could not tell" — never a read in progress forever.
    return appSettings.loaded.kind === 'pending'
      ? { state: $t('settings.privacy.state.checking'), tone: 'checking' as const }
      : { state: $t('settings.privacy.state.unknown'), tone: 'unknown' as const };
  }

  /** Where AI translation sends the text: nowhere off this computer for the
   *  loopback provider, the named service otherwise, and no claim while the
   *  provider is unknown. */
  const aiBody = $derived.by(() => {
    const provider = general?.ai_provider;
    if (provider === undefined) return $t('settings.privacy.ai.bodyUnknown');
    if (provider === 'local') return $t('settings.privacy.ai.bodyLocal');
    return $t('settings.privacy.ai.bodyHosted', { provider: providerDisplayName(provider) });
  });

  const rows = $derived.by((): Row[] => {
    const updates = stateOf(
      general?.check_updates_on_startup,
      'settings.privacy.state.on',
      'settings.privacy.state.off',
    );
    return [
      {
        id: 'serverPing',
        anchor: 'game.serverPing',
        nameKey: 'settings.privacy.serverPing.name',
        body: $t('settings.privacy.serverPing.body'),
        ...stateOf(
          general?.allow_server_ping,
          'settings.privacy.state.allowed',
          'settings.privacy.state.notAllowed',
        ),
        note: null,
      },
      {
        id: 'ai',
        anchor: 'integrations.aiTranslation',
        nameKey: 'settings.privacy.ai.name',
        body: aiBody,
        ...stateOf(
          general?.allow_ai_translation,
          'settings.privacy.state.allowed',
          'settings.privacy.state.notAllowed',
        ),
        note: null,
      },
      {
        id: 'updates',
        anchor: 'updates.startupCheck',
        nameKey: 'settings.privacy.updates.name',
        body: $t('settings.privacy.updates.body'),
        ...updates,
        // A temporary session never dials at startup (the real settings are
        // out of reach) — say so only when that is known, and only when the
        // check would otherwise run.
        note:
          general?.check_updates_on_startup === true && dataLocation.fellBack
            ? $t('settings.privacy.updates.paused')
            : null,
      },
    ];
  });

  const TONE_CLASS: Record<Row['tone'], string> = {
    known: 'text-primary font-medium',
    checking: 'text-placeholder',
    unknown: 'text-warning-text',
  };
</script>

<SettingsField anchor="privacy.overview">
  <section class="flex flex-col gap-6">
    <div class="flex flex-col gap-3">
      <!-- The jump target when the About pointer lands here: focus reads the
           page's heading instead of the first row's Change. -->
      <h3 class="font-medium text-sm text-primary" tabindex="-1" data-flash-focus>
        {$t('settings.privacy.title')}
      </h3>
      <p class="text-sm text-secondary">{$t('settings.privacy.intro')}</p>

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

      <ul class="flex flex-col">
        {#each rows as row (row.id)}
          <li
            class="flex flex-col gap-1 py-3 border-t first:border-t-0"
            data-testid="privacy-row-{row.id}"
          >
            <div class="flex items-baseline justify-between gap-3">
              <span id="privacy-name-{row.id}" class="text-sm text-primary">{$t(row.nameKey)}</span>
              <span class="flex items-baseline gap-3 shrink-0">
                <span class="text-sm {TONE_CLASS[row.tone]}">{row.state}</span>
                <button
                  type="button"
                  class="btn-tertiary text-xs"
                  aria-describedby="privacy-name-{row.id}"
                  onclick={() => jumpInSettings(row.anchor)}
                >
                  {$t('settings.privacy.change')}
                </button>
              </span>
            </div>
            <p class="text-xs text-muted">{row.body}</p>
            {#if row.note}
              <p class="text-xs text-warning-text">{row.note}</p>
            {/if}
          </li>
        {/each}
      </ul>
    </div>

    <div class="flex flex-col items-start gap-1">
      <p class="text-xs text-muted">{$t('settings.privacy.footer')}</p>
      <button
        type="button"
        class="btn-link inline-flex items-center gap-1 text-xs"
        use:tooltip={PRIVACY_POLICY_URL}
        onclick={() => void openExternalHttps(PRIVACY_POLICY_URL)}
      >
        {$t('settings.about.privacyPolicy')}
        <Icon name="externalLink" size={12} />
      </button>
    </div>
  </section>
</SettingsField>
