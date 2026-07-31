<script lang="ts">
  // Full-window shell for the per-instance translation-coverage report: a
  // namespace list (least-translated first) on the left, a target-language
  // picker in the header, and a placeholder detail pane on the right.
  //
  // This is the shell only — the key table and per-key editing land in a
  // later PR. Own module rather than a section of ManageInstancesModal.svelte,
  // which is already at this project's 800-line file ceiling.
  import { commands, type InstanceCoverage } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { coverageTone, namespacePercent, sortNamespaces, type CoverageTone } from './coverage';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import SplitterHandle from '$lib/ui/SplitterHandle.svelte';

  let {
    open = $bindable(),
    instanceId,
  }: {
    open: boolean;
    /** The instance to report on. Null while nothing is selected — the
     *  fetch effect below simply skips. */
    instanceId: string | null;
  } = $props();

  const LIST_MIN_WIDTH = 220;
  const LIST_MAX_WIDTH = 420;
  let listWidth = $state(280);

  let coverage = $state<InstanceCoverage | null>(null);
  // The user's explicit language pick. Null means "not overridden yet": the
  // load effect sends an empty lang so the backend derives one from the UI
  // locale, and the picker is seeded from the resolved `coverage.lang`
  // (via `selectedLang` below) rather than writing it back into `userLang` —
  // that keeps the seed from re-triggering the fetch a second time for the
  // same instance.
  let userLang = $state<string | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  // Monotonic request id: a response only applies if it's still the most
  // recent request in flight, so a late response for an instance or language
  // the user has since navigated away from can't clobber newer state.
  let requestId = 0;

  async function load(id: string, lang: string) {
    const myRequest = ++requestId;
    loading = true;
    loadError = null;
    const res = await commands.l10nCoverage(id, lang);
    if (myRequest !== requestId) return; // superseded by a newer request
    loading = false;
    if (res.status === 'ok') {
      coverage = res.data;
    } else {
      coverage = null;
      loadError = $t('instance.l10n.loadFailed', { error: formatError(res.error) });
    }
  }

  // Sentinel so the very first run (mount) always counts as an instance
  // change, even when `instanceId` starts out null.
  let lastInstanceId: string | null | undefined = undefined;

  // Split into two effects rather than one: the fetch effect below needs to
  // depend on BOTH `instanceId` and `userLang` (an instance switch OR an
  // explicit pick must each trigger a load), but this reset must run only
  // once per instance change and must not itself count as "the user picked a
  // language" for the fetch effect. Folding the reset into the fetch effect
  // (guarding the write with `untrack`) was tried first — it does not work:
  // `untrack` only skips new dependency registration for reads *inside* the
  // callback, so an unrelated ordinary read of `userLang` elsewhere in the
  // same run still resubscribes to the value it just (untracked-ly) wrote,
  // and the effect fires a second, redundant time. Keeping the reset in its
  // own instanceId-only effect sidesteps that: it never reads `userLang` at
  // all, so its write can't create a self-loop, and Svelte coalesces the
  // resulting instanceId + userLang invalidations of the fetch effect below
  // into a single run.
  $effect(() => {
    const id = instanceId;
    if (id !== lastInstanceId) {
      lastInstanceId = id;
      userLang = null;
    }
  });

  $effect(() => {
    if (!open) return;
    const id = instanceId;
    const lang = userLang ?? '';
    if (!id) {
      coverage = null;
      loadError = null;
      loading = false;
      return;
    }
    void load(id, lang);
  });

  const sortedNamespaces = $derived(coverage ? sortNamespaces(coverage.namespaces) : []);
  const languageOptions = $derived(
    (coverage?.availableCodes ?? []).map((code) => ({ value: code, label: code })),
  );
  // The user's pending pick wins immediately, so the picker reflects the
  // click before the response lands; once nothing is pending, fall back to
  // whatever the backend last resolved.
  const selectedLang = $derived(userLang ?? coverage?.lang ?? '');

  function toneClass(tone: CoverageTone): string {
    if (tone === 'ok') return 'text-success';
    if (tone === 'partial') return 'text-warning-text';
    return 'text-danger';
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
            value={selectedLang}
            options={languageOptions}
            onChange={(v) => (userLang = String(v))}
            ariaLabel={$t('instance.l10n.languageLabel')}
            dataTestid="l10n-language-select"
          />
        {/if}
        <CloseButton onClick={close} ariaLabel={$t('instance.l10n.closeLabel')} />
      </div>
    </header>
    <div class="flex flex-1 overflow-hidden">
      <aside
        class="shrink-0 overflow-y-auto p-2"
        style="width:{listWidth}px"
        aria-label={$t('instance.l10n.listRegionLabel')}
      >
        {#if loading}
          <LoadingPanel label={$t('instance.l10n.loading')} />
        {:else if loadError}
          <p class="p-3 text-sm text-danger" data-testid="l10n-error">{loadError}</p>
        {:else if sortedNamespaces.length === 0}
          <p class="p-3 text-sm text-muted" data-testid="l10n-empty">
            {$t('instance.l10n.empty')}
          </p>
        {:else}
          <ul class="flex flex-col gap-1">
            {#each sortedNamespaces as row (row.namespace)}
              {@const percent = namespacePercent(row)}
              <li>
                <!--
                  No aria-label: the namespace name and its percentage below
                  ARE the accessible content a screen-reader user needs — an
                  aria-label would replace both with nothing useful. See
                  OverviewTab.svelte's body-zone rule.
                -->
                <div
                  class="flex items-center justify-between gap-2 rounded px-2 py-1.5 text-sm"
                  data-testid="l10n-namespace-row"
                >
                  <span class="truncate">{row.namespace}</span>
                  <span class="font-mono {toneClass(coverageTone(percent))}">
                    {$t('instance.l10n.percentValue', { percent })}
                  </span>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </aside>
      <SplitterHandle
        bind:width={listWidth}
        min={LIST_MIN_WIDTH}
        max={LIST_MAX_WIDTH}
        label={$t('instance.l10n.resizeList')}
        testId="l10n-list-splitter"
      />
      <!-- Placeholder — the key table and per-key editing land in a later PR. -->
      <section
        class="flex flex-1 min-w-0 items-center justify-center p-4 text-center text-sm text-muted"
        data-testid="l10n-detail-placeholder"
      >
        {$t('instance.l10n.detailPlaceholder')}
      </section>
    </div>
  </Modal>
{/if}
