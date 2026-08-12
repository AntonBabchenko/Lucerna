<script lang="ts">
  // Export the user's translation overrides as one shareable file.
  //
  // The file is deliberately dual-use: a genuine resource pack a friend
  // WITHOUT Lucerna can drop into `resourcepacks/`, carrying Lucerna metadata
  // so another launcher can import the entries with full fidelity.
  //
  // That dual use has an honest limit this dialog states rather than hides:
  // as a plain resource pack the zip is built for ONE Minecraft version
  // (`pack.mcmeta`'s pack_format is version-specific and no value loads
  // everywhere), so on a different version the friend's game warns or refuses.
  // The Lucerna-to-Lucerna path is immune — the recipient's launcher rebuilds
  // for their own version. Hence the target version is shown plainly.
  //
  // The namespace list comes from the GLOBAL override store, not from the
  // instance: a user must be able to share a translation for a mod they no
  // longer have installed here. `instanceNamespaces` only decides what starts
  // ticked, so the common case (share what this instance uses) is one click
  // while the rest stays one tick away.
  import { onMount } from 'svelte';
  import { save } from '@tauri-apps/plugin-dialog';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import SelectAllCheckbox from '$lib/ui/SelectAllCheckbox.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

  let {
    lang,
    mcVersion,
    instanceNamespaces,
    onClose,
  }: {
    lang: string;
    /** Target Minecraft version the resource-pack half of the file is built
     *  for — stated to the user, not silently assumed. */
    mcVersion: string;
    /** Namespaces of the mods in the instance this was opened from. Seeds the
     *  initial tick only; the list itself is the whole store. */
    instanceNamespaces: string[];
    onClose: () => void;
  } = $props();

  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let namespaces = $state<string[]>([]);
  let checked = $state<Record<string, boolean>>({});
  let note = $state('');
  let busy = $state(false);

  let query = $state('');

  const titleId = `l10n-share-export-title-${crypto.randomUUID()}`;

  /** Below this many rows the list is scannable and a search box is just noise.
   *  Above it, one "Pre-fill all" run can put a hundred namespaces in the store
   *  and scrolling alone stops being a way to find anything. */
  const SEARCH_THRESHOLD = 10;
  const showSearch = $derived(namespaces.length > SEARCH_THRESHOLD);

  /** The search is a VIEW. `checked` stays keyed on the whole store and
   *  `selected` is derived from it, never from `visible` — otherwise filtering
   *  would silently drop the hidden half of the user's selection at export. */
  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q === '' ? namespaces : namespaces.filter((ns) => ns.toLowerCase().includes(q));
  });

  const selected = $derived(namespaces.filter((ns) => checked[ns]));
  // Select-all is scoped to the VISIBLE (filtered) rows, matching what
  // `setVisible` mutates — with a search active, «выбрать все» takes the
  // matches, not the whole list.
  const allVisibleSelected = $derived(visible.length > 0 && visible.every((ns) => checked[ns]));
  const someVisibleSelected = $derived(visible.some((ns) => checked[ns]));

  /** Bulk actions act on what the user can see. Ticking rows hidden behind a
   *  filter would be a change they cannot audit. */
  function setVisible(value: boolean) {
    const next = { ...checked };
    for (const ns of visible) next[ns] = value;
    checked = next;
  }

  onMount(async () => {
    const res = await commands.l10nOverriddenNamespaces(lang);
    if (res.status === 'ok') {
      namespaces = res.data;
      const seeded: Record<string, boolean> = {};
      for (const ns of res.data) seeded[ns] = instanceNamespaces.includes(ns);
      checked = seeded;
    } else {
      loadError = formatError(res.error);
    }
    loading = false;
  });

  function toggle(ns: string) {
    checked = { ...checked, [ns]: !checked[ns] };
  }

  async function runExport() {
    if (busy || selected.length === 0) return;
    // The save dialog belongs to the frontend (see ExportPackDialog): the
    // backend is handed a path it never had to choose.
    const dest = await save({
      defaultPath: `lucerna-translations-${lang}-${new Date().toISOString().slice(0, 10)}.zip`,
      filters: [{ name: $t('common.fileFilter.zip'), extensions: ['zip'] }],
    });
    // Cancelled — leave the dialog exactly as the user left it.
    if (!dest) return;

    busy = true;
    const res = await commands.l10nExport(mcVersion, lang, selected, note, dest);
    busy = false;
    if (res.status === 'ok') {
      pushSuccess($t('instance.l10n.share.exportDone'));
      onClose();
    } else {
      pushWarning($t('instance.l10n.share.exportTitle'), [formatError(res.error)]);
    }
  }
</script>

<Modal
  ariaLabelledby={titleId}
  {onClose}
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
  panelClass="w-[480px] max-w-full p-5 flex flex-col gap-3"
  dataTestid="l10n-share-export-dialog"
>
  <DialogTitle id={titleId}>{$t('instance.l10n.share.exportTitle')}</DialogTitle>

  {#if loading}
    <LoadingPanel label={$t('common.loading')} />
  {:else if loadError}
    <p class="text-sm text-danger" role="alert" data-testid="share-export-error">{loadError}</p>
  {:else if namespaces.length === 0}
    <div class="flex flex-col gap-1" data-testid="share-export-empty">
      <p class="text-sm text-primary">{$t('instance.l10n.share.emptyTitle')}</p>
      <p class="text-xs text-secondary">{$t('instance.l10n.share.emptyBody')}</p>
    </div>
  {:else}
    <div class="flex flex-col gap-0.5">
      <p class="text-sm text-primary" data-testid="share-export-heading">
        {$t('instance.l10n.share.listHeading')}
      </p>
      <p class="text-xs text-secondary" data-testid="share-export-tick-note">
        {$t('instance.l10n.share.listTickNote')}
      </p>
    </div>

    {#if showSearch}
      <input
        class="border rounded px-2 py-1 text-sm"
        placeholder={$t('instance.l10n.share.searchPlaceholder')}
        bind:value={query}
        data-testid="share-export-search"
      />
    {/if}

    <div class="flex items-center justify-between gap-2">
      <span
        class="text-xs text-secondary"
        data-testid="share-export-count"
        data-selected={selected.length}
        data-total={namespaces.length}
      >
        {$t('instance.l10n.share.selectedCount', {
          selected: selected.length,
          total: namespaces.length,
        })}
      </span>
      <SelectAllCheckbox
        allSelected={allVisibleSelected}
        indeterminate={someVisibleSelected && !allVisibleSelected}
        onToggle={(checkAll) => setVisible(checkAll)}
        testid="share-export-select-all"
      />
    </div>

    <div class="flex max-h-80 flex-col gap-1 overflow-y-auto">
      {#each visible as ns (ns)}
        <label class="flex items-center gap-2 text-sm text-primary">
          <input
            type="checkbox"
            checked={checked[ns]}
            onchange={() => toggle(ns)}
            data-testid="share-export-ns-{ns}"
          />
          {ns}
        </label>
      {/each}
    </div>

    <label class="flex flex-col gap-1 text-sm">
      <span class="text-secondary">{$t('instance.l10n.share.noteLabel')}</span>
      <input
        class="border rounded px-2 py-1"
        bind:value={note}
        maxlength="300"
        data-testid="share-export-note"
      />
    </label>

    <!-- Describes the file this dialog would produce, so it belongs with the
         list rather than with the empty state, where there is no file. -->
    <p class="text-xs text-muted" data-testid="share-export-hint">
      {$t('instance.l10n.share.exportHint', { version: mcVersion })}
    </p>
  {/if}

  <div class="mt-2 flex justify-end gap-2">
    <button type="button" class="btn-ghost btn-sm" onclick={onClose} disabled={busy}>
      {$t('common.cancel')}
    </button>
    <BusyButton
      {busy}
      disabled={selected.length === 0}
      class="btn-primary btn-sm"
      data-testid="share-export-run"
      onclick={() => void runExport()}
    >
      {$t('instance.l10n.share.exportAction')}
    </BusyButton>
  </div>
</Modal>
