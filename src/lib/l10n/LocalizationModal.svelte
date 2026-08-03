<script lang="ts">
  // Full-window shell for the per-instance translation-coverage report: a
  // namespace list (least-translated first) on the left, a target-language
  // picker + Apply action in the header, and the key editor (KeyTable) on
  // the right once a namespace is picked.
  //
  // Own module rather than a section of ManageInstancesModal.svelte, which is
  // already at this project's 800-line file ceiling.
  import { commands, type InstanceCoverage, type NamespaceCoverage } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushInfo, pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { coverageTone, namespacePercent, sortNamespaces, type CoverageTone } from './coverage';
  import KeyTable from './KeyTable.svelte';
  import PrefillDialog from './PrefillDialog.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
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
    aiConsent = false,
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
    /** `general.allow_ai_translation`. A PROP, not a read: this component's
     *  test suite pins exact `l10nCoverage` call counts, and +page.svelte
     *  already reads settings — adding a second reader here would only give
     *  the two of them a way to disagree. */
    aiConsent?: boolean;
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
      nsOrder = sortNamespaces(res.data.namespaces).map((r) => r.namespace);
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

  function afterPrefillRun() {
    void refreshCoverageSilently();
    keyReloadToken += 1;
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
          pushSuccess($t('instance.l10n.apply.toastAppliedTitle'));
        } else {
          // `false` is not a failure — the pack is written and registered,
          // it just can't flip on in options.txt yet because the instance
          // has never been launched. Say so plainly instead of either
          // claiming success or raising an error for something that worked.
          pushInfo($t('instance.l10n.apply.toastDeferredTitle'), [
            $t('instance.l10n.apply.toastDeferredLine'),
          ]);
        }
      } else {
        pushWarning($t('instance.l10n.apply.toastFailedTitle'), [formatError(res.error)]);
      }
    } finally {
      applying = false;
    }
  }

  const applyReason = $derived.by(() => {
    if (!coverage) return '';
    if (coverage.applyGate === 'unknown_format')
      return $t('instance.l10n.apply.reasonUnknownFormat');
    if (coverage.applyGate === 'too_old') return $t('instance.l10n.apply.reasonTooOld');
    return '';
  });

  // The pre-fill needs both permissions to be meaningful: consent to reach a
  // provider at all, and an instance the resulting pack could actually be
  // applied to. Offering the buttons without the second would let a user pay
  // for strings this Minecraft version can never load.
  const canPrefill = $derived(aiConsent && coverage?.applyGate === 'ready');

  // `null` = the dialog is closed. `{ namespace: null }` = whole instance.
  let prefillScope = $state<{ namespace: string | null } | null>(null);

  // Rows in the pinned order, with any namespace that appeared after the last
  // full load appended (sorted) rather than dropped.
  const sortedNamespaces = $derived.by(() => {
    if (!coverage) return [];
    const byName = new Map(coverage.namespaces.map((r) => [r.namespace, r]));
    const pinned = nsOrder
      .map((name) => byName.get(name))
      .filter((r): r is NamespaceCoverage => r !== undefined);
    const seen = new Set(nsOrder);
    const fresh = sortNamespaces(coverage.namespaces.filter((r) => !seen.has(r.namespace)));
    return [...pinned, ...fresh];
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
      <h2 id="l10n-modal-title" class="font-semibold text-primary">
        {$t('instance.l10n.title')}
      </h2>
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
        {#if canPrefill}
          <button
            type="button"
            class="btn-secondary btn-sm"
            data-testid="l10n-prefill-all"
            onclick={() => (prefillScope = { namespace: null })}
          >
            {$t('instance.l10n.prefill.allButton')}
          </button>
        {/if}
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
        {/if}
        <CloseButton onClick={close} ariaLabel={$t('instance.l10n.closeLabel')} />
      </div>
    </header>
    {#if coverage?.packState === 'present_not_enabled'}
      <!--
        The Finding-2 scenario: a modpack update's own overrides/options.txt
        overwrote the instance's options.txt wholesale, wiping the
        resourcePacks entry while leaving the generated pack file itself on
        disk (see l10n::options_txt's module doc). Re-running Apply rebuilds
        and re-registers the pack AND re-enables it in options.txt in one
        call — the same action the header's Apply button already performs —
        so this reuses it rather than a distinct command.
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
    {/if}
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
        {:else if sortedNamespaces.length === 0}
          <p class="p-3 text-sm text-muted" data-testid="l10n-empty">
            {$t('instance.l10n.empty')}
          </p>
        {:else}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <ul class="flex flex-col gap-1" onkeydown={onListKeydown}>
            {#each sortedNamespaces as row, i (row.namespace)}
              {@const percent = namespacePercent(row)}
              {@const selected = selectedNamespace === row.namespace}
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
                {#if canPrefill}
                  <!--
                    Icon-only action ⇒ .btn-icon family carrying use:tooltip AND
                    aria-label from the same key (DESIGN.md §5). The tooltip sits
                    on the button, not on a wrapping span: the action stays
                    enabled, and the span form only opens on hover — its
                    :focus-visible check can never match a non-focusable
                    wrapper, so keyboard focus would reach the button silently.
                    tabindex follows the roving row so consent-on does not
                    double the list's tab stops: Tab from the focused row
                    reaches its own AI button and then leaves the list.
                  -->
                  <button
                    type="button"
                    class="btn-icon btn-icon-sm shrink-0"
                    tabindex={i === focusIndex ? 0 : -1}
                    aria-label={$t('instance.l10n.prefill.namespaceButtonAria', {
                      namespace: row.namespace,
                    })}
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
                    use:tooltip={$t('instance.l10n.prefill.namespaceButtonAria', {
                      namespace: row.namespace,
                    })}
                  >
                    <Icon name="aiTranslate" size={14} />
                  </button>
                {/if}
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
      <section class="flex flex-1 min-w-0 flex-col overflow-hidden" data-testid="l10n-detail-pane">
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
{/if}
