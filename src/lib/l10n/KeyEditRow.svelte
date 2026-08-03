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
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import type { BadgeVariant } from '$lib/ui/cards/card-status';
  import { tooltip } from '$lib/ui/tooltip';
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
  // Save/clear outcome is otherwise purely visual (the state pill flips), so
  // a screen-reader user cannot tell a success from a backend rejection.
  //
  // Cleared on every keystroke, and that is what makes a REPEATED outcome
  // audible at all: a live region announces text CHANGES, so writing the same
  // "Saved" string a second time is a no-op and the region stays silent —
  // exactly the save-fix-save-again flow this exists for. Resetting in the
  // input event puts the blank in its own event turn, where it commits;
  // resetting at the top of save() would not, since the IPC await and Svelte's
  // flush are both microtasks and the empty state could never paint.
  let announcement = $state('');

  const dirty = $derived(draft !== displayValue(row));
  const fieldId = `l10n-key-${namespace}-${row.key}`;
  const errorId = `${fieldId}-error`;

  // Semantic variant, not a class recipe: the hand-rolled tones this replaced
  // had drifted off the design system (raw `bg-success/10` where the token is
  // `bg-success-bg`, `rounded-full` and `text-[11px]` where every other status
  // pill in the app is `rounded` / `text-xs`).
  const STATE_VARIANT: Record<KeyState, BadgeVariant> = {
    from_mod: 'neutral',
    ok: 'success',
    stale: 'warning',
    orphan: 'danger',
    missing: 'muted',
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
        // sent, so the resulting state is deterministic — with one exception.
        // `state_of` returns Orphan for a key that is gone from the mod's
        // English map BEFORE it ever compares source_en, so an orphan stays an
        // orphan no matter what the user writes into it. Claiming 'ok' here
        // would show a green "Translated" pill until the next refetch quietly
        // took it back.
        //
        // `origin: 'manual'` is not decoration: the patch spreads `...row`, so
        // a hand-edited machine string would otherwise keep its marker and
        // then be silently wiped by the next bulk revert. It also mirrors what
        // the backend just did — `NamespaceStore::set` hardcodes
        // `Origin::Manual`, so the row would be wrong until the next refetch.
        onSaved({
          ...row,
          overrideValue: draft,
          state: row.state === 'orphan' ? 'orphan' : 'ok',
          origin: 'manual',
        });
        announcement = $t('instance.l10n.keyTable.savedAnnouncement');
      } else {
        // Not mirrored into `announcement`: the error paragraph below is
        // role="alert", so assistive tech already announces this string
        // assertively. Feeding the polite region the same text would say it
        // twice — the redundancy DESIGN.md's Known gaps already warns about.
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
          // Restores the invariant the backend pins: no override means no
          // origin. Without this the spread would keep the old marker on a
          // row that no longer has anything to attribute.
          origin: null,
        };
        draft = displayValue(updated);
        onSaved(updated);
        announcement = $t('instance.l10n.keyTable.clearedAnnouncement');
      } else {
        // Not mirrored into `announcement`: the error paragraph below is
        // role="alert", so assistive tech already announces this string
        // assertively. Feeding the polite region the same text would say it
        // twice — the redundancy DESIGN.md's Known gaps already warns about.
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

<!--
  Named group, not a bare div: the rows are a long uniform list, so a screen
  reader entering one has to hear which key it is on before the field labels,
  which are identical on every row.
-->
<div
  class="flex flex-col gap-1.5 border-b border-border-subtle px-1 py-2.5 last:border-b-0"
  role="group"
  aria-label={row.key}
  data-testid="l10n-key-row"
  data-state={row.state}
>
  <div class="flex items-start justify-between gap-2">
    <!--
      Only the clipped case gets a tooltip: a key that fits already reads in
      full, and a hover card repeating it would be noise on every row.
    -->
    <code
      class="min-w-0 flex-1 truncate text-xs text-muted"
      use:tooltip={{ text: row.key, whenOverflowing: true }}>{row.key}</code
    >
    {#if row.origin === 'machine'}
      <!--
        Machine-written and untouched. The marker is what makes bulk revert
        honest — it is exactly the set that action would drop — and it clears
        the moment the user saves an edit over it (see `save`). Rendered as
        an INFO badge with a leading glyph so the who-wrote-this axis is not
        just a second identically-shaped pill next to the state.
      -->
      <StatusBadge
        variant="info"
        icon="aiTranslate"
        title={$t('instance.l10n.keyTable.machineBadgeTitle')}
        testid="l10n-key-origin-machine">{$t('instance.l10n.keyTable.machineBadge')}</StatusBadge
      >
    {/if}
    <StatusBadge variant={STATE_VARIANT[row.state]} testid="l10n-key-state"
      >{stateLabel}</StatusBadge
    >
  </div>
  <div class="flex flex-col gap-0.5">
    <span class="text-[11px] text-muted">{$t('instance.l10n.keyTable.sourceLabel')}</span>
    <p class="text-sm text-primary">{row.sourceEn}</p>
  </div>
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
        aria-invalid={error ? 'true' : undefined}
        aria-describedby={error ? errorId : undefined}
        bind:value={draft}
        oninput={() => (announcement = '')}
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
    <p id={errorId} role="alert" class="text-xs text-danger" data-testid="l10n-key-error">
      {error}
    </p>
  {/if}
  <!--
    Rendered unconditionally, empty: a live region only announces changes made
    while it is already in the accessibility tree, so one that appears together
    with its first message is silent.
  -->
  <span class="sr-only" aria-live="polite" data-testid="l10n-key-live">{announcement}</span>
</div>
