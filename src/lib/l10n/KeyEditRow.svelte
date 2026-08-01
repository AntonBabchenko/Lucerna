<script lang="ts">
  // One editable row in the key table. Owns its own draft text + save/clear
  // IPC calls; on success it hands the parent a freshly-patched KeyRow rather
  // than asking it to refetch — see KeyTable's doc comment for why.
  //
  // Explicit Save (not blur, not debounce): a wrong translation is silently
  // invisible in game, l10nSetOverride can reject the value, and the modal
  // can close at any moment — an explicit, deliberate commit is the only
  // model where "saved" reliably means "the user meant this". Save is
  // disabled while the draft is empty so an accidental blank-out can never be
  // read as a clear; clearing an override is its own explicit action.
  //
  // CONTRACT: this component must be given a fresh instance for every
  // (namespace, lang, row.key) triple. `draft`/`error` below are seeded from
  // `row` once, at creation, and are never re-synced if `row` or `lang`
  // change under a live instance — a reused instance would show stale text
  // from whatever (namespace, lang) it was last created for, read as an
  // unsaved edit, and let Save write it under the wrong language. This is
  // deliberate, not an oversight: the safety lives one level up, in
  // KeyTable's each-block key (`${namespace}|${lang}|${row.key}`) — do not
  // reuse this component from a container that doesn't key the same way.
  import { commands, type KeyRow, type KeyState } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { displayValue } from './key-rows';

  let {
    row,
    namespace,
    lang,
    onSaved,
  }: {
    row: KeyRow;
    namespace: string;
    lang: string;
    onSaved: (updated: KeyRow) => void;
  } = $props();

  let draft = $state(displayValue(row));
  let saving = $state(false);
  let clearing = $state(false);
  let error = $state<string | null>(null);

  const dirty = $derived(draft !== displayValue(row));
  const fieldId = `l10n-key-${namespace}-${row.key}`;

  const STATE_TONE: Record<KeyState, string> = {
    from_mod: 'bg-subtle text-secondary',
    ok: 'bg-success/10 text-success',
    stale: 'bg-warning-bg text-warning-text',
    orphan: 'bg-danger/10 text-danger',
    missing: 'bg-subtle text-muted',
  };
  const stateLabel = $derived(
    {
      from_mod: $t('instance.l10n.keyTable.stateFromMod'),
      ok: $t('instance.l10n.keyTable.stateOk'),
      stale: $t('instance.l10n.keyTable.stateStale'),
      orphan: $t('instance.l10n.keyTable.stateOrphan'),
      missing: $t('instance.l10n.keyTable.stateMissing'),
    }[row.state],
  );

  async function save() {
    if (saving || clearing || draft.trim() === '' || !dirty) return;
    saving = true;
    error = null;
    try {
      const res = await commands.l10nSetOverride(namespace, lang, row.key, draft, row.sourceEn);
      if (res.status === 'ok') {
        // The backend validated `draft` against the CURRENT sourceEn we just
        // sent, so the resulting state is deterministic: an override that
        // matches the current English is 'ok', full stop — no re-fetch
        // needed to know that.
        onSaved({ ...row, overrideValue: draft, state: 'ok' });
      } else {
        error = formatError(res.error);
      }
    } finally {
      saving = false;
    }
  }

  async function clear() {
    if (saving || clearing || row.overrideValue === null) return;
    clearing = true;
    error = null;
    try {
      const res = await commands.l10nSetOverride(namespace, lang, row.key, '', row.sourceEn);
      if (res.status === 'ok') {
        const updated: KeyRow = {
          ...row,
          overrideValue: null,
          state: row.modValue !== null ? 'from_mod' : 'missing',
        };
        draft = displayValue(updated);
        onSaved(updated);
      } else {
        error = formatError(res.error);
      }
    } finally {
      clearing = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    void save();
  }
</script>

<div
  class="flex flex-col gap-1.5 border-b border-border-subtle px-1 py-2.5 last:border-b-0"
  data-testid="l10n-key-row"
  data-state={row.state}
>
  <div class="flex items-start justify-between gap-2">
    <code class="min-w-0 flex-1 truncate text-xs text-muted" title={row.key}>{row.key}</code>
    <span
      class="shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium {STATE_TONE[row.state]}"
      data-testid="l10n-key-state">{stateLabel}</span
    >
  </div>
  <p class="text-sm text-primary">{row.sourceEn}</p>
  {#if row.modValue !== null && row.state !== 'from_mod'}
    <p class="text-xs text-muted">
      {$t('instance.l10n.keyTable.modValueLabel')}: {row.modValue}
    </p>
  {/if}
  {#if row.state === 'orphan'}
    <p class="text-xs text-danger">{$t('instance.l10n.keyTable.orphanHint')}</p>
  {:else if row.state === 'stale'}
    <p class="text-xs text-warning-text">{$t('instance.l10n.keyTable.staleHint')}</p>
  {/if}
  <div class="flex items-end gap-2">
    <div class="flex min-w-0 flex-1 flex-col gap-0.5">
      <label class="text-[11px] text-muted" for={fieldId}>
        {$t('instance.l10n.keyTable.yourTranslationLabel')}
      </label>
      <input
        id={fieldId}
        type="text"
        class="h-8 w-full rounded border border-border-emphasis bg-surface px-2 text-sm text-primary"
        bind:value={draft}
        onkeydown={onKeydown}
        data-testid="l10n-key-input"
      />
    </div>
    <BusyButton
      busy={saving}
      disabled={clearing || draft.trim() === '' || !dirty}
      class="btn-primary btn-xs"
      data-testid="l10n-key-save"
      onclick={save}
    >
      {$t('instance.l10n.keyTable.saveLabel')}
    </BusyButton>
    {#if row.overrideValue !== null}
      <BusyButton
        busy={clearing}
        disabled={saving}
        class="btn-secondary btn-xs"
        data-testid="l10n-key-clear"
        onclick={clear}
      >
        {$t('instance.l10n.keyTable.clearLabel')}
      </BusyButton>
    {/if}
  </div>
  {#if error}
    <p class="text-xs text-danger" data-testid="l10n-key-error">{error}</p>
  {/if}
</div>
