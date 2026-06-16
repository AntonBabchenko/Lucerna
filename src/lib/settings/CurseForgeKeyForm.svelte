<script lang="ts">
  // CurseForge API key management form — Task 19 of the v0.5.0 mod
  // browser plan. Rendered inside SettingsModal's CurseForge tab.
  //
  // Loads the current key status on mount via mods_get_curseforge_key_status.
  // On Save, calls mods_set_curseforge_key (which pings api.curseforge.com to
  // validate the key before persisting it to the OS keyring). On success the
  // status flips to 'set' and the input clears; on rejection the status
  // flips to 'invalid' and the typed error is rendered through formatError.
  // The Clear button (only visible when a key is stored or invalid) wipes
  // the keyring entry via mods_clear_curseforge_key.
  //
  // All three IPC calls follow the result-status pattern (typedError) — no
  // try/catch around them.
  import { commands, type KeyStatus } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { cfKeyVersion } from './state.svelte';
  import { cfKeyErrorStatus } from './cf-key-status';

  let status = $state<KeyStatus | 'loading' | 'unverified'>('loading');
  let pendingKey = $state('');
  let saving = $state(false);
  let error = $state<string | null>(null);

  async function refresh() {
    const result = await commands.modsGetCurseforgeKeyStatus();
    if (result.status === 'ok') {
      status = result.data;
    } else {
      error = formatError(result.error);
    }
  }

  $effect(() => {
    void refresh();
  });

  async function save() {
    const trimmed = pendingKey.trim();
    if (trimmed === '') return;
    saving = true;
    error = null;
    const result = await commands.modsSetCurseforgeKey(trimmed);
    if (result.status === 'ok') {
      pendingKey = '';
      await refresh();
      // Notify watchers (Mod browser banner, etc.) that the key
      // transitioned to a usable state.
      cfKeyVersion.value++;
    } else {
      error = formatError(result.error);
      const pill = cfKeyErrorStatus(result.error);
      if (pill === 'invalid') {
        // The key was genuinely rejected — reflect that and re-arm the banner.
        status = 'invalid';
        cfKeyVersion.value++;
      } else {
        // Reachability failure (region/Cloudflare/network): we don't know if
        // the key is valid. Show 'unverified' without calling refresh() —
        // refresh() would overwrite status with the stored 'set'/'missing'.
        // Leave the stored status and dependent banners (cfKeyVersion) untouched.
        status = 'unverified';
      }
    }
    saving = false;
  }

  async function clear() {
    const result = await commands.modsClearCurseforgeKey();
    if (result.status === 'ok') {
      await refresh();
      cfKeyVersion.value++;
    } else {
      error = formatError(result.error);
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

<div>
  <p class="text-sm text-secondary mb-3">{$t('settings.curseforge.aboutBody')}</p>
  <div class="text-sm mb-3">
    <span class="text-muted">{$t('settings.curseforge.statusLabel')} </span>
    {#if status === 'set'}
      <span class="text-success font-medium">{$t('settings.curseforge.statusOk')}</span>
    {:else if status === 'invalid'}
      <span class="text-danger font-medium">{$t('settings.curseforge.statusInvalid')}</span>
    {:else if status === 'unverified'}
      <span class="text-warning-text font-medium">{$t('settings.curseforge.statusUnverified')}</span
      >
    {:else if status === 'missing'}
      <span class="text-secondary">{$t('settings.curseforge.statusMissing')}</span>
    {:else}
      <span class="text-placeholder">{$t('settings.curseforge.statusChecking')}</span>
    {/if}
  </div>

  {#if status === 'missing'}
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
  {:else}
    <p class="text-xs text-secondary mb-3">
      {$t('settings.curseforge.replaceHintBefore')}
      <span class="font-medium">{$t('settings.curseforge.replaceAction')}</span>.
      {$t('settings.curseforge.replaceHintAfter')}
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

  <label class="block">
    <span class="text-xs text-muted"
      >{status === 'missing'
        ? $t('settings.curseforge.inputLabelNew')
        : $t('settings.curseforge.inputLabelReplace')}</span
    >
    <input
      type="password"
      class="w-full border border-border-emphasis rounded px-3 py-1.5 text-sm font-mono"
      placeholder="$2a$10$..."
      bind:value={pendingKey}
      disabled={saving}
    />
  </label>

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mt-2">
      {error}
    </div>
  {/if}

  <div class="flex gap-2 mt-3">
    <button
      type="button"
      class="btn-primary btn-sm"
      disabled={saving || pendingKey.trim() === ''}
      onclick={save}
    >
      {status === 'missing'
        ? $t('settings.curseforge.saveKey')
        : $t('settings.curseforge.updateKey')}
    </button>
    {#if status === 'set' || status === 'invalid'}
      <button type="button" class="btn-secondary btn-sm" onclick={clear}>
        {$t('settings.curseforge.clearKey')}
      </button>
    {/if}
  </div>

  <p class="text-xs text-muted mt-3">{$t('settings.curseforge.keyringNote')}</p>
</div>
