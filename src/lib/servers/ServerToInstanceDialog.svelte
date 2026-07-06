<script lang="ts">
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { t } from '$lib/i18n';
  import { commands, type ServerWithStatus_Serialize } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';

  let {
    server,
    onCancel,
    onCreated,
  }: {
    server: ServerWithStatus_Serialize;
    onCancel: () => void;
    onCreated: (instanceId: string) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(server.name);
  let addToMultiplayer = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const address = $derived(`localhost:${server.port ?? 25565}`);

  // §7 fallback gating: this creates a new client instance, which would write
  // it into the wrong (temporary default) root while the configured data
  // root is unavailable. See data-root-gating.ts.
  const createDisabledReason = $derived.by(() => {
    const key = dataRootCreateDisabledKey(dataLocation.fellBack);
    return key === null ? null : $t(key);
  });

  async function create() {
    // Belt-and-braces: the button is also disabled via createDisabledReason.
    if (dataLocation.fellBack) return;
    const trimmed = name.trim();
    if (trimmed === '') {
      error = $t('servers.toInstance.nameRequired');
      return;
    }
    busy = true;
    error = null;
    try {
      const result = await commands.serverCreateClientInstance(
        server.id,
        trimmed,
        addToMultiplayer,
      );
      if (result.status === 'error') {
        error = formatError(result.error);
        return;
      }
      if (addToMultiplayer && !result.data.multiplayer_added) {
        pushWarning($t('servers.toInstance.successNoServer', { name: trimmed }));
      } else {
        pushSuccess($t('servers.toInstance.success', { name: trimmed }));
      }
      onCreated(result.data.instance.id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  ariaLabelledby="server-to-instance-title"
  onClose={onCancel}
  panelClass="w-[460px] p-5 flex flex-col gap-3"
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
>
  <h3 id="server-to-instance-title" class="text-base font-semibold text-primary">
    {$t('servers.toInstance.title')}
  </h3>

  <label class="block">
    <span class="text-xs text-muted">{$t('servers.toInstance.nameLabel')}</span>
    <input
      type="text"
      class="h-8 w-full rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
      bind:value={name}
      disabled={busy}
      data-testid="client-instance-name"
    />
  </label>

  <label class="flex cursor-pointer items-start gap-2">
    <input
      type="checkbox"
      class="mt-0.5"
      bind:checked={addToMultiplayer}
      disabled={busy}
      data-testid="add-to-multiplayer"
    />
    <span class="flex-1">
      <span class="text-sm text-primary">{$t('servers.toInstance.addToMultiplayer')}</span>
      <span class="block text-xs text-muted"
        >{$t('servers.toInstance.addressHint', { address })}</span
      >
    </span>
  </label>

  <p class="rounded bg-subtle px-3 py-2 text-xs text-muted">
    {$t('servers.toInstance.clientModsHint')}
  </p>

  {#if error}
    <p class="text-sm text-danger" data-testid="client-instance-error">{error}</p>
  {/if}

  <div class="mt-2 flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel} disabled={busy}>
      {$t('common.cancel')}
    </button>
    <span class="inline-flex" use:tooltip={{ text: createDisabledReason ?? '', describe: false }}>
      <BusyButton
        class="btn-primary btn-sm"
        {busy}
        disabled={createDisabledReason !== null}
        onclick={() => void create()}
      >
        {$t('servers.toInstance.create')}
      </BusyButton>
    </span>
  </div>
</Modal>
