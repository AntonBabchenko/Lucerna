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
  import { cfKeyVersion } from './state.svelte';

  let status = $state<KeyStatus | 'loading'>('loading');
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
      status = 'invalid';
      // Also notify watchers — the key went bad and the banner should
      // come back.
      cfKeyVersion.value++;
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
  <div class="text-sm mb-3">
    <span class="text-neutral-500">Status: </span>
    {#if status === 'set'}
      <span class="text-green-700 font-medium">OK — key is set</span>
    {:else if status === 'invalid'}
      <span class="text-red-700 font-medium">Invalid — please enter a new key</span>
    {:else if status === 'missing'}
      <span class="text-neutral-700">Not configured</span>
    {:else}
      <span class="text-neutral-400">Checking…</span>
    {/if}
  </div>

  {#if status === 'missing'}
    <ol class="text-sm text-neutral-700 list-decimal pl-5 space-y-1 mb-3">
      <li>
        Open
        <button
          type="button"
          class="font-mono text-blue-600 hover:text-blue-700 hover:underline"
          onclick={openConsoleHome}
        >
          console.curseforge.com ↗
        </button>
        and sign in with a free account
      </li>
      <li>
        Once logged in, go to the
        <button
          type="button"
          class="font-mono text-blue-600 hover:text-blue-700 hover:underline"
          onclick={openApiKeysPage}
        >
          API Keys ↗
        </button>
        tab
      </li>
      <li>Generate a key for FTlauncher</li>
      <li>Paste it below</li>
    </ol>
  {:else}
    <p class="text-xs text-neutral-600 mb-3">
      To replace the stored key, paste a new one below and click <span class="font-medium"
        >Update key</span
      >. Get one at
      <button
        type="button"
        class="font-mono text-blue-600 hover:text-blue-700 hover:underline"
        onclick={openApiKeysPage}
      >
        console.curseforge.com → API Keys ↗
      </button>.
    </p>
  {/if}

  <label class="block">
    <span class="text-xs text-neutral-500"
      >{status === 'missing' ? 'Key' : 'New key (overwrites stored)'}</span
    >
    <input
      type="password"
      class="w-full border border-neutral-300 rounded px-3 py-1.5 text-sm font-mono"
      placeholder="$2a$10$..."
      bind:value={pendingKey}
      disabled={saving}
    />
  </label>

  {#if error}
    <div class="bg-red-50 border border-red-200 text-red-900 text-sm rounded p-2 mt-2">
      {error}
    </div>
  {/if}

  <div class="flex gap-2 mt-3">
    <button
      type="button"
      class="px-3 py-1 bg-blue-600 hover:bg-blue-700 text-white text-sm rounded disabled:opacity-50"
      disabled={saving || pendingKey.trim() === ''}
      onclick={save}
    >
      {status === 'missing' ? 'Save key' : 'Update key'}
    </button>
    {#if status === 'set' || status === 'invalid'}
      <button
        type="button"
        class="px-3 py-1 border border-neutral-300 text-sm rounded"
        onclick={clear}
      >
        Clear key
      </button>
    {/if}
  </div>

  <p class="text-xs text-neutral-500 mt-3">Stored in OS keyring (Windows Credential Manager).</p>
</div>
