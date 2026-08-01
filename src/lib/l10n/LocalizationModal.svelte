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
    lang = $bindable(),
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
  } = $props();

  const LIST_MIN_WIDTH = 220;
  const LIST_MAX_WIDTH = 420;
  let listWidth = $state(280);

  let coverage = $state<InstanceCoverage | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  // Monotonic request id: a response only applies if it's still the most
  // recent request in flight, so a late response for an instance or language
  // the user has since navigated away from can't clobber newer state.
  let requestId = 0;

  async function load(id: string, requestedLang: string) {
    const myRequest = ++requestId;
    loading = true;
    loadError = null;
    const res = await commands.l10nCoverage(id, requestedLang);
    if (myRequest !== requestId) return; // superseded by a newer request
    loading = false;
    if (res.status === 'ok') {
      coverage = res.data;
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

  const sortedNamespaces = $derived(coverage ? sortNamespaces(coverage.namespaces) : []);
  const languageOptions = $derived(
    (coverage?.availableCodes ?? []).map((code) => ({ value: code, label: code })),
  );

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
            value={lang}
            options={languageOptions}
            onChange={(v) => (lang = String(v))}
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
