<script lang="ts">
  import type { ModpackHit } from '$lib/ipc/bindings';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';

  // Import-by-link entry point. Resolving a link is READ-ONLY (one metadata GET
  // in Rust): on success this hands the resolved pack up to the existing
  // ModpackDetailModal → ImportPickerDialog flow, so a link can never install
  // anything on its own — the user still picks a version and confirms. Nothing
  // here downloads pack bytes.
  //
  // The grammar lives in Rust (`deeplink::parse_import_url`) and is NOT mirrored
  // here: one parser, one test suite. This dialog only shows what it reports.

  let {
    prefill = '',
    fromExternal = false,
    onCancel,
    onResolved,
  }: {
    prefill?: string;
    // True when another app handed us this link (a `lucerna://` deeplink) rather
    // than the user pasting it — the dialog says so, because the user should
    // always be able to tell that something external initiated this.
    fromExternal?: boolean;
    onCancel: () => void;
    onResolved: (hit: ModpackHit, versionId: string | null) => void;
  } = $props();

  // svelte-ignore state_referenced_locally — intentional one-time seed from the
  // prop; the parent remounts the dialog (keyed) when it opens with a new link.
  let url = $state(prefill);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const canSubmit = $derived(url.trim().length > 0 && !busy);

  async function resolve() {
    if (!canSubmit) return;
    busy = true;
    error = null;
    try {
      const r = await commands.modpackResolveUrl(url.trim());
      if (r.status !== 'ok') {
        error = formatError(r.error);
        return;
      }
      onResolved(r.data.hit, r.data.version_id);
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void resolve();
    }
  }
</script>

<Modal
  onClose={onCancel}
  ariaLabelledby="import-url-heading"
  dataTestid="import-from-url-dialog"
  panelClass="max-w-lg w-full flex flex-col"
>
  <header class="flex items-center justify-between border-b border-border-subtle px-5 py-3">
    <h2 class="min-w-0 truncate text-lg font-semibold text-primary" id="import-url-heading">
      {$t('modpacks.import.url.title')}
    </h2>
    <CloseButton onClick={onCancel} ariaLabel={$t('common.cancel')} />
  </header>

  <div class="space-y-3 px-5 py-4">
    {#if fromExternal}
      <p
        class="flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs text-secondary"
        data-testid="import-url-external-note"
      >
        <Icon name="info" size={14} class="mt-0.5 shrink-0" />
        <span>{$t('modpacks.import.url.external')}</span>
      </p>
    {/if}

    <label class="block">
      <span class="mb-1 block text-sm font-medium text-secondary">
        {$t('modpacks.import.url.inputLabel')}
      </span>
      <input
        type="text"
        class="input mt-1 w-full"
        placeholder={$t('modpacks.import.url.placeholder')}
        bind:value={url}
        onkeydown={onKeydown}
        data-testid="import-url-input"
      />
    </label>

    <p class="text-xs text-muted">{$t('modpacks.import.url.help')}</p>

    {#if error}
      <p
        class="rounded-md bg-danger-bg px-3 py-2 text-sm text-danger"
        role="alert"
        data-testid="import-url-error"
      >
        {error}
      </p>
    {/if}
  </div>

  <footer class="flex items-center justify-end gap-2 border-t border-border-subtle px-5 py-3">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
      {$t('common.cancel')}
    </button>
    <BusyButton
      {busy}
      disabled={!canSubmit}
      class="btn-primary btn-sm inline-flex items-center gap-1.5"
      onclick={resolve}
      data-testid="import-url-submit"
    >
      {busy ? $t('modpacks.import.url.resolving') : $t('modpacks.import.url.continue')}
    </BusyButton>
  </footer>
</Modal>
