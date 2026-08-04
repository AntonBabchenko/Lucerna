<script lang="ts">
  // Receiving a friend's translations: pick a file → preview → import.
  //
  // The zip came from another person, so it is hostile input the whole way
  // through. `l10nInspectBundle` writes nothing — it exists purely so the user
  // sees what an import WOULD do before anything touches the global override
  // store; `l10nImportBundle` re-validates from scratch rather than trusting
  // the preview. The backend's split is mirrored in the copy: a whole-file
  // problem rejects the file and is explained here, while individual malformed
  // entries are only counted (`invalid`) and skipped.
  //
  // The conflict default is deliberately "keep mine": importing must never
  // silently overwrite the user's own translations. Taking the sender's is an
  // explicit choice.
  //
  // The file picker lives here rather than in the parent because the picked
  // path is this dialog's whole subject — cancelling the picker means there is
  // no dialog to show, so it closes itself.
  import { onMount } from 'svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import {
    commands,
    type BundleSummary,
    type ConflictPolicy,
    type ImportResult,
    type Error as IpcError,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';

  let {
    lang,
    onImported,
    onClose,
  }: {
    /** The language currently open in the editor. Only used to decide whether
        the bundle's language needs announcing — it never overrides it. */
    lang: string;
    /** Fired once entries landed. Carries the BUNDLE's language, which is the
        one that actually changed on disk. */
    onImported: (r: { lang: string }) => void;
    onClose: () => void;
  } = $props();

  type Phase = 'picking' | 'summary' | 'importing' | 'done' | 'failed';

  let phase = $state<Phase>('picking');
  let path = $state<string | null>(null);
  let summary = $state<BundleSummary | null>(null);
  let policy = $state<ConflictPolicy>('keep_mine');
  let result = $state<ImportResult | null>(null);
  // Why the file was rejected outright, as one or more finished sentences.
  let rejection = $state<string[]>([]);
  let importError = $state<string | null>(null);

  const titleId = `l10n-share-import-title-${crypto.randomUUID()}`;

  // What the merge could possibly write. Everything else in the file is
  // `invalid` — counted for honesty, but nothing to import, so there is no
  // action to offer.
  const importable = $derived(summary ? summary.newEntries + summary.conflicts : 0);

  /**
   * Turn a rejected file into copy a user can act on.
   *
   * Two cases earn their own wording because they have different remedies: a
   * plain resource pack should be installed as one, and a newer schema needs a
   * newer launcher. Everything else falls back to the shared error formatter.
   */
  function explain(e: IpcError): string[] {
    if (e.kind === 'l10n_share_bundle_invalid') {
      switch (e.error.kind) {
        case 'schema_too_new':
          return [$t('instance.l10n.share.schemaTooNew')];
        case 'no_metadata': {
          const lines = [$t('instance.l10n.share.notABundle')];
          // A `pack.mcmeta` with no Lucerna metadata: almost certainly a
          // regular resource pack someone handed over by mistake. Say where it
          // does belong rather than only what it is not.
          if (e.error.looks_like_resource_pack) {
            lines.push($t('instance.l10n.share.looksLikePack'));
          }
          return lines;
        }
      }
    }
    return [formatError(e)];
  }

  onMount(async () => {
    const picked = await openFile({
      multiple: false,
      filters: [{ name: $t('common.fileFilter.zip'), extensions: ['zip'] }],
    });
    // Cancelled: there is nothing for this dialog to be about.
    if (typeof picked !== 'string') {
      onClose();
      return;
    }
    path = picked;
    const res = await commands.l10nInspectBundle(picked);
    if (res.status === 'ok') {
      summary = res.data;
      phase = 'summary';
    } else {
      rejection = explain(res.error);
      phase = 'failed';
    }
  });

  async function run() {
    if (path === null || phase !== 'summary' || importable === 0) return;
    phase = 'importing';
    importError = null;
    const res = await commands.l10nImportBundle(path, policy);
    if (res.status === 'ok') {
      result = res.data;
      phase = 'done';
      // The bundle's language, not the prop: those entries landed in THAT
      // language, and the parent uses this to refresh the right view and to
      // offer applying it elsewhere.
      onImported({ lang: res.data.lang });
    } else {
      // Nothing was written, so the preview is still true — back to it, with
      // the reason, rather than dead-ending the dialog.
      importError = formatError(res.error);
      phase = 'summary';
    }
  }
</script>

<Modal
  ariaLabelledby={titleId}
  {onClose}
  closeOnBackdrop={phase !== 'importing'}
  closeOnEscape={phase !== 'importing'}
  panelClass="w-[480px] max-w-full p-5 flex flex-col gap-3"
  dataTestid="l10n-share-import-dialog"
>
  <DialogTitle id={titleId}>{$t('instance.l10n.share.importTitle')}</DialogTitle>

  {#if phase === 'picking'}
    <LoadingPanel label={$t('common.loading')} />
  {:else}
    <!--
      One body element for both outcomes: either the file describes itself, or
      it explains why it was refused. Never both — a rejected file has no
      summary to show.
    -->
    <div class="flex flex-col gap-1 text-sm" data-testid="share-import-summary">
      {#if rejection.length > 0}
        {#each rejection as line (line)}
          <p class="text-primary">{line}</p>
        {/each}
      {:else if summary}
        <!-- A stranger's free text. The backend truncates it and strips
             control characters; it is rendered as text, never as markup. -->
        {#if summary.note !== ''}
          <p class="text-secondary" data-testid="share-import-note">
            {$t('instance.l10n.share.fromNote', { note: summary.note })}
          </p>
        {/if}
        <p class="text-primary">
          {$t('instance.l10n.share.importSummary', {
            fresh: summary.newEntries,
            conflicts: summary.conflicts,
            invalid: summary.invalid,
          })}
        </p>
      {/if}
    </div>

    {#if summary && summary.lang !== lang}
      <!-- The entries land in the bundle's language, not the one open in the
           editor. Said before the import, not discovered after it. -->
      <p class="text-sm text-warning-text" data-testid="share-import-crosslang">
        {$t('instance.l10n.share.importedInto', { lang: summary.lang })}
      </p>
    {/if}

    {#if summary && phase !== 'done'}
      <fieldset class="flex flex-col gap-1 text-sm">
        <label class="flex items-center gap-2">
          <input
            type="radio"
            name="l10n-share-conflict"
            data-testid="share-import-keep"
            checked={policy === 'keep_mine'}
            disabled={phase === 'importing'}
            onchange={() => {
              policy = 'keep_mine';
            }}
          />
          {$t('instance.l10n.share.conflictKeepMine')}
        </label>
        <label class="flex items-center gap-2">
          <input
            type="radio"
            name="l10n-share-conflict"
            data-testid="share-import-take"
            checked={policy === 'take_file'}
            disabled={phase === 'importing'}
            onchange={() => {
              policy = 'take_file';
            }}
          />
          {$t('instance.l10n.share.conflictTakeFile')}
        </label>
      </fieldset>
    {/if}

    {#if phase === 'done' && result}
      <p class="text-sm font-medium text-primary" data-testid="share-import-result">
        {$t('instance.l10n.share.importResult', {
          added: result.added,
          replaced: result.replaced,
          kept: result.kept,
        })}
      </p>
    {/if}

    {#if importError}
      <p class="text-sm text-danger" data-testid="share-import-error">{importError}</p>
    {/if}
  {/if}

  <div class="mt-2 flex justify-end gap-2">
    {#if phase === 'done' || phase === 'failed'}
      <button type="button" class="btn-primary btn-sm" data-testid="share-import-close" onclick={onClose}>
        {$t('common.close')}
      </button>
    {:else}
      <button
        type="button"
        class="btn-ghost btn-sm"
        data-testid="share-import-cancel"
        disabled={phase === 'importing'}
        onclick={onClose}
      >
        {$t('common.cancel')}
      </button>
      <!--
        No Import button until a file has been picked and inspected: until then
        there is nothing to import, and offering the action early would let a
        caller (or a test) act on a dialog that has not read the file yet.
      -->
      {#if phase !== 'picking'}
        <BusyButton
          busy={phase === 'importing'}
          disabled={importable === 0}
          class="btn-primary btn-sm"
          data-testid="share-import-run"
          onclick={run}
        >
          {$t('instance.l10n.share.importAction')}
        </BusyButton>
      {/if}
    {/if}
  </div>
</Modal>
