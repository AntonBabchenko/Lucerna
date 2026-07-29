<script lang="ts">
  // Settings → Integrations: opt-in `lucerna://` link handling.
  //
  // Off by default, and nothing is written to the OS until the user turns it on
  // here — the exact registry key is spelled out next to the toggle rather than
  // asking for trust. Turning it off removes the key again. Import-by-link still
  // works without this: the user can paste a link into the import dialog.
  import { onMount } from 'svelte';
  import { commands, type SchemeState } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

  let enabled = $state(false);
  let schemeState = $state<SchemeState | null>(null);
  let registryKey = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  const unsupported = $derived(schemeState === 'unsupported');
  // Enabled, but the OS points at a different exe (moved install / portable
  // copy) and the startup self-heal did not fix it. Offer to re-assert rather
  // than leaving links opening a binary that is no longer there.
  const stale = $derived(enabled && schemeState === 'registered_to_other_path');

  onMount(async () => {
    registryKey = await commands.urlSchemeKey();
    const s = await commands.appSettingsGet();
    // `?? false` because specta types `#[serde(default)]` fields as optional:
    // an app.json written before this field existed has no value at all.
    if (s.status === 'ok') enabled = s.data.general.register_url_scheme ?? false;
    await refreshState();
  });

  async function refreshState() {
    const r = await commands.urlSchemeState();
    schemeState = r.status === 'ok' ? r.data : null;
  }

  /** Read-modify-write of app.json so this panel only owns its own field. */
  async function persist(next: boolean) {
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      error = formatError(cur.error);
      return false;
    }
    const r = await commands.appSettingsSetGeneral({
      ...cur.data.general,
      register_url_scheme: next,
    });
    if (r.status !== 'ok') {
      error = formatError(r.error);
      return false;
    }
    return true;
  }

  async function toggle(next: boolean) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      // Touch the OS first: if registration fails there is nothing to persist,
      // and the checkbox must not claim a state the machine is not in.
      const r = next ? await commands.urlSchemeRegister() : await commands.urlSchemeUnregister();
      if (r.status !== 'ok') {
        error = $t('settings.general.urlScheme.failed', { message: formatError(r.error) });
        enabled = !next;
        return;
      }
      if (!(await persist(next))) {
        enabled = !next;
        return;
      }
      enabled = next;
      await refreshState();
    } finally {
      busy = false;
    }
  }

  async function reassert() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const r = await commands.urlSchemeRegister();
      if (r.status !== 'ok') {
        error = $t('settings.general.urlScheme.failed', { message: formatError(r.error) });
        return;
      }
      await refreshState();
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-3">
  <h3 class="font-medium text-sm text-primary">{$t('settings.general.urlScheme.title')}</h3>

  <label class="flex items-start gap-2" class:cursor-pointer={!unsupported}>
    <input
      type="checkbox"
      class="mt-0.5"
      checked={enabled}
      disabled={unsupported || busy}
      onchange={(e) => void toggle(e.currentTarget.checked)}
      data-testid="url-scheme-toggle"
    />
    <span class="flex-1">
      <span class="text-sm text-primary">{$t('settings.general.urlScheme.label')}</span>
      <span class="block text-xs text-muted">
        {$t('settings.general.urlScheme.description')}
      </span>
    </span>
  </label>

  {#if unsupported}
    <p class="text-xs text-muted" data-testid="url-scheme-unsupported">
      {$t('settings.general.urlScheme.unsupported')}
    </p>
  {:else if registryKey}
    <p class="flex items-start gap-2 text-xs text-muted" data-testid="url-scheme-registry-hint">
      <Icon name="info" size={14} class="mt-0.5 shrink-0" />
      <span>{$t('settings.general.urlScheme.registryHint', { key: registryKey })}</span>
    </p>
  {/if}

  {#if stale}
    <div class="flex items-center gap-2" data-testid="url-scheme-stale">
      <p class="flex-1 text-xs text-warning-text">
        {$t('settings.general.urlScheme.repointed')}
      </p>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={busy}
        onclick={() => void reassert()}
        data-testid="url-scheme-reassert"
      >
        {$t('settings.general.urlScheme.reassert')}
      </button>
    </div>
  {/if}

  {#if error}
    <p class="text-xs text-danger" role="alert" data-testid="url-scheme-error">{error}</p>
  {/if}
</div>
