<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import type { InstanceWithStatus, SavedServer, ShortcutTarget } from '$lib/ipc/bindings';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushActionToast } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import { Icon } from '$lib/ui/icons';

  // Create a desktop shortcut that launches this instance in one click —
  // optionally straight into a world or onto a server, on top of the existing
  // Quick Play support (which needs MC 1.20+, so those two targets are gated on
  // `instanceQuickPlaySupport`).
  //
  // The shortcut file carries argv, not a `lucerna://` URL: argv is the trusted
  // channel that may launch the game directly (see src-tauri/src/shortcuts).

  let { instance, onClose }: { instance: InstanceWithStatus; onClose: () => void } = $props();

  type TargetKind = 'instance' | 'world' | 'server';

  let kind = $state<TargetKind>('instance');
  let worldFolder = $state('');
  let serverAddress = $state('');
  let name = $state('');
  // True once the user edits the name — from then on we stop re-deriving it, so
  // a deliberate name is never overwritten by switching target.
  let nameEdited = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  let worlds = $state<string[] | null>(null);
  let servers = $state<SavedServer[] | null>(null);
  // null until resolved; false = pre-1.20 (no Quick Play), so world/server off.
  let quickPlay = $state<boolean | null>(null);

  onMount(() => {
    void (async () => {
      const support = await commands.instanceQuickPlaySupport(instance.id);
      quickPlay = support.status === 'ok' ? support.data : false;
      const w = await commands.listWorldNames(instance.id);
      if (w.status === 'ok') {
        worlds = w.data.map((e) => e.folder_name);
        worldFolder = worlds[0] ?? '';
      } else {
        worlds = [];
      }
      const s = await commands.listSavedServers(instance.id);
      if (s.status === 'ok') {
        servers = s.data;
        serverAddress = servers[0]?.address ?? '';
      } else {
        servers = [];
      }
      await refreshName();
    })();
  });

  const target = $derived<ShortcutTarget>(
    kind === 'world'
      ? { kind: 'world', instance_id: instance.id, folder: worldFolder }
      : kind === 'server'
        ? { kind: 'server', instance_id: instance.id, address: serverAddress }
        : { kind: 'instance', instance_id: instance.id },
  );

  /** Ask the backend for the default file name so the field matches what it
   * would pick — one source of truth for sanitisation, not a JS re-implementation. */
  async function refreshName() {
    if (nameEdited) return;
    name = await commands.shortcutDefaultName(instance.name, target);
  }

  function selectKind(next: TargetKind) {
    kind = next;
    error = null;
    void refreshName();
  }

  // Quick Play targets need a chosen value; the instance target always can go.
  const canSubmit = $derived(
    !busy &&
      name.trim().length > 0 &&
      (kind === 'instance' ||
        (kind === 'world' && worldFolder !== '') ||
        (kind === 'server' && serverAddress !== '')),
  );

  const quickPlayDisabled = $derived(quickPlay === false);

  async function create() {
    if (!canSubmit) return;
    busy = true;
    error = null;
    try {
      const r = await commands.shortcutCreate(target, name.trim());
      if (r.status !== 'ok') {
        error = formatError(r.error);
        return;
      }
      const path = r.data;
      pushActionToast(
        'success',
        get(t)('shortcut.created', { path }),
        {
          label: get(t)('shortcut.openFolder'),
          run: () => {
            void import('@tauri-apps/plugin-opener')
              .then((m) => m.revealItemInDir(path))
              .catch(() => {
                /* best-effort reveal: the toast already shows the full path */
              });
          },
        },
        [],
      );
      onClose();
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  {onClose}
  ariaLabelledby="create-shortcut-heading"
  dataTestid="create-shortcut-dialog"
  panelClass="max-w-md w-full flex flex-col"
>
  <header class="flex items-center justify-between border-b border-border-subtle px-5 py-3">
    <h2 class="min-w-0 truncate text-lg font-semibold text-primary" id="create-shortcut-heading">
      {$t('shortcut.title')}
    </h2>
    <CloseButton onClick={onClose} ariaLabel={$t('common.cancel')} />
  </header>

  <div class="space-y-4 px-5 py-4">
    <fieldset>
      <legend class="mb-2 text-sm font-medium text-secondary">{$t('shortcut.targetLegend')}</legend>
      <div class="space-y-0.5">
        {#each [{ id: 'instance' as const, label: $t('shortcut.targetInstance'), disabled: false }, { id: 'world' as const, label: $t('shortcut.targetWorld'), disabled: quickPlayDisabled }, { id: 'server' as const, label: $t('shortcut.targetServer'), disabled: quickPlayDisabled }] as opt (opt.id)}
          <label
            for={`shortcut-kind-${opt.id}`}
            class="flex items-center gap-3 rounded-md px-2 py-2 transition-colors"
            class:cursor-pointer={!opt.disabled}
            class:hover:bg-subtle={!opt.disabled}
            class:opacity-50={opt.disabled}
          >
            <input
              type="radio"
              id={`shortcut-kind-${opt.id}`}
              name="shortcut-kind"
              checked={kind === opt.id}
              disabled={opt.disabled}
              onchange={() => selectKind(opt.id)}
              data-testid={`shortcut-kind-${opt.id}`}
            />
            <span class="flex-1 text-sm text-primary">{opt.label}</span>
          </label>
        {/each}
      </div>
      {#if quickPlayDisabled}
        <p
          class="mt-1 flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs text-muted"
          data-testid="shortcut-no-quick-play"
        >
          <Icon name="info" size={14} class="mt-0.5 shrink-0" />
          <span>{$t('shortcut.noQuickPlay')}</span>
        </p>
      {/if}
    </fieldset>

    {#if kind === 'world'}
      {#if worlds && worlds.length > 0}
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-secondary">
            {$t('shortcut.worldLabel')}
          </span>
          <Select
            class="w-full text-sm"
            value={worldFolder}
            options={worlds.map((w) => ({ value: w, label: w }))}
            onChange={(v) => {
              worldFolder = String(v);
              void refreshName();
            }}
            ariaLabel={$t('shortcut.worldLabel')}
          />
        </label>
      {:else if worlds}
        <p class="text-sm text-muted" data-testid="shortcut-no-worlds">{$t('shortcut.noWorlds')}</p>
      {/if}
    {/if}

    {#if kind === 'server'}
      {#if servers && servers.length > 0}
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-secondary">
            {$t('shortcut.serverLabel')}
          </span>
          <Select
            class="w-full text-sm"
            value={serverAddress}
            options={servers.map((s) => ({
              value: s.address,
              label: s.name ? `${s.name} — ${s.address}` : s.address,
            }))}
            onChange={(v) => {
              serverAddress = String(v);
              void refreshName();
            }}
            ariaLabel={$t('shortcut.serverLabel')}
          />
        </label>
      {:else if servers}
        <p class="text-sm text-muted" data-testid="shortcut-no-servers">
          {$t('shortcut.noServers')}
        </p>
      {/if}
    {/if}

    <label class="block">
      <span class="mb-1 block text-sm font-medium text-secondary">{$t('shortcut.nameLabel')}</span>
      <input
        type="text"
        class="input mt-1 w-full"
        bind:value={name}
        oninput={() => (nameEdited = true)}
        data-testid="shortcut-name-input"
      />
    </label>

    {#if error}
      <p
        class="rounded-md bg-danger-bg px-3 py-2 text-sm text-danger"
        role="alert"
        data-testid="shortcut-error"
      >
        {error}
      </p>
    {/if}
  </div>

  <footer class="flex items-center justify-end gap-2 border-t border-border-subtle px-5 py-3">
    <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
      {$t('common.cancel')}
    </button>
    <BusyButton
      {busy}
      disabled={!canSubmit}
      class="btn-primary btn-sm inline-flex items-center gap-1.5"
      onclick={create}
      data-testid="shortcut-submit"
    >
      <Icon name="monitor" size={14} />
      {busy ? $t('shortcut.creating') : $t('shortcut.create')}
    </BusyButton>
  </footer>
</Modal>
