<script lang="ts">
  // Full-window shell for the per-instance translation-coverage report: a
  // namespace list (least-translated first) on the left, a target-language
  // picker + Apply action in the header, and the key editor (KeyTable) on
  // the right once a namespace is picked.
  //
  // Own module rather than a section of ManageInstancesModal.svelte, which is
  // already at this project's 800-line file ceiling.
  import {
    commands,
    type ApplyGate,
    type InstanceCoverage,
    type NamespaceCoverage,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushInfo, pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import {
    NAMESPACE_SORTS,
    coverageTone,
    namespacePercent,
    sortNamespaces,
    type CoverageTone,
    type NamespaceSort,
  } from './coverage';
  import type { PrefillReadiness } from './prefill-readiness';
  import ApplyTargetsDialog from './ApplyTargetsDialog.svelte';
  import KeyTable from './KeyTable.svelte';
  import PrefillDialog from './PrefillDialog.svelte';
  import SearchResults from './SearchResults.svelte';
  import ShareExportDialog from './ShareExportDialog.svelte';
  import ShareImportDialog from './ShareImportDialog.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import { Icon } from '$lib/ui/icons';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { nextRovingIndex } from '$lib/ui/roving';
  import Select from '$lib/ui/Select.svelte';
  import { clampPanelWidth } from '$lib/ui/splitter';
  import SplitterHandle from '$lib/ui/SplitterHandle.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    open = $bindable(),
    instanceId,
    lang = $bindable(),
    aiReady = 'no_consent',
    mcVersion = '',
  }: {
    open: boolean;
    /** The instance to report on. Null while nothing is selected — the
     *  fetch effect below simply skips. */
    instanceId: string | null;
    /** The target language, owned by +page.svelte and shared with the
     *  Overview row's translation percentage — see l10nLang there. Changing
     *  it here (via the picker) propagates straight out through the binding,
     *  so the row refetches for the same language instead of the two
     *  surfaces silently measuring different things. */
    lang: string;
    /** Why the AI pre-fill is or is not usable. A PROP, not a read: this
     *  component's test suite pins exact `l10nCoverage` call counts, and
     *  +page.svelte already reads settings — adding a second reader here
     *  would only give the two of them a way to disagree. */
    aiReady?: PrefillReadiness;
    /** The Minecraft version of `instanceId`. Only the share export needs
     *  it — a shared file's resource-pack half is built for ONE version — and
     *  `InstanceCoverage` does not carry it, so the caller resolves it for
     *  the SAME instance it passes as `instanceId`. Optional with an empty
     *  default because the component tests that never open the export dialog
     *  have no version to give it. */
    mcVersion?: string;
  } = $props();

  // Draggable list/detail split. Not persisted — reopening starts from the
  // default, same as ManageInstancesModal and the skin editor.
  //
  // The floor is a constant (a namespace name has to stay readable); the
  // ceiling is DERIVED, so the list may grow on a wide window until the
  // detail pane hits its own minimum. Mirrors ManageInstancesModal.
  const LIST_MIN_WIDTH = 220;
  const LIST_FALLBACK_MAX = 420;
  // Enough for the search row plus one wrapped chip row.
  const DETAIL_MIN_WIDTH = 520;
  let listWidth = $state(280);
  let rowWidth = $state(0);
  const listMax = $derived(
    rowWidth > 0 ? Math.max(LIST_MIN_WIDTH, rowWidth - DETAIL_MIN_WIDTH) : LIST_FALLBACK_MAX,
  );

  // Owned here rather than by SplitterHandle: the shared handle takes bounds
  // as props and stays observer-free, which keeps it safe to render in
  // component tests. Mirrors ManageInstancesModal's observeRow.
  function observeRow(node: HTMLElement) {
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      rowWidth = node.clientWidth;
      // Re-clamp when the window shrinks the ceiling below the current width.
      listWidth = clampPanelWidth(listWidth, LIST_MIN_WIDTH, listMax);
    });
    ro.observe(node);
    return {
      destroy() {
        ro.disconnect();
      },
    };
  }

  let coverage = $state<InstanceCoverage | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let selectedNamespace = $state<string | null>(null);
  let applying = $state(false);

  // The sidebar order is decided once per load, not re-derived on every
  // silent refresh: sortNamespaces puts the least-translated first, so saving
  // the first key of a 0% namespace lifts it off the floor and sinks it past
  // every namespace still at 0% — in a 300-mod pack, clean out of the
  // viewport, mid-edit. Percentages still update live; only the ORDER is
  // pinned.
  let nsOrder = $state<string[]>([]);

  // Monotonic request id: a response only applies if it's still the most
  // recent request in flight, so a late response for an instance or language
  // the user has since navigated away from can't clobber newer state. Shared
  // with the silent coverage refresh below so the two compete on the same
  // "most recent wins" ordering instead of being able to race each other.
  let requestId = 0;

  // A namespace list is per-instance — a namespace name that happens to
  // match between two instances would otherwise show that instance's key
  // table under a stale selection from before the switch.
  $effect(() => {
    void instanceId;
    selectedNamespace = null;
    // The roving index is an offset into a list that has just been replaced
    // wholesale, so it now points at an unrelated namespace. The clamp effect
    // below can't catch this — it only fires when the new list is SHORTER.
    focusIndex = 0;
  });

  async function load(id: string, requestedLang: string) {
    const myRequest = ++requestId;
    loading = true;
    loadError = null;
    const res = await commands.l10nCoverage(id, requestedLang);
    if (myRequest !== requestId) return; // superseded by a newer request
    loading = false;
    if (res.status === 'ok') {
      coverage = res.data;
      // The user's choice survives a reload. It used to be rebuilt with the
      // default order while the control beside it still displayed "by name",
      // so switching language silently reverted the list and lied about it.
      //
      // UNTESTED, honestly: the only trigger is `load()`, i.e. a language or
      // instance change, and the test written for it drove Apply instead —
      // which calls the SILENT refresh and never rebuilds this order, so it
      // passed with the fix removed. Rather than keep a fourth vacuous test
      // this session, the gap is recorded here.
      nsOrder = sortNamespaces(res.data.namespaces, nsSort).map((r) => r.namespace);
      // The backend may resolve a bare launcher locale (e.g. "ru", from the
      // page's initial guess) to a full Minecraft code ("ru_ru"). Write the
      // resolved code back through the bindable prop so this picker and the
      // Overview row converge on the same value instead of one of them
      // being stuck on the bare guess forever. A no-op once `lang` is
      // already a full code, since the backend just echoes it back.
      if (res.data.lang !== requestedLang) lang = res.data.lang;
    } else {
      coverage = null;
      loadError = $t('instance.l10n.loadFailed', { error: formatError(res.error) });
    }
  }

  $effect(() => {
    if (!open) return;
    const id = instanceId;
    const targetLang = lang;
    if (!id) {
      coverage = null;
      loadError = null;
      loading = false;
      return;
    }
    void load(id, targetLang);
  });

  // Best-effort background refresh of the coverage percentages after a save
  // in the key table: KeyTable patches its own row locally rather than
  // re-fetching (see its doc comment), so the namespace list's percentages
  // and the header total would otherwise drift stale for the rest of the
  // session. l10nCoverage is cheap to re-run (jar results are cached by
  // SHA-1), so this doesn't need a loading state of its own — a failure here
  // just leaves the percentages stale for a moment rather than surfacing an
  // error banner over a background sync the user didn't ask for.
  // A finished pre-fill run rewrites the rows the user is currently looking
  // at, but KeyTable's fetch is keyed on (instance, namespace, lang) — none of
  // which a run changes. Bumping this makes it refetch; without it the new
  // translations sit on disk, invisible until the modal is reopened.
  let keyReloadToken = $state(0);

  let shareExportOpen = $state(false);
  let shareImportOpen = $state(false);
  // The offer dialog is opened proactively — after an apply, an import, or an
  // AI pre-fill run, and from its own button beside Apply.
  //
  // The two are not the same event. Import and pre-fill change the GLOBAL
  // store, which silently puts every other instance out of date — that is when
  // an unprompted offer earns its interruption, and there it still self-closes
  // when there is nothing to do. Apply acts on THIS instance on purpose and
  // nothing elsewhere moved under the user, so it no longer opens anything;
  // the button does. A dialog the user opened deliberately owes them an
  // answer even when the answer is "nobody needs this", hence `unsolicited`.
  let offerOpen = $state(false);
  let offerLang = $state('');
  let offerExclude = $state<string | null>(null);

  /** The instance-wide search box. Non-empty swaps the right pane into results
   *  mode; clearing it returns to browsing. Not a separate screen, because the
   *  user is mid-task and a screen change would cost them their place. */
  let findQuery = $state('');
  /** Debounced copy. `l10n_search` walks every enabled jar, so re-running it on
   *  each keystroke would make a 300-mod pack unusable.
   *
   *  DELIBERATELY UNTESTED, and not for want of trying: three attempts (a call
   *  count, fake timers, real timers) all passed with the debounce removed,
   *  because Svelte coalesces the prop changes into a single effect run before
   *  any difference can show. A test that cannot fail is not evidence, so
   *  rather than keep a green one that proves nothing, the gap is recorded
   *  here. The guard matters in production, where real typing arrives with
   *  real gaps that batching does not absorb. */
  let findSettled = $state('');
  let findTimer: ReturnType<typeof setTimeout> | null = null;

  const FIND_DEBOUNCE_MS = 250;

  $effect(() => {
    const q = findQuery;
    if (findTimer) clearTimeout(findTimer);
    findTimer = setTimeout(() => {
      findSettled = q;
    }, FIND_DEBOUNCE_MS);
    return () => {
      if (findTimer) clearTimeout(findTimer);
    };
  });

  const finding = $derived(findQuery.trim() !== '');

  let offerUnsolicited = $state(true);

  function openOffer(targetLang: string, exclude: string | null, unsolicited = true) {
    offerLang = targetLang;
    offerExclude = exclude;
    offerUnsolicited = unsolicited;
    offerOpen = true;
  }

  function handleImported(r: { lang: string }) {
    // Same-language import: the rows and percentages on screen were read
    // before the merge and are now stale on disk — reuse the exact refresh the
    // pre-fill run uses. A different language changes nothing that is
    // currently displayed.
    if (r.lang === lang) {
      void refreshCoverageSilently();
      keyReloadToken += 1;
    }
    // The offer must ask about the language that was actually imported, not
    // the one open in the editor: applying the editor's language would ship a
    // pack containing none of what just arrived. The current instance is NOT
    // excluded — its own pack is stale too after an import.
    openOffer(r.lang, null);
  }

  function afterPrefillRun() {
    void refreshCoverageSilently();
    keyReloadToken += 1;
    // A run is the largest single change this feature ever makes to the
    // global store, and it ends by rebuilding THIS instance's pack — which
    // leaves every other instance exactly as stale as a manual apply does.
    openOffer(lang, instanceId);
  }

  async function refreshCoverageSilently() {
    if (!instanceId) return;
    const id = instanceId;
    const targetLang = lang;
    const myRequest = ++requestId;
    const res = await commands.l10nCoverage(id, targetLang);
    if (myRequest !== requestId) return;
    if (res.status === 'ok') coverage = res.data;
  }

  // l10nApply builds the pack from every namespace's saved overrides at
  // once — there is no per-namespace variant of it — so Apply lives once for
  // the whole modal rather than beside each namespace, where it would
  // misleadingly suggest applying just that namespace's changes.
  async function apply() {
    if (!instanceId || !coverage || coverage.applyGate !== 'ready' || applying) return;
    applying = true;
    try {
      const res = await commands.l10nApply(instanceId, lang);
      if (res.status === 'ok') {
        if (res.data) {
          // The pack is live, but a game that is ALREADY running will not show
          // it until its resources are reloaded — and the thing a user
          // naturally tries, switching the game's language, is exactly what
          // does not work: Minecraft re-reads the language files without
          // re-resolving the active pack set. Confirmed in a live game
          // 2026-08-05, where it read as "the launcher applied nothing".
          pushSuccess($t('instance.l10n.apply.toastAppliedTitle'), [
            $t('instance.l10n.apply.toastAppliedReloadLine'),
          ]);
        } else {
          // `false` is not a failure — the pack is written and registered,
          // it just can't flip on in options.txt yet because the instance
          // has never been launched. Say so plainly instead of either
          // claiming success or raising an error for something that worked.
          pushInfo($t('instance.l10n.apply.toastDeferredTitle'), [
            $t('instance.l10n.apply.toastDeferredLine'),
          ]);
        }
        // Apply is the one action that changes pack state, and it used to be
        // the one action that left the screen showing the old state: the
        // "switched off in this instance" banner stayed up after the Apply
        // that turned it back on, and the percentages never moved. Every
        // other write path already refreshes; this one simply did not.
        void refreshCoverageSilently();
      } else {
        pushWarning($t('instance.l10n.apply.toastFailedTitle'), [formatError(res.error)]);
      }
    } finally {
      applying = false;
    }
  }

  // Keyed over the whole union rather than an if-chain: a fourth gate variant
  // has to be a compile error here, not a silently empty reason. The pre-fill
  // buttons below now read their enablement off this string, so an unhandled
  // gate would quietly re-enable a paid run whose output the game cannot load.
  const applyReasons: Record<ApplyGate, string> = $derived({
    ready: '',
    unknown_format: $t('instance.l10n.apply.reasonUnknownFormat'),
    too_old: $t('instance.l10n.apply.reasonTooOld'),
  });
  const applyReason = $derived(coverage ? applyReasons[coverage.applyGate] : '');

  // The pre-fill needs consent to reach a provider, a credential to
  // authenticate with, and an instance whose Minecraft version could actually
  // load the resulting pack. Missing any of them, the triggers stay on screen
  // and go disabled WITH the reason — hiding them teaches the user the feature
  // does not exist, which is the one thing that is never true. The version
  // gate is the least fixable of the three and is still said out loud rather
  // than implied by absence.
  //
  // An empty string means "nothing missing"; `canPrefill` reads off it so the
  // enablement and the explanation can never disagree.
  const prefillDisabledReason = $derived.by(() => {
    if (aiReady === 'no_consent') return $t('instance.l10n.prefill.disabledNoConsent');
    if (aiReady === 'no_key') return $t('instance.l10n.prefill.disabledNoKey');
    // Before coverage lands there is no applyGate to judge, and the header
    // renders regardless — so this is a real state, not a transient nobody
    // sees on a large pack.
    if (!coverage) return $t('instance.l10n.loading');
    // Borrowed from Apply rather than reworded: it is the same fact about the
    // same instance, and two vocabularies for it would read as two problems.
    if (coverage.applyGate !== 'ready') return applyReason;
    return '';
  });
  const canPrefill = $derived(prefillDisabledReason === '');

  // `null` = the dialog is closed. `{ namespace: null }` = whole instance.
  let prefillScope = $state<{ namespace: string | null } | null>(null);

  // Rows in the pinned order, with any namespace that appeared after the last
  // full load appended (sorted) rather than dropped.
  /** Below this many mods the list is scannable and a filter box is only
   *  clutter; a 400-mod pack is a different animal. Mirrors the threshold the
   *  share-export dialog uses for the same reason. */
  const NS_FILTER_THRESHOLD = 12;

  let nsFilter = $state('');
  let nsSort = $state<NamespaceSort>('leastCovered');

  const showNsFilter = $derived((coverage?.namespaces.length ?? 0) > NS_FILTER_THRESHOLD);

  /** Re-pin the order — and ONLY here. `nsOrder` exists because re-deriving it
   *  on every refresh made a row sink out of the viewport mid-edit the moment
   *  its first key was saved. Picking a sort is the one moment the user has
   *  asked for the list to move. */
  function pickSort(order: NamespaceSort) {
    nsSort = order;
    nsOrder = sortNamespaces(coverage?.namespaces ?? [], order).map((r) => r.namespace);
  }

  const sortedNamespaces = $derived.by(() => {
    if (!coverage) return [];
    const byName = new Map(coverage.namespaces.map((r) => [r.namespace, r]));
    const pinned = nsOrder
      .map((name) => byName.get(name))
      .filter((r): r is NamespaceCoverage => r !== undefined);
    const seen = new Set(nsOrder);
    const fresh = sortNamespaces(
      coverage.namespaces.filter((r) => !seen.has(r.namespace)),
      nsSort,
    );
    const all = [...pinned, ...fresh];
    const q = nsFilter.trim().toLowerCase();
    if (q === '') return all;
    // The open namespace never disappears under the filter: the key table on
    // the right would then be showing a mod that is not on the left, which
    // reads as a bug rather than as a filter.
    return all.filter(
      (r) => r.namespace.toLowerCase().includes(q) || r.namespace === selectedNamespace,
    );
  });
  // Totals come from the same rows the sidebar renders, so the header can
  // never disagree with the list under it. `percent` stays the backend's own
  // key-weighted figure rather than a second, differently-rounded derivation.
  const totals = $derived.by(() => {
    const rows = coverage?.namespaces ?? [];
    return rows.reduce(
      (acc, r) => ({
        total: acc.total + r.totalKeys,
        covered: acc.covered + r.fromMod + r.overridden,
      }),
      { total: 0, covered: 0 },
    );
  });

  const languageOptions = $derived(
    (coverage?.availableCodes ?? []).map((code) => ({ value: code, label: code })),
  );

  // Roving focus over the namespace list: one tab stop for a list that can
  // hold 300 rows. Arrow keys move FOCUS only — activation stays on
  // Enter/Space/click (the rows are real <button>s, so that is free). Moving
  // the selection on focus would fire a key-table fetch per arrow press.
  let focusIndex = $state(0);
  let rowEls = $state<(HTMLButtonElement | null)[]>([]);

  function onListKeydown(e: KeyboardEvent) {
    const next = nextRovingIndex(e.key, focusIndex, sortedNamespaces.length, 'vertical');
    if (next === null) return;
    e.preventDefault();
    focusIndex = next;
    rowEls[next]?.focus();
  }

  // A shorter list (filtered instance, language switch) must not leave the
  // roving index pointing past the end.
  $effect(() => {
    if (focusIndex > sortedNamespaces.length - 1)
      focusIndex = Math.max(0, sortedNamespaces.length - 1);
  });

  // The selection cue is a background tint on one row; if that row is out of
  // the scroll viewport the modal shows nothing about what the right pane is
  // displaying. Scroll it back into view whenever the selection changes.
  $effect(() => {
    const name = selectedNamespace;
    if (!name) return;
    const idx = sortedNamespaces.findIndex((r) => r.namespace === name);
    // Optional call, not just optional chain: happy-dom has no layout, so the
    // method is absent there entirely (same guard as Select.svelte).
    rowEls[idx]?.scrollIntoView?.({ block: 'nearest' });
  });

  // 'none' is muted, not danger: coverage.ts documents zero as "nothing is
  // wrong, it is just untranslated", the detail pane already tones the same
  // concept muted (KeyTable's `missing` chip, KeyEditRow's `missing` pill),
  // and danger has to stay legible for orphans and load failures.
  function toneClass(tone: CoverageTone): string {
    if (tone === 'ok') return 'text-success';
    if (tone === 'partial') return 'text-warning-text';
    return 'text-muted';
  }

  function close() {
    open = false;
  }
</script>

{#if open}
  <Modal
    ariaLabelledby="l10n-modal-title"
    onClose={close}
    panelClass="w-full h-full overflow-hidden flex flex-col"
    dataTestid="l10n-modal"
  >
    <header class="flex items-center justify-between gap-3 border-b px-4 py-2">
      <!--
        Title and summary share one flex child so the action group stays the
        header's second item and keeps its right edge — a bare third sibling
        would push the actions off `justify-between`'s far end.
      -->
      <div class="flex min-w-0 items-baseline gap-3">
        <DialogTitle id="l10n-modal-title">{$t('instance.l10n.title')}</DialogTitle>
        <!--
          Marked Beta while the feature settles, the same stance the own-server
          screen takes. Beside the title rather than inside it so screen readers
          announce the heading without the qualifier swallowed into it.
        -->
        <span
          class="rounded bg-subtle px-1.5 py-0.5 text-xs text-secondary"
          data-testid="l10n-beta-badge"
        >
          {$t('instance.l10n.beta')}
        </span>
        {#if coverage}
          <span class="shrink-0 text-xs text-muted" data-testid="l10n-summary">
            {$t('instance.l10n.summary', {
              total: totals.total,
              covered: totals.covered,
              percent: coverage.percent,
            })}
          </span>
        {/if}
      </div>
      <div class="flex items-center gap-2">
        {#if languageOptions.length > 0}
          <Select
            value={lang}
            options={languageOptions}
            onChange={(v) => (lang = String(v))}
            ariaLabel={$t('instance.l10n.languageLabel')}
            dataTestid="l10n-language-select"
          />
        {/if}
        {#if prefillDisabledReason}
          <!--
            Inline, not only in the tooltip — the same remedy the Apply gate
            below already carries, for the same reason: the wrapper span never
            matches :focus-visible and `describe: false` suppresses
            aria-describedby, so a keyboard or screen-reader user would never
            reach the one copy of the text telling them what to fix. This is
            the header's single copy; the per-namespace icons stay
            tooltip-only, because one line per mod row is noise, not help.
          -->
          <span class="text-xs text-warning-text" data-testid="l10n-prefill-reason">
            {prefillDisabledReason}
          </span>
        {/if}
        <span
          class="inline-flex"
          use:tooltip={prefillDisabledReason
            ? { text: prefillDisabledReason, describe: false }
            : null}
        >
          <button
            type="button"
            class="btn-secondary btn-sm"
            disabled={!canPrefill}
            data-testid="l10n-prefill-all"
            onclick={() => (prefillScope = { namespace: null })}
          >
            {$t('instance.l10n.prefill.allButton')}
          </button>
        </span>
        <!--
          Both act on the GLOBAL override store, not on this instance's
          coverage, so neither waits for the coverage load: a user must be
          able to send or receive translations for mods this instance does not
          have, and an import is exactly what an empty instance needs.
        -->
        <button
          type="button"
          class="btn-secondary btn-sm"
          data-testid="l10n-share-export"
          onclick={() => (shareExportOpen = true)}
        >
          {$t('instance.l10n.share.exportBtn')}
        </button>
        <button
          type="button"
          class="btn-secondary btn-sm"
          data-testid="l10n-share-import"
          onclick={() => (shareImportOpen = true)}
        >
          {$t('instance.l10n.share.importBtn')}
        </button>
        {#if applyReason}
          <!--
            Inline, not only in the tooltip: the wrapper-span pattern below is
            hover-only by construction (the span never matches :focus-visible)
            and `describe: false` suppresses aria-describedby, so a keyboard or
            screen-reader user would never reach the one copy of the
            remediation text.
          -->
          <span class="text-xs text-warning-text" data-testid="l10n-apply-reason">
            {applyReason}
          </span>
        {/if}
        {#if coverage}
          <span
            class="inline-flex"
            use:tooltip={applyReason ? { text: applyReason, describe: false } : null}
          >
            <BusyButton
              busy={applying}
              disabled={coverage.applyGate !== 'ready'}
              class="btn-secondary btn-sm"
              data-testid="l10n-apply"
              onclick={apply}
            >
              {$t('instance.l10n.apply.button')}
            </BusyButton>
          </span>
          <!--
            Carrying the same translations to other instances is now something
            the user asks for, not something Apply does to them on the way past.
          -->
          <button
            type="button"
            class="btn-ghost btn-sm"
            data-testid="l10n-apply-elsewhere"
            onclick={() => instanceId && openOffer(lang, instanceId, false)}
          >
            {$t('instance.l10n.targets.openButton')}
          </button>
        {/if}
        <CloseButton onClick={close} ariaLabel={$t('instance.l10n.closeLabel')} />
      </div>
    </header>
    {#if coverage?.packState === 'present_not_enabled'}
      <!--
        A modpack update's own overrides/options.txt overwrote the instance's
        wholesale, wiping the resourcePacks entry while leaving the generated
        pack file on disk (see l10n::options_txt's module doc). Re-running
        Apply rebuilds, re-registers AND re-enables in one call — the same
        action the header's Apply button performs — so this reuses it rather
        than a distinct command.

        The copy states what was OBSERVED, not what caused it. The backend
        reports a state, never a reason, and this banner used to name a modpack
        update as the culprit — which read as a flat lie to a maintainer who
        had merely run an AI pre-fill. The button's label likewise has to admit
        that Apply rebuilds rather than just flipping a switch.
      -->
      <!--
        `status`, not `alert`: it appears after the coverage load lands and
        describes a condition the user did not just cause, so it should be read
        at the next opportunity rather than interrupt.
      -->
      <div
        role="status"
        class="flex items-center justify-between gap-3 border-b bg-warning-bg px-4 py-2 text-sm text-warning-text"
        data-testid="l10n-pack-disabled-banner"
      >
        <span>{$t('instance.l10n.packDisabled.message')}</span>
        <BusyButton
          busy={applying}
          disabled={coverage.applyGate !== 'ready'}
          class="btn-secondary btn-sm shrink-0"
          data-testid="l10n-pack-reenable"
          onclick={apply}
        >
          {$t('instance.l10n.packDisabled.reenableButton')}
        </BusyButton>
      </div>
    {:else if coverage?.packState === 'present_awaiting_launch'}
      <!--
        There is no options.txt at all, because the instance has never been
        launched. Deliberately BUTTONLESS: `options_txt::update_atomically`
        returns Ok(false) on a missing file and never creates one, so Apply
        cannot change this state however many times it is pressed. Offering it
        here was offering a button with no reachable success.
      -->
      <div
        role="status"
        class="flex items-center gap-3 border-b bg-subtle px-4 py-2 text-sm text-secondary"
        data-testid="l10n-pack-awaiting-launch-banner"
      >
        <span>{$t('instance.l10n.packDisabled.awaitingLaunch')}</span>
      </div>
    {/if}
    <!--
      The instance-wide search sits full width under the header rather than as
      another control in the crowded toolbar: it is the entry point for the
      case the per-mod editor cannot serve at all — the player has the TEXT and
      not the mod — so it must not compete for space with the per-mod search,
      which answers a different question.
    -->
    <div class="flex items-center gap-2 border-b px-4 py-2">
      <input
        class="min-w-0 flex-1 border-0 bg-transparent text-sm outline-none"
        placeholder={$t('instance.l10n.find.placeholder')}
        bind:value={findQuery}
        data-testid="l10n-find-input"
      />
      {#if finding}
        <button
          type="button"
          class="btn-ghost btn-xs shrink-0"
          aria-label={$t('instance.l10n.find.clear')}
          data-testid="l10n-find-clear"
          onclick={() => {
            // Clear the settled copy TOO. Otherwise the debounce still holds
            // the abandoned query, and typing again within the window remounts
            // the results and issues one more full-instance scan for text the
            // user has already thrown away.
            findQuery = '';
            findSettled = '';
          }}
        >
          <Icon name="close" />
        </button>
      {/if}
    </div>
    {#if finding && instanceId}
      <div class="flex-1 overflow-y-auto" data-testid="l10n-find-results">
        {#if findSettled.trim() !== findQuery.trim()}
          <!-- Between the keystroke and the debounce the old pane is gone and
               the new one has nothing yet; without this the modal body is an
               empty rectangle for as long as the user keeps typing. -->
          <LoadingPanel label={$t('common.loading')} />
        {:else}
          <SearchResults
            {instanceId}
            {lang}
            query={findSettled}
            onSaved={refreshCoverageSilently}
          />
        {/if}
      </div>
    {:else}
      <div class="flex flex-1 overflow-hidden" use:observeRow>
        <aside
          class="shrink-0 overflow-y-auto p-2"
          style="width:{listWidth}px"
          aria-label={$t('instance.l10n.listRegionLabel')}
        >
          {#if loading}
            <LoadingPanel label={$t('instance.l10n.loading')} />
          {:else if loadError}
            <p role="alert" class="p-3 text-sm text-danger" data-testid="l10n-error">{loadError}</p>
          {:else if (coverage?.namespaces.length ?? 0) === 0}
            <!--
              Keyed on the UNFILTERED list. Keying it on the filtered one meant
              that a filter matching nothing replaced this whole branch — the
              filter input included — so the user was told the instance had no
              translatable text and was left with no way to clear the filter
              that caused it. A dead end.
            -->
            <p class="p-3 text-sm text-muted" data-testid="l10n-empty">
              {$t('instance.l10n.empty')}
            </p>
          {:else}
            {#if showNsFilter}
              <input
                class="mb-1 w-full rounded border px-2 py-1 text-xs"
                placeholder={$t('instance.l10n.nsFilter.placeholder')}
                bind:value={nsFilter}
                data-testid="l10n-ns-filter"
              />
            {/if}
            <div class="mb-1 flex items-center gap-1">
              <span class="flex-1 truncate text-xs text-muted" data-testid="l10n-ns-sort-current">
                {$t(`instance.l10n.nsSort.${nsSort}`)}
              </span>
              <Select
                value={nsSort}
                options={NAMESPACE_SORTS.map((order) => ({
                  value: order,
                  label: $t(`instance.l10n.nsSort.${order}`),
                }))}
                onChange={(v) => pickSort(v as NamespaceSort)}
                ariaLabel={$t('instance.l10n.nsSort.label')}
                dataTestid="l10n-ns-sort"
                class="shrink-0"
              />
            </div>
            {#if sortedNamespaces.length === 0}
              <p class="p-3 text-sm text-muted" data-testid="l10n-ns-filter-empty">
                {$t('instance.l10n.nsFilter.noMatch')}
              </p>
            {/if}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <ul class="flex flex-col gap-1" onkeydown={onListKeydown}>
              {#each sortedNamespaces as row, i (row.namespace)}
                {@const percent = namespacePercent(row)}
                {@const selected = selectedNamespace === row.namespace}
                {@const nsLabel = $t('instance.l10n.prefill.namespaceButtonAria', {
                  namespace: row.namespace,
                })}
                <!--
                A flex row, not a button containing a button: the selector IS
                a <button>, and nesting the per-namespace translate action
                inside it would be invalid HTML and a Svelte a11y error. The
                action is a SIBLING, and deliberately does not carry
                data-testid="l10n-namespace-row" — the modal's suite counts
                elements with that testid.
              -->
                <li class="flex items-center gap-1">
                  <!--
                  No aria-label: the namespace name and its percentage below
                  ARE the accessible content a screen-reader user needs — an
                  aria-label would replace both with nothing useful. See
                  OverviewTab.svelte's body-zone rule.
                -->
                  <button
                    bind:this={rowEls[i]}
                    type="button"
                    class="flex min-w-0 flex-1 items-center justify-between gap-2 rounded px-2 py-1.5 text-left text-sm transition-colors hover:bg-subtle"
                    class:bg-accent-soft={selected}
                    tabindex={i === focusIndex ? 0 : -1}
                    aria-current={selected ? 'true' : undefined}
                    data-testid="l10n-namespace-row"
                    onclick={() => {
                      // The tab stop follows activation, as in TabBar /
                      // SegmentedControl / ToggleChipGroup: a click focuses the
                      // row whatever its tabindex says, so leaving the index
                      // behind would make the next arrow press jump away from
                      // the row the user is on, and would drop the list's single
                      // tab stop back on row 1 when focus re-enters the list.
                      focusIndex = i;
                      selectedNamespace = row.namespace;
                    }}
                  >
                    <span class="truncate">{row.namespace}</span>
                    <span class="flex shrink-0 items-center gap-2">
                      <span class="font-mono {toneClass(coverageTone(percent))}">
                        {$t('instance.l10n.percentValue', { percent })}
                      </span>
                      <span class="text-xs text-muted">
                        {$t('instance.l10n.namespaceCount', {
                          covered: row.fromMod + row.overridden,
                          total: row.totalKeys,
                        })}
                      </span>
                    </span>
                  </button>
                  <!--
                    Icon-only action ⇒ .btn-icon family carrying use:tooltip AND
                    aria-label from the same key (DESIGN.md §5). The tooltip is
                    routed to whichever element can actually fire it: enabled,
                    it stays on the BUTTON, which also gives keyboard users the
                    hint (the wrapper's :focus-visible check can never match a
                    non-focusable span); disabled, it moves to the SPAN, because
                    a disabled button fires no pointer events at all — and a
                    disabled button is not focusable either, so nothing is lost.
                    Exactly one of the two carries text at any time.
                    tabindex follows the roving row so an enabled AI action does
                    not double the list's tab stops: Tab from the focused row
                    reaches its own AI button and then leaves the list.
                  -->
                  <span
                    class="inline-flex shrink-0"
                    use:tooltip={canPrefill
                      ? null
                      : { text: prefillDisabledReason, describe: false }}
                  >
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      disabled={!canPrefill}
                      tabindex={i === focusIndex ? 0 : -1}
                      aria-label={nsLabel}
                      data-testid="l10n-prefill-namespace"
                      onclick={() => {
                        // Claim the tab stop, same rule as the row button beside
                        // it: a click focuses this button whatever its tabindex
                        // said, and PrefillDialog's focus trap restores focus
                        // here on close — so leaving the index behind would send
                        // the next arrow press to a row the user never left.
                        focusIndex = i;
                        prefillScope = { namespace: row.namespace };
                      }}
                      use:tooltip={canPrefill ? nsLabel : null}
                    >
                      <Icon name="aiTranslate" size={14} />
                    </button>
                  </span>
                </li>
              {/each}
            </ul>
          {/if}
        </aside>
        <SplitterHandle
          bind:width={listWidth}
          min={LIST_MIN_WIDTH}
          max={listMax}
          label={$t('instance.l10n.resizeList')}
          testId="l10n-list-splitter"
        />
        <section
          class="flex flex-1 min-w-0 flex-col overflow-hidden"
          data-testid="l10n-detail-pane"
        >
          {#if selectedNamespace && instanceId}
            <KeyTable
              {instanceId}
              namespace={selectedNamespace}
              {lang}
              onOverrideSaved={refreshCoverageSilently}
              reloadToken={keyReloadToken}
            />
          {:else}
            <div
              class="flex flex-1 items-center justify-center p-4 text-center text-sm text-muted"
              data-testid="l10n-detail-placeholder"
            >
              {$t('instance.l10n.detailPlaceholder')}
            </div>
          {/if}
        </section>
      </div>
    {/if}
  </Modal>
  <!--
    Stacked AFTER the modal it covers, per Modal.svelte's mount-order == paint-
    order invariant: all modals share z-50, so the last-mounted one must also be
    the last-painted one or Escape would close the wrong layer.
  -->
  {#if prefillScope && instanceId}
    <PrefillDialog
      {instanceId}
      {lang}
      namespace={prefillScope.namespace}
      onClose={() => (prefillScope = null)}
      onFinished={afterPrefillRun}
    />
  {/if}
  <!--
    None of the three owns an `open` prop — this modal decides when each one
    exists, which is also what lets the offer dialog close itself by calling
    `onClose` the moment it finds nothing worth offering.
  -->
  {#if shareExportOpen}
    <ShareExportDialog
      {lang}
      {mcVersion}
      instanceNamespaces={coverage?.namespaces.map((n) => n.namespace) ?? []}
      onClose={() => (shareExportOpen = false)}
    />
  {/if}
  {#if shareImportOpen}
    <ShareImportDialog
      {lang}
      onImported={handleImported}
      onClose={() => (shareImportOpen = false)}
    />
  {/if}
  {#if offerOpen}
    <ApplyTargetsDialog
      lang={offerLang}
      exclude={offerExclude}
      unsolicited={offerUnsolicited}
      onClose={() => (offerOpen = false)}
    />
  {/if}
{/if}
