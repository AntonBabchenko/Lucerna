<script lang="ts">
  import {
    commands,
    type Diagnosis,
    type LogFileMeta,
    type LogSource,
    type RepairChoice,
    type RepairPlan,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import RepairConfirmCard from '$lib/logs/RepairConfirmCard.svelte';
  import { enqueueRepair } from '$lib/logs/repair-ops.svelte';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { LOGS_STEPS } from '$lib/onboarding/contextual-tours';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import {
    groupStackFolds,
    maybeParseCrashReport,
    tagWithSeverity,
    type CrashSection,
    type RenderUnit,
    type Severity,
  } from './render';

  let {
    open = $bindable(false),
    initialPath = null as string | null,
    instanceId = null as string | null,
    instanceName = null as string | null,
  }: {
    open: boolean;
    initialPath?: string | null;
    instanceId?: string | null;
    instanceName?: string | null;
  } = $props();

  // ---------------------------------------------------------------------------
  // Read-cap
  // ---------------------------------------------------------------------------

  const CAP_STORAGE_KEY = 'ftl.logs.cap_bytes';
  const CAP_OPTIONS: { label: string; value: number }[] = [
    { label: '1 MB', value: 1 * 1024 * 1024 },
    { label: '5 MB', value: 5 * 1024 * 1024 },
    { label: '25 MB', value: 25 * 1024 * 1024 },
    { label: '100 MB', value: 100 * 1024 * 1024 },
  ];

  function readCapFromStorage(): number {
    if (typeof localStorage === 'undefined') return CAP_OPTIONS[1].value;
    const raw = localStorage.getItem(CAP_STORAGE_KEY);
    const parsed = raw ? Number(raw) : NaN;
    if (!Number.isFinite(parsed) || parsed <= 0) return CAP_OPTIONS[1].value;
    return parsed;
  }

  // ---------------------------------------------------------------------------
  // Core state
  // ---------------------------------------------------------------------------

  let files = $state<LogFileMeta[]>([]);
  let listError = $state<string | null>(null);
  let selectedPath = $state<string | null>(null);
  let selectedContent = $state<string>('');
  let diagnosis = $state<Diagnosis | null>(null);
  // Auto-repair: the concrete plan is fetched lazily on "Fix this" intent.
  let repairPlan = $state<RepairPlan | null>(null);
  let repairLoading = $state(false);
  let repairUnavailable = $state(false);
  let repairApplying = $state(false);
  let contentError = $state<string | null>(null);
  let loadingContent = $state(false);
  let capBytes = $state<number>(readCapFromStorage());
  let search = $state('');
  // Debounced mirror of `search`. The input stays bound to `search` for
  // responsive typing, but the expensive O(N) match scan + full-body
  // re-highlight key off `debouncedSearch`, so they run once the user pauses
  // rather than on every keystroke (jank-free on multi-thousand-line logs).
  let debouncedSearch = $state('');
  const SEARCH_DEBOUNCE_MS = 120;

  // ---------------------------------------------------------------------------
  // Toolbar display preferences (persisted)
  // ---------------------------------------------------------------------------

  let wrap = $state(localStorage.getItem('ftl.logs.wrap') !== '0');
  let fold = $state(localStorage.getItem('ftl.logs.fold') !== '0');
  let hiddenLevels = $state<Set<Severity>>(
    new Set(
      (localStorage.getItem('ftl.logs.levels') ?? '').split(',').filter(Boolean) as Severity[],
    ),
  );

  $effect(() => {
    localStorage.setItem('ftl.logs.wrap', wrap ? '1' : '0');
  });
  $effect(() => {
    localStorage.setItem('ftl.logs.fold', fold ? '1' : '0');
  });
  $effect(() => {
    localStorage.setItem('ftl.logs.levels', Array.from(hiddenLevels).join(','));
  });

  // ---------------------------------------------------------------------------
  // Share state
  // ---------------------------------------------------------------------------

  let shareConfirm = $state(false);
  let shareUploading = $state(false);
  let shareUrl = $state<string | null>(null);

  // Clear share pill when a different file is opened
  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    selectedPath; // reactive dependency
    shareUrl = null;
    shareConfirm = false;
  });

  async function doShare() {
    if (!selectedContent) return;
    shareUploading = true;
    const result = await commands.shareLogToMclogs(selectedContent);
    shareUploading = false;
    shareConfirm = false;
    if (result.status === 'ok') {
      shareUrl = result.data;
    } else {
      pushWarning($t('logs.share.uploadFailed'), [formatError(result.error)]);
    }
  }

  async function openLogFolder() {
    if (!selectedPath) return;
    const r = await commands.openLogFolder(selectedPath);
    if (r.status !== 'ok') {
      pushWarning($t('logs.openFolder.errorToast'), [formatError(r.error)]);
    }
  }

  // ---------------------------------------------------------------------------
  // Crash-report raw-view toggle
  // ---------------------------------------------------------------------------

  let rawView = $state(false);

  // Reset when switching files
  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    selectedPath;
    rawView = false;
  });

  // ---------------------------------------------------------------------------
  // Fold-expansion map (per render-unit index, crash-section expansion map)
  // ---------------------------------------------------------------------------

  let foldExpanded = $state<Map<number, boolean>>(new Map());
  let sectionExpanded = $state<Map<number, boolean>>(new Map());

  function toggleFold(index: number) {
    const next = new Map(foldExpanded);
    next.set(index, !next.get(index));
    foldExpanded = next;
  }

  function toggleSection(index: number) {
    const next = new Map(sectionExpanded);
    next.set(index, !next.get(index));
    sectionExpanded = next;
  }

  // Reset fold/section maps when content changes
  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    selectedContent;
    foldExpanded = new Map();
    sectionExpanded = new Map();
  });

  // ---------------------------------------------------------------------------
  // Effects: load on open / initialPath
  // ---------------------------------------------------------------------------

  $effect(() => {
    if (open) {
      void reloadList();
    }
  });

  $effect(() => {
    if (open && initialPath && initialPath !== selectedPath) {
      selectedPath = initialPath;
      void loadContent(initialPath);
    }
  });

  // ---------------------------------------------------------------------------
  // Data-loading functions
  // ---------------------------------------------------------------------------

  async function reloadList() {
    if (!instanceId) {
      files = [];
      return;
    }
    listError = null;
    const result = await commands.listLogFiles(instanceId);
    if (result.status === 'ok') {
      files = result.data;
    } else {
      listError = JSON.stringify(result.error);
    }
  }

  async function selectFile(path: string) {
    selectedPath = path;
    await loadContent(path);
  }

  async function loadContent(path: string) {
    loadingContent = true;
    contentError = null;
    diagnosis = null;
    repairPlan = null;
    repairUnavailable = false;
    repairApplying = false;
    const result = await commands.readLogFile(path, capBytes);
    loadingContent = false;
    if (result.status === 'ok') {
      selectedContent = result.data;
      if (instanceId) {
        const d = await commands.diagnoseLog(instanceId, path);
        if (d.status === 'ok') {
          diagnosis = d.data;
        } else {
          // biome-ignore lint/suspicious/noConsole: best-effort UI degradation when IPC fails
          console.warn('[LogsPopover] diagnose_log failed:', d.error);
          diagnosis = null;
        }
      }
    } else {
      contentError = JSON.stringify(result.error);
      selectedContent = '';
    }
  }

  // ---------------------------------------------------------------------------
  // Auto-repair (preview → confirm → apply)
  // ---------------------------------------------------------------------------

  async function startRepair() {
    if (!diagnosis || !selectedPath || !instanceId) return;
    repairLoading = true;
    repairUnavailable = false;
    repairPlan = null;
    const res = await commands.buildRepairPlan(instanceId, selectedPath);
    repairLoading = false;
    if (res.status === 'ok' && res.data) {
      repairPlan = res.data;
    } else {
      // No constructible plan → keep advisory text, show a soft note.
      repairUnavailable = true;
      if (res.status === 'error') {
        // biome-ignore lint/suspicious/noConsole: best-effort UI degradation when IPC fails
        console.warn('[LogsPopover] build_repair_plan failed:', res.error);
      }
    }
  }

  async function applyRepair(choice: RepairChoice) {
    if (!instanceId) return;
    // Keep the card open with a busy Apply button until the op reaches a
    // terminal result (enqueueRepair resolves once it has drained), then
    // close. A success/warning toast is emitted by the repair-ops store.
    repairApplying = true;
    try {
      await enqueueRepair(instanceId, instanceName ?? instanceId, choice);
    } finally {
      repairApplying = false;
      repairPlan = null;
    }
  }

  function cancelRepair() {
    repairPlan = null;
  }

  function onCapChange(value: number) {
    capBytes = value;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(CAP_STORAGE_KEY, String(value));
    }
    if (selectedPath) {
      void loadContent(selectedPath);
    }
  }

  // ---------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------

  function sourceLabel(s: LogSource): string {
    switch (s) {
      case 'game':
        return $t('logs.severity.game');
      case 'crash':
        return $t('logs.severity.crash');
      case 'launcher':
        return $t('logs.severity.launcher');
    }
  }

  function formatBytes(n: number | null | undefined): string {
    if (n == null) return '';
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  function formatMtime(ms: number | null | undefined): string {
    if (!ms) return '';
    return new Date(ms).toLocaleString();
  }

  function escapeHtml(s: string): string {
    return s
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }

  function escapeRegExp(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }

  // ---------------------------------------------------------------------------
  // Derived state
  // ---------------------------------------------------------------------------

  let groupedFiles = $derived(
    (['game', 'crash', 'launcher'] as LogSource[]).map((src) => ({
      source: src,
      label: sourceLabel(src),
      items: files.filter((f) => f.source === src),
    })),
  );

  let selectedMeta = $derived(files.find((f) => f.path === selectedPath) ?? null);

  let isTruncated = $derived(
    !!selectedMeta &&
      selectedContent.length > 0 &&
      ((selectedMeta.size_bytes ?? 0) > capBytes || selectedContent.length === capBytes),
  );

  // Render pipeline
  const renderModel = $derived.by((): RenderUnit[] => {
    const lines = selectedContent.split('\n');
    const tagged = tagWithSeverity(lines);
    const units = fold
      ? groupStackFolds(tagged)
      : tagged.map((l) => ({ kind: 'line' as const, text: l.text, level: l.level }));
    return units.filter((u) => !hiddenLevels.has(u.level));
  });

  // Crash-report detection
  const crashSections = $derived(maybeParseCrashReport(selectedContent));
  const isStructured = $derived(crashSections !== null && !rawView);

  // Severity counts
  const severityCounts = $derived.by(() => {
    const lines = selectedContent.split('\n');
    const tagged = tagWithSeverity(lines);
    const counts = new Map<Severity, number>();
    for (const l of tagged) {
      counts.set(l.level, (counts.get(l.level) ?? 0) + 1);
    }
    return counts;
  });

  // Levels that actually appear in the current file (for toolbar checkboxes)
  const activeLevels = $derived<Severity[]>(
    (['error', 'warn', 'info', 'debug', 'trace', 'fatal', 'other'] as Severity[]).filter((lv) =>
      severityCounts.has(lv),
    ),
  );

  // Level display names
  function levelLabel(lv: Severity): string {
    return lv.toUpperCase();
  }

  function toggleLevel(lv: Severity) {
    const next = new Set(hiddenLevels);
    if (next.has(lv)) {
      next.delete(lv);
    } else {
      next.add(lv);
    }
    hiddenLevels = next;
  }

  // ---------------------------------------------------------------------------
  // Search navigation
  // ---------------------------------------------------------------------------

  interface MatchLocation {
    unitIndex: number;
    charStart: number;
    charEnd: number;
  }

  // Mirror `search` into `debouncedSearch` after a short pause. Re-running on
  // each keystroke clears the previous timer, so only the last keystroke in a
  // burst commits — classic trailing debounce.
  $effect(() => {
    const s = search;
    const id = setTimeout(() => {
      debouncedSearch = s;
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(id);
  });

  // Collect all match locations from the renderModel
  const allMatches = $derived.by((): MatchLocation[] => {
    if (!debouncedSearch) return [];
    const needle = debouncedSearch.toLowerCase();
    const locs: MatchLocation[] = [];
    for (let ui = 0; ui < renderModel.length; ui++) {
      const unit = renderModel[ui];
      const text = unit.kind === 'line' ? unit.text : unit.firstFrame;
      let pos = 0;
      const lower = text.toLowerCase();
      while (pos < lower.length) {
        const idx = lower.indexOf(needle, pos);
        if (idx === -1) break;
        locs.push({ unitIndex: ui, charStart: idx, charEnd: idx + needle.length });
        pos = idx + needle.length;
      }
    }
    return locs;
  });

  let currentMatchIndex = $state(0);
  const totalMatches = $derived(allMatches.length);

  // Reset current index when the committed search or content changes
  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    debouncedSearch;
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    selectedContent;
    currentMatchIndex = 0;
  });

  function nextMatch() {
    if (totalMatches === 0) return;
    currentMatchIndex = (currentMatchIndex + 1) % totalMatches;
    scrollToCurrentMatch();
  }

  function prevMatch() {
    if (totalMatches === 0) return;
    currentMatchIndex = (currentMatchIndex - 1 + totalMatches) % totalMatches;
    scrollToCurrentMatch();
  }

  function scrollToCurrentMatch() {
    // Use a microtask so the DOM has updated
    requestAnimationFrame(() => {
      const el = document.querySelector('[data-match-active="true"]');
      if (el) el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    });
  }

  function onSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      search = '';
    } else if (e.key === 'Enter') {
      // Flush the debounce so Enter navigates the matches for exactly what is
      // typed, even if pressed within the debounce window.
      debouncedSearch = search;
      if (e.shiftKey) {
        prevMatch();
      } else {
        nextMatch();
      }
      e.preventDefault();
    }
  }

  // ---------------------------------------------------------------------------
  // Highlight helpers
  // ---------------------------------------------------------------------------

  /**
   * Render a line's text with search match highlighting.
   * Active match: bg-accent text-white; others: <mark> (yellow).
   * Returns HTML string.
   */
  function highlightLine(text: string, unitIndex: number, matchesForUnit: MatchLocation[]): string {
    if (!debouncedSearch || matchesForUnit.length === 0) return escapeHtml(text);
    let result = '';
    let pos = 0;
    for (const loc of matchesForUnit) {
      result += escapeHtml(text.slice(pos, loc.charStart));
      const matchText = escapeHtml(text.slice(loc.charStart, loc.charEnd));
      const globalIdx = allMatches.findIndex(
        (m) => m.unitIndex === unitIndex && m.charStart === loc.charStart,
      );
      const isActive = globalIdx === currentMatchIndex;
      if (isActive) {
        result += `<mark class="bg-accent text-white" data-match-active="true">${matchText}</mark>`;
      } else {
        result += `<mark>${matchText}</mark>`;
      }
      pos = loc.charEnd;
    }
    result += escapeHtml(text.slice(pos));
    return result;
  }

  function matchesForUnit(unitIndex: number): MatchLocation[] {
    return allMatches.filter((m) => m.unitIndex === unitIndex);
  }

  // ---------------------------------------------------------------------------
  // Severity CSS classes
  // ---------------------------------------------------------------------------

  function severityLineClass(level: Severity): string {
    switch (level) {
      case 'error':
      case 'fatal':
        return 'border-l-2 pl-2 border-danger bg-danger-bg';
      case 'warn':
        return 'border-l-2 pl-2 border-warning-text bg-warning-bg/30';
      case 'debug':
      case 'trace':
        return 'border-l-2 pl-2 border-border-subtle text-muted';
      case 'info':
        return 'border-l-2 pl-2 border-border-subtle';
      default:
        return 'border-l-2 pl-2 border-transparent';
    }
  }

  // ---------------------------------------------------------------------------
  // Crash section helpers
  // ---------------------------------------------------------------------------

  function isSectionDefaultOpen(section: CrashSection, index: number): boolean {
    if (section.title === 'Head') return true;
    if (section.body.includes('Exception') || section.body.includes('Error')) return true;
    return false;
  }

  function sectionRenderUnits(body: string): RenderUnit[] {
    const lines = body.split('\n');
    const tagged = tagWithSeverity(lines);
    const units = fold
      ? groupStackFolds(tagged)
      : tagged.map((l) => ({ kind: 'line' as const, text: l.text, level: l.level }));
    return units.filter((u) => !hiddenLevels.has(u.level));
  }

  function copyToClipboard(text: string) {
    void navigator.clipboard.writeText(text);
  }
</script>

{#if open}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/40">
    <button
      type="button"
      class="absolute inset-0"
      aria-label={$t('logs.close.scrimAriaLabel')}
      onclick={() => (open = false)}
      onkeydown={(e) => {
        if (e.key === 'Escape') open = false;
      }}
    ></button>
    <aside
      class="relative bg-surface rounded shadow-xl max-w-5xl w-full max-h-[85vh] overflow-hidden flex flex-col m-4"
    >
      <!-- Header row 1: Read cap / Share / Reload / Close -->
      <header class="flex flex-col border-b">
        <div class="flex items-center justify-between px-4 py-2">
          <h2 class="text-sm font-semibold text-primary">{$t('logs.toolbar.title')}</h2>
          <div class="flex items-center gap-3 flex-wrap">
            <div class="text-xs flex items-center gap-1" data-tour-ctx="logs-cap">
              {$t('logs.toolbar.readCap')}
              <Select
                class="text-xs"
                ariaLabel={$t('logs.toolbar.readCap')}
                value={capBytes}
                options={CAP_OPTIONS}
                onChange={(v) => onCapChange(Number(v))}
              />
            </div>
            <button
              class="btn-secondary btn-xs inline-flex items-center gap-1.5"
              onclick={() => void reloadList()}
            >
              <Icon name="refresh" class="icon-spin-hover" />{$t('logs.toolbar.reload')}
            </button>

            <!-- Open the directory containing the currently-selected log file
                 in the OS file manager. Disabled when no file is selected. -->
            <button
              class="btn-tertiary text-xs inline-flex items-center gap-1"
              data-tour-ctx="logs-open-folder"
              disabled={!selectedPath}
              onclick={() => void openLogFolder()}
            >
              {$t('logs.toolbar.openFolder')}
              <Icon name="folderOpen" size={14} />
            </button>

            <!-- Share button -->
            <button
              class="btn-secondary btn-xs"
              data-tour-ctx="logs-share"
              disabled={!selectedContent || shareUploading}
              onclick={() => (shareConfirm = true)}
            >
              {shareUploading ? $t('logs.share.uploading') : $t('logs.share.shareBtn')}
            </button>

            <CloseButton
              onClick={() => (open = false)}
              ariaLabel={$t('logs.close.popoverAriaLabel')}
            />
          </div>
        </div>

        <!-- Header row 2: toolbar (wrap / fold / level filters) -->
        <div
          class="flex items-center gap-4 px-4 py-1.5 border-t text-xs flex-wrap"
          data-tour-ctx="logs-toolbar"
        >
          <label class="flex items-center gap-1 cursor-pointer select-none">
            <input type="checkbox" bind:checked={wrap} class="accent-primary" />
            {$t('logs.toolbar.wrapLines')}
          </label>
          <label class="flex items-center gap-1 cursor-pointer select-none">
            <input type="checkbox" bind:checked={fold} class="accent-primary" />
            {$t('logs.toolbar.collapseStacks')}
          </label>
          {#if activeLevels.length > 0}
            <span class="text-muted">{$t('logs.toolbar.showLevels')}</span>
            {#each activeLevels as lv}
              <label class="flex items-center gap-1 cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={!hiddenLevels.has(lv)}
                  onchange={() => toggleLevel(lv)}
                  class="accent-primary"
                />
                {levelLabel(lv)}
                <span class="text-muted">({severityCounts.get(lv) ?? 0})</span>
              </label>
            {/each}
          {/if}
          {#if crashSections !== null}
            <label class="flex items-center gap-1 cursor-pointer select-none ml-auto">
              <input type="checkbox" bind:checked={rawView} class="accent-primary" />
              {$t('logs.toolbar.rawView')}
            </label>
          {/if}
        </div>
      </header>

      <!-- Share confirm dialog -->
      {#if shareConfirm}
        <div class="absolute inset-0 z-50 flex items-center justify-center bg-black/50">
          <div class="bg-surface rounded shadow-xl p-6 max-w-sm w-full mx-4">
            <h3 class="font-semibold text-sm mb-2">{$t('logs.share.dialogTitle')}</h3>
            <p class="text-xs text-muted mb-4">
              {$t('logs.share.dialogBody')}
            </p>
            <div class="flex gap-2 justify-end">
              <button class="btn-secondary btn-xs" onclick={() => (shareConfirm = false)}>
                {$t('common.cancel')}
              </button>
              <button class="btn-warning btn-xs" onclick={() => void doShare()}>
                {$t('logs.share.uploadBtn')}
              </button>
            </div>
          </div>
        </div>
      {/if}

      <!-- Share URL pill -->
      {#if shareUrl}
        <div
          class="flex items-center gap-2 px-4 py-1.5 bg-warning-bg border-b text-xs text-warning-text"
        >
          <span class="font-semibold">{$t('logs.share.sharedLabel')}</span>
          <a href={shareUrl} target="_blank" rel="noopener noreferrer" class="underline truncate"
            >{shareUrl}</a
          >
          <button class="btn-secondary btn-xs" onclick={() => copyToClipboard(shareUrl!)}>
            {$t('logs.share.copyBtn')}
          </button>
          <CloseButton
            onClick={() => (shareUrl = null)}
            ariaLabel={$t('logs.share.dismissAriaLabel')}
            class="ml-auto"
          />
        </div>
      {/if}

      <div class="flex-1 flex overflow-hidden">
        <!-- Sidebar: grouped file list -->
        <nav class="w-72 border-r overflow-y-auto text-sm" data-tour-ctx="logs-sidebar">
          {#if listError}
            <p class="p-3 text-danger text-xs">
              {$t('logs.empty.listError', { error: listError })}
            </p>
          {:else if files.length === 0}
            <p class="p-3 text-muted text-xs">{$t('logs.empty.noLogs')}</p>
          {:else}
            {#each groupedFiles as group}
              {#if group.items.length > 0}
                <h3 class="px-3 pt-2 pb-1 text-xs font-semibold uppercase text-muted">
                  {group.label}
                </h3>
                <ul>
                  {#each group.items as f}
                    <li>
                      <button
                        class="w-full text-left px-3 py-1 hover:bg-subtle {selectedPath === f.path
                          ? 'bg-accent-soft'
                          : ''}"
                        onclick={() => void selectFile(f.path)}
                      >
                        <div class="font-mono text-xs truncate">{f.name}</div>
                        <div class="text-[10px] text-muted">
                          {formatBytes(f.size_bytes)} · {formatMtime(f.modified_unix_ms)}
                        </div>
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            {/each}
          {/if}
        </nav>

        <!-- Main content area -->
        <section class="flex-1 flex flex-col overflow-hidden">
          <!-- Search bar -->
          <div class="px-3 py-2 border-b flex items-center gap-2" data-tour-ctx="logs-search">
            <input
              class="flex-1 border rounded px-2 py-1 text-xs"
              placeholder={$t('logs.search.placeholder')}
              bind:value={search}
              disabled={!selectedPath}
              onkeydown={onSearchKeydown}
            />
            {#if debouncedSearch && totalMatches > 0}
              <span class="text-xs text-muted whitespace-nowrap">
                {$t('logs.search.matchCounter', {
                  current: currentMatchIndex + 1,
                  total: totalMatches,
                })}
              </span>
            {:else if debouncedSearch && totalMatches === 0}
              <span class="text-xs text-muted whitespace-nowrap">{$t('logs.search.noMatches')}</span
              >
            {/if}
            <button
              class="btn-icon"
              disabled={totalMatches === 0}
              aria-label={$t('logs.search.prevAriaLabel')}
              title={$t('logs.search.prevTitle')}
              onclick={prevMatch}
            >
              <Icon name="chevronUp" />
            </button>
            <button
              class="btn-icon"
              disabled={totalMatches === 0}
              aria-label={$t('logs.search.nextAriaLabel')}
              title={$t('logs.search.nextTitle')}
              onclick={nextMatch}
            >
              <Icon name="chevronDown" />
            </button>
          </div>

          {#if loadingContent}
            <div class="flex justify-center p-4 text-secondary">
              <Spinner delayMs={150} label={$t('logs.empty.reading')} />
            </div>
          {:else if contentError}
            <p class="p-4 text-sm text-danger">{contentError}</p>
          {:else if !selectedPath}
            <p class="p-4 text-sm text-muted">{$t('logs.empty.noFile')}</p>
          {:else}
            {#if isTruncated}
              <div class="px-3 py-1 bg-warning-bg text-warning-text text-xs border-b">
                {$t('logs.empty.truncated', { cap: formatBytes(capBytes) })}
              </div>
            {/if}

            <!-- Diagnosis card -->
            {#if diagnosis}
              <details
                open
                class="mx-3 mt-3 border border-warning-text/30 bg-warning-bg rounded p-3 shrink-0"
              >
                <summary class="cursor-pointer font-semibold text-warning-text select-none">
                  <span class="flex items-center gap-1.5"
                    ><Icon name="warning" /> {diagnosis.title}</span
                  >
                </summary>
                <p class="mt-2 text-sm text-warning-text selectable">{diagnosis.explanation}</p>
                <p class="mt-2 text-sm text-warning-text selectable">
                  <span class="font-semibold">{$t('logs.diagnosis.whatToTry')}</span>
                  {diagnosis.recommendation}
                </p>
                {#if diagnosis.repair && instanceId}
                  {#if repairPlan}
                    <RepairConfirmCard
                      plan={repairPlan}
                      busy={repairApplying}
                      onConfirm={applyRepair}
                      onCancel={cancelRepair}
                    />
                  {:else}
                    <button
                      type="button"
                      class="btn-primary btn-sm mt-2"
                      data-testid="repair-start"
                      disabled={repairLoading}
                      onclick={startRepair}
                    >
                      {repairLoading ? $t('logs.repair.checking') : $t('logs.repair.fixThis')}
                    </button>
                    {#if repairUnavailable}
                      <p class="mt-1 text-xs text-warning-text/80">
                        {$t('logs.repair.unavailable')}
                      </p>
                    {/if}
                  {/if}
                {/if}
                {#if diagnosis.matched_excerpt}
                  <pre
                    class="mt-2 text-xs font-mono bg-surface p-2 rounded border border-warning-text/30 overflow-x-auto whitespace-pre-wrap selectable">{diagnosis.matched_excerpt}</pre>
                {/if}
              </details>
            {/if}

            <!-- Log body: structured crash view or standard line view.
                 selectable so the user can drag-copy log lines / crash
                 stacks into a bug report or to mclo.gs. -->
            <div class="flex-1 overflow-auto font-mono text-xs leading-tight bg-base selectable">
              {#if isStructured && crashSections}
                <!-- Path A: structured crash report sections -->
                <div class="px-3 py-2 space-y-2">
                  {#each crashSections as section, si}
                    {@const defaultOpen = isSectionDefaultOpen(section, si)}
                    {@const expanded = sectionExpanded.get(si) ?? defaultOpen}
                    {@const units = sectionRenderUnits(section.body)}
                    <div class="border rounded overflow-hidden">
                      <button
                        type="button"
                        class="w-full text-left px-3 py-1.5 bg-subtle hover:bg-accent-soft font-semibold flex items-center gap-1"
                        onclick={() => toggleSection(si)}
                      >
                        <span class="text-muted"
                          ><Icon name={expanded ? 'chevronDown' : 'caret'} /></span
                        >
                        {section.title}
                      </button>
                      {#if expanded}
                        <div class="px-0 py-1">
                          {#each units as unit, ui}
                            {@const globalUnitIndex = renderModel.findIndex(
                              (u) =>
                                u.kind === unit.kind &&
                                (u.kind === 'line' ? u.text : u.firstFrame) ===
                                  (unit.kind === 'line' ? unit.text : unit.firstFrame),
                            )}
                            {#if unit.kind === 'line'}
                              <div class={severityLineClass(unit.level)} class:min-w-max={!wrap}>
                                <span
                                  class={wrap
                                    ? 'whitespace-pre-wrap break-words'
                                    : 'whitespace-pre'}
                                  ><!-- eslint-disable-next-line svelte/no-at-html-tags -->{@html highlightLine(
                                    unit.text,
                                    globalUnitIndex >= 0 ? globalUnitIndex : ui,
                                    matchesForUnit(globalUnitIndex >= 0 ? globalUnitIndex : ui),
                                  )}</span
                                >
                              </div>
                            {:else}
                              {@const foldKey = si * 100000 + ui}
                              {@const isExpanded = foldExpanded.get(foldKey) ?? false}
                              <div class={severityLineClass(unit.level)} class:min-w-max={!wrap}>
                                <button
                                  type="button"
                                  class="btn-tertiary text-left w-full inline-flex items-center gap-1.5"
                                  onclick={() => toggleFold(foldKey)}
                                >
                                  <Icon name={isExpanded ? 'chevronDown' : 'caret'} />
                                  <span
                                    class={wrap
                                      ? 'whitespace-pre-wrap break-words'
                                      : 'whitespace-pre'}>{unit.firstFrame}</span
                                  >
                                  {#if !isExpanded}
                                    <span class="text-muted italic"
                                      >({$t('logs.empty.foldMore', {
                                        count: unit.hiddenFrames.length,
                                      })})</span
                                    >
                                  {/if}
                                </button>
                                {#if isExpanded}
                                  {#each unit.hiddenFrames as frame}
                                    <div class="pl-4">
                                      <span
                                        class={wrap
                                          ? 'whitespace-pre-wrap break-words'
                                          : 'whitespace-pre'}>{frame}</span
                                      >
                                    </div>
                                  {/each}
                                {/if}
                              </div>
                            {/if}
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {/each}
                </div>
              {:else}
                <!-- Path B: standard line-by-line view -->
                <div class="px-3 py-2">
                  {#each renderModel as unit, ui}
                    {#if unit.kind === 'line'}
                      <div class="log-row {severityLineClass(unit.level)}" class:min-w-max={!wrap}>
                        <span class="text-placeholder select-none"
                          >{(ui + 1).toString().padStart(6, ' ')}:
                        </span><span
                          class={wrap ? 'whitespace-pre-wrap break-words' : 'whitespace-pre'}
                          ><!-- eslint-disable-next-line svelte/no-at-html-tags -->{@html highlightLine(
                            unit.text,
                            ui,
                            matchesForUnit(ui),
                          )}</span
                        >
                      </div>
                    {:else}
                      {@const isExpanded = foldExpanded.get(ui) ?? false}
                      <div class="log-row {severityLineClass(unit.level)}" class:min-w-max={!wrap}>
                        <button
                          type="button"
                          class="btn-tertiary text-left w-full"
                          onclick={() => toggleFold(ui)}
                        >
                          <span class="text-placeholder select-none"
                            >{(ui + 1).toString().padStart(6, ' ')}:
                          </span><Icon name={isExpanded ? 'chevronDown' : 'caret'} />
                          <span class={wrap ? 'whitespace-pre-wrap break-words' : 'whitespace-pre'}
                            >{unit.firstFrame}</span
                          >
                          {#if !isExpanded}
                            <span class="text-muted italic"
                              >({$t('logs.empty.foldMore', {
                                count: unit.hiddenFrames.length,
                              })})</span
                            >
                          {/if}
                        </button>
                        {#if isExpanded}
                          {#each unit.hiddenFrames as frame}
                            <div class="pl-8">
                              <span
                                class={wrap ? 'whitespace-pre-wrap break-words' : 'whitespace-pre'}
                                >{frame}</span
                              >
                            </div>
                          {/each}
                        {/if}
                      </div>
                    {/if}
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        </section>
      </div>
      <ContextualTour id="logs" steps={LOGS_STEPS} />
    </aside>
  </div>
{/if}

<style>
  /*
   * Native windowing for the standard line view: the browser skips layout +
   * paint for rows outside (and near) the viewport, so a multi-thousand-line
   * log scrolls smoothly without us virtualizing the list by hand. The rows
   * stay in the DOM, so search highlighting, match counts, and
   * `scrollIntoView` on the active match all keep working unchanged. The
   * `auto` keyword remembers each row's last rendered size, so estimates stay
   * accurate even with line wrapping. Browsers without content-visibility
   * support (older WebKitGTK) simply ignore this and render every row — graceful
   * degradation, no behavioural change.
   */
  .log-row {
    content-visibility: auto;
    contain-intrinsic-size: auto 1.25em;
  }
</style>
