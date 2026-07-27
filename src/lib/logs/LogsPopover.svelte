<script lang="ts">
  import { onDestroy } from 'svelte';
  import {
    commands,
    type AnnotateResult,
    type Diagnosis,
    type LoaderKind,
    type LogFileMeta,
    type LogSource,
  } from '$lib/ipc/bindings';
  import { SvelteSet } from 'svelte/reactivity';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import LogDiagnosisBanner from '$lib/logs/LogDiagnosisBanner.svelte';
  import { latestDiagnosis, refreshDiagnosis } from '$lib/logs/log-diagnosis.svelte';
  import {
    inlineDiagnosisRedundant,
    logBannerEligible,
    logDiagnosisSignature,
  } from '$lib/logs/log-diagnosis-view';
  import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';
  import DiagnosisRestoreButton from '$lib/ui/DiagnosisRestoreButton.svelte';
  import { clearOldPreview } from '$lib/logs/manage';
  import { chooseOpenLog } from '$lib/logs/select-log';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { LOGS_STEPS } from '$lib/onboarding/contextual-tours';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';
  import ToggleChip from '$lib/ui/ToggleChip.svelte';
  import Select from '$lib/ui/Select.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import Spinner from '$lib/ui/Spinner.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { DIAGNOSIS_COPY } from '$lib/logs/diagnosis-copy';
  import LogHintBadge from '$lib/logs/LogHintBadge.svelte';
  import LogHintCard from '$lib/logs/LogHintCard.svelte';
  import {
    annotationMap,
    createLogHintHover,
    fallbackCopyMap,
    resolveHintCopy,
    type ActiveHint,
  } from '$lib/logs/log-hints.svelte';
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
    mcVersion = null as string | null,
    loader = null as LoaderKind | null,
    gameRunning = false,
  }: {
    open: boolean;
    initialPath?: string | null;
    instanceId?: string | null;
    instanceName?: string | null;
    // The active instance's MC version + loader, threaded through to
    // LogDiagnosisBanner so its repair cards can run the mod-browser install flow
    // (decideModInstall needs both). Null until an instance is selected.
    mcVersion?: string | null;
    loader?: LoaderKind | null;
    // True while any Minecraft process is running. Repairs that mutate instance
    // files can't run then (the JVM holds them open), so they're deferred until
    // the game closes instead of failing with InstanceBusy.
    gameRunning?: boolean;
  } = $props();

  // Restore affordance for a dismissed log diagnosis banner: shown only when the
  // banner would otherwise be visible but the user hid it.
  const latestLogDiagnosis = $derived(latestDiagnosis());
  const logRestoreSignature = $derived(logDiagnosisSignature(latestLogDiagnosis));
  const bannerDismissed = $derived(
    instanceId !== null &&
      logRestoreSignature !== null &&
      diagnosisDismiss.isDismissed(`log:${instanceId}`, logRestoreSignature),
  );
  const showLogRestore = $derived(logBannerEligible(latestLogDiagnosis) && bannerDismissed);

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
  // Log management: per-file delete (centered confirm dialog) + bulk "clear old".
  let confirmingDeletePath = $state<string | null>(null);
  let deletingPaths = $state(new SvelteSet<string>());
  let clearOldOpen = $state(false);
  let clearingOld = $state(false);
  const clearOldStats = $derived(clearOldPreview(files));
  let diagnosis = $state<Diagnosis | null>(null);
  // When the open file IS the latest log, the top banner already shows this same
  // diagnosis (title/explanation/fix) — collapse the inline card to its unique
  // raw `matched_excerpt` instead of repeating the prose.
  const inlineRedundant = $derived(
    inlineDiagnosisRedundant(selectedPath, latestLogDiagnosis, bannerDismissed),
  );

  // ---------------------------------------------------------------------------
  // Inline hint annotations (per-line badges + hover card)
  // ---------------------------------------------------------------------------

  let annotateResult = $state<AnnotateResult | null>(null);
  const annotations = $derived(annotationMap(annotateResult));
  const hintFallbacks = $derived(fallbackCopyMap(annotateResult));
  // When the current file has any annotations, EVERY row (line + fold, both
  // views) reserves a uniform left gutter and the badge sits absolutely inside
  // it — annotated lines' text must not shift out of column alignment.
  const hintGutter = $derived(annotations.size > 0);
  const hints = createLogHintHover();
  onDestroy(() => hints.dispose());

  function activateHint(patternId: string, el: HTMLElement, viaFocus: boolean) {
    const copy = resolveHintCopy(patternId, hintFallbacks, $t);
    if (!copy) return;
    const r = el.getBoundingClientRect();
    const hint: ActiveHint = {
      patternId,
      copy,
      anchor: { top: r.top, left: r.left, width: r.width, height: r.height, bottom: r.bottom },
    };
    if (viaFocus) hints.openFromFocus(hint);
    else hints.rowEnter(hint);
  }

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

  async function openLogFolder(path: string) {
    const r = await commands.openLogFolder(path);
    if (r.status !== 'ok') {
      pushWarning($t('logs.openFolder.errorToast'), [formatError(r.error)]);
    }
  }

  // Row-level share: select the file first so the confirm dialog uploads the
  // file the user clicked, then open the dialog. Ordering matters — the
  // selectedPath $effect above resets shareConfirm, and it has already re-run
  // by the time the awaited selectFile resolves.
  async function shareFile(path: string) {
    if (selectedPath !== path) {
      await selectFile(path);
    }
    if (!selectedContent) return; // load failed — the viewer already shows the error
    shareConfirm = true;
  }

  // ---------------------------------------------------------------------------
  // Crash-report raw-view toggle
  // ---------------------------------------------------------------------------

  let rawView = $state(false);

  // Reset when switching files
  $effect(() => {
    selectedPath;
    rawView = false;
    hints.close();
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
    selectedContent;
    foldExpanded = new Map();
    sectionExpanded = new Map();
  });

  // ---------------------------------------------------------------------------
  // Effects: load on open / initialPath
  // ---------------------------------------------------------------------------

  // On open — and on a mid-open deep-link change — refresh the file list and
  // select which log to show: an explicit `initialPath` deep-link when present,
  // otherwise the newest log (the backend sorts newest-first). This is what
  // surfaces a fresh run's log instead of a stale prior selection, and re-reads
  // content so a rewritten `latest.log` isn't shown from the previous run.
  // Keyed on `open`/`initialPath` only (not `selectedPath`), so manual dropdown
  // selection while the panel is open is left alone.
  $effect(() => {
    const isOpen = open;
    const deepLink = initialPath;
    if (!isOpen) return;
    void openViewer(deepLink);
  });

  // Refresh the banner's latest diagnosis when the popover opens so it shows
  // current state rather than whatever was last fetched.
  $effect(() => {
    if (open && instanceId) void refreshDiagnosis(instanceId);
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

  // Open-time selection: refresh the list, then show the deep-link (if still
  // present) or the newest log. `selectFile` loads its content, refreshing a
  // rewritten `latest.log` from a prior run.
  async function openViewer(deepLink: string | null) {
    // Best-effort: trim logs past the retention policy so the list shows the
    // already-trimmed set. Silent — never surface errors from this pass.
    if (instanceId) {
      await commands.applyLogRetention(instanceId);
    }
    await reloadList();
    const target = chooseOpenLog(files, deepLink);
    if (target) {
      await selectFile(target);
    } else {
      selectedPath = null;
      selectedContent = '';
    }
  }

  // Toolbar reload: refresh the list and re-read the currently-selected log
  // (so its content reflects the latest run), without changing the selection.
  async function reload() {
    await reloadList();
    if (selectedPath) {
      await loadContent(selectedPath);
    }
  }

  async function selectFile(path: string) {
    selectedPath = path;
    await loadContent(path);
  }

  // ---------------------------------------------------------------------------
  // Log management: per-file delete + "clear old"
  // ---------------------------------------------------------------------------

  async function deleteFile(path: string) {
    confirmingDeletePath = null;
    deletingPaths.add(path);
    const r = await commands.deleteLogFile(path);
    deletingPaths.delete(path);
    if (r.status !== 'ok') {
      pushWarning($t('logs.manage.delete'), [formatError(r.error)]);
      return;
    }
    if (selectedPath === path) {
      selectedPath = null;
      selectedContent = '';
    }
    await reloadList();
  }

  async function confirmClearOld() {
    if (!instanceId) return;
    clearingOld = true;
    const r = await commands.clearOldLogs(instanceId);
    clearingOld = false;
    clearOldOpen = false;
    if (r.status !== 'ok') {
      pushWarning($t('logs.manage.clearOld'), [formatError(r.error)]);
      return;
    }
    pushSuccess(
      $t('logs.manage.cleared', {
        count: r.data.deleted_count,
        size: formatSize($t, r.data.freed_bytes),
      }),
    );
    await reloadList();
    // If the open file was swept away, clear the viewer.
    if (selectedPath && !files.some((f) => f.path === selectedPath)) {
      selectedPath = null;
      selectedContent = '';
    }
  }

  async function loadContent(path: string) {
    loadingContent = true;
    contentError = null;
    diagnosis = null;
    annotateResult = null;
    const result = await commands.readLogFile(path, capBytes);
    loadingContent = false;
    // The user may have switched files while we awaited — a stale resolution
    // must not clobber the newer selection's state. Re-check after EVERY await
    // that precedes a state commit.
    if (selectedPath !== path) return;
    if (result.status === 'ok') {
      selectedContent = result.data;
      if (instanceId) {
        const d = await commands.diagnoseLog(instanceId, path);
        if (selectedPath !== path) return;
        if (d.status === 'ok') {
          diagnosis = d.data;
        } else {
          // biome-ignore lint/suspicious/noConsole: best-effort UI degradation when IPC fails
          console.warn('[LogsPopover] diagnose_log failed:', d.error);
          diagnosis = null;
        }
      }
      // Inline hint badges don't need an instance — annotate whatever content
      // was just loaded. Best-effort: a failure here degrades to no badges,
      // never blocks the log body from displaying (already set above).
      const a = await commands.annotateLogFile(path, capBytes, 'client');
      if (selectedPath !== path) return;
      if (a.status === 'ok') {
        annotateResult = a.data;
      } else {
        // biome-ignore lint/suspicious/noConsole: best-effort UI degradation when IPC fails
        console.warn('[LogsPopover] annotate_log_file failed:', a.error);
      }
    } else {
      contentError = JSON.stringify(result.error);
      selectedContent = '';
    }
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
      case 'game_console':
        return $t('logs.severity.gameConsole');
      case 'launcher':
        return $t('logs.severity.launcher');
      default:
        return s;
    }
  }

  /** Optional one-line hint shown under a group header. Empty for groups that
   *  need no explanation. */
  function sourceHint(s: LogSource): string {
    switch (s) {
      case 'game_console':
        return $t('logs.sourceHint.gameConsole');
      case 'launcher':
        return $t('logs.sourceHint.launcher');
      default:
        return '';
    }
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

  // ---------------------------------------------------------------------------
  // Derived state
  // ---------------------------------------------------------------------------

  let groupedFiles = $derived(
    (['game', 'game_console', 'crash', 'launcher'] as LogSource[]).map((src) => ({
      source: src,
      label: sourceLabel(src),
      hint: sourceHint(src),
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
      : tagged.map((l) => ({
          kind: 'line' as const,
          text: l.text,
          level: l.level,
          index: l.index,
        }));
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

  // Map a parsed severity to the shared ToggleChip tone palette. error/fatal →
  // danger; warn → warning; debug/trace → muted; everything else → neutral.
  function levelChipTone(lv: Severity): 'danger' | 'warning' | 'muted' | 'neutral' {
    switch (lv) {
      case 'error':
      case 'fatal':
        return 'danger';
      case 'warn':
        return 'warning';
      case 'debug':
      case 'trace':
        return 'muted';
      default:
        return 'neutral';
    }
  }

  // ---------------------------------------------------------------------------
  // Search navigation
  // ---------------------------------------------------------------------------

  // `unitKey` is an opaque, view-scoped identifier for the rendered unit a match
  // belongs to — NOT an index into `renderModel`. The structured crash view and
  // the standard line view render different unit sets, and duplicate lines
  // (blanks, repeated `\tat` frames) are common, so a match cannot be located by
  // text equality. Each view therefore assigns its own stable key space
  // (`unitKeyFor`) and the match list is (re)built from whichever view is
  // actually on screen, so highlight + active-match targeting land on the exact
  // rendered occurrence.
  interface MatchLocation {
    unitKey: number;
    charStart: number;
    charEnd: number;
  }

  // Stable per-view unit key. Standard view keys on the render-model index;
  // structured view keys on (section, unit) — the same scheme `foldKey` uses so
  // duplicate lines across sections never collide.
  const STRUCTURED_SECTION_STRIDE = 100000;
  function unitKeyFor(sectionIndex: number, unitIndex: number): number {
    return sectionIndex < 0 ? unitIndex : sectionIndex * STRUCTURED_SECTION_STRIDE + unitIndex;
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

  function collectUnitMatches(text: string, unitKey: number, needle: string): MatchLocation[] {
    const locs: MatchLocation[] = [];
    let pos = 0;
    const lower = text.toLowerCase();
    while (pos < lower.length) {
      const idx = lower.indexOf(needle, pos);
      if (idx === -1) break;
      locs.push({ unitKey, charStart: idx, charEnd: idx + needle.length });
      pos = idx + needle.length;
    }
    return locs;
  }

  // Collect all match locations from the units that are ACTUALLY rendered. In
  // the structured crash view that is the per-section unit list (keyed
  // section-scoped); otherwise it is the flat `renderModel`. Keeping the match
  // model aligned with the rendered view is what makes duplicate lines highlight
  // and navigate independently instead of collapsing onto the first occurrence.
  const allMatches = $derived.by((): MatchLocation[] => {
    if (!debouncedSearch) return [];
    const needle = debouncedSearch.toLowerCase();
    const locs: MatchLocation[] = [];
    if (isStructured && crashSections) {
      for (let si = 0; si < crashSections.length; si++) {
        const units = sectionRenderUnits(crashSections[si].body, crashSections[si].startLine);
        for (let ui = 0; ui < units.length; ui++) {
          const unit = units[ui];
          const text = unit.kind === 'line' ? unit.text : unit.firstFrame;
          locs.push(...collectUnitMatches(text, unitKeyFor(si, ui), needle));
        }
      }
    } else {
      for (let ui = 0; ui < renderModel.length; ui++) {
        const unit = renderModel[ui];
        const text = unit.kind === 'line' ? unit.text : unit.firstFrame;
        locs.push(...collectUnitMatches(text, unitKeyFor(-1, ui), needle));
      }
    }
    return locs;
  });

  let currentMatchIndex = $state(0);
  const totalMatches = $derived(allMatches.length);

  // Reset current index when the committed search or content changes
  $effect(() => {
    debouncedSearch;
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
      // Flush the debounce so highlights and the match counter clear at once,
      // rather than lingering for the debounce window after the input blanks.
      debouncedSearch = '';
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

  // Per-unit match index, built in ONE pass over `allMatches` with the global
  // index assigned during collection. The old shape re-filtered `allMatches`
  // per rendered unit and ran a per-match `findIndex` over the whole match set
  // — O(units×matches) + O(matches²) per next/prev press, a multi-second
  // freeze on a 5 MB log with a common search token.
  const matchesByUnit = $derived.by(() => {
    const map = new Map<number, Array<{ loc: MatchLocation; globalIdx: number }>>();
    for (let i = 0; i < allMatches.length; i++) {
      const m = allMatches[i];
      const arr = map.get(m.unitKey);
      if (arr) arr.push({ loc: m, globalIdx: i });
      else map.set(m.unitKey, [{ loc: m, globalIdx: i }]);
    }
    return map;
  });

  /**
   * Render a line's text with search match highlighting.
   * Active match: bg-accent text-white; others: <mark> (yellow).
   * Returns HTML string.
   */
  function highlightLine(
    text: string,
    unitMatches: Array<{ loc: MatchLocation; globalIdx: number }>,
  ): string {
    if (!debouncedSearch || unitMatches.length === 0) return escapeHtml(text);
    let result = '';
    let pos = 0;
    for (const { loc, globalIdx } of unitMatches) {
      result += escapeHtml(text.slice(pos, loc.charStart));
      const matchText = escapeHtml(text.slice(loc.charStart, loc.charEnd));
      if (globalIdx === currentMatchIndex) {
        result += `<mark class="bg-accent text-white" data-match-active="true">${matchText}</mark>`;
      } else {
        result += `<mark>${matchText}</mark>`;
      }
      pos = loc.charEnd;
    }
    result += escapeHtml(text.slice(pos));
    return result;
  }

  function matchesForUnit(unitKey: number): Array<{ loc: MatchLocation; globalIdx: number }> {
    return matchesByUnit.get(unitKey) ?? [];
  }

  // ---------------------------------------------------------------------------
  // Severity CSS classes
  // ---------------------------------------------------------------------------

  // `gutter` (per-file, not per-line) swaps the base padding for a wider
  // `relative pl-6` gutter that the absolutely-positioned hint badge sits in —
  // uniform across ALL rows so annotated lines stay column-aligned.
  function severityLineClass(level: Severity, gutter: boolean): string {
    const pad = gutter ? 'relative pl-6' : 'pl-2';
    switch (level) {
      case 'error':
      case 'fatal':
        return `border-l-2 ${pad} border-danger bg-danger-bg`;
      case 'warn':
        return `border-l-2 ${pad} border-warning-text bg-warning-bg/30`;
      case 'debug':
      case 'trace':
        return `border-l-2 ${pad} border-border-subtle text-muted`;
      case 'info':
        return `border-l-2 ${pad} border-border-subtle`;
      default:
        return `border-l-2 ${pad} border-transparent`;
    }
  }

  // ---------------------------------------------------------------------------
  // Crash section helpers
  // ---------------------------------------------------------------------------

  function isSectionDefaultOpen(section: CrashSection): boolean {
    if (section.title === 'Head') return true;
    if (section.body.includes('Exception') || section.body.includes('Error')) return true;
    return false;
  }

  function sectionRenderUnits(body: string, startLine: number): RenderUnit[] {
    const lines = body.split('\n');
    const tagged = tagWithSeverity(lines, startLine);
    const units = fold
      ? groupStackFolds(tagged)
      : tagged.map((l) => ({
          kind: 'line' as const,
          text: l.text,
          level: l.level,
          index: l.index,
        }));
    return units.filter((u) => !hiddenLevels.has(u.level));
  }

  function copyToClipboard(text: string) {
    void navigator.clipboard.writeText(text);
  }
</script>

{#if open}
  <Modal
    ariaLabelledby="logs-popover-title"
    onClose={() => (open = false)}
    panelClass="relative bg-surface rounded shadow-xl max-w-5xl w-full max-h-[85vh] overflow-hidden flex flex-col m-4"
  >
    <!-- Header row 1: action bar -->
    <header class="flex flex-col border-b">
      <div class="flex items-center justify-between px-4 py-2">
        <h2 id="logs-popover-title" class="text-sm font-semibold text-primary">
          {$t('logs.toolbar.title')}
        </h2>
        <div class="flex items-center gap-1.5">
          {#if showLogRestore && instanceId}
            <DiagnosisRestoreButton
              testid="log-diagnosis-restore"
              onRestore={() => diagnosisDismiss.restore(`log:${instanceId}`)}
            />
          {/if}
          <button
            type="button"
            class="btn-icon"
            aria-label={$t('logs.toolbar.reload')}
            use:tooltip={$t('logs.toolbar.reload')}
            onclick={() => void reload()}
          >
            <Icon name="refresh" class="icon-spin-hover" />
          </button>

          <button
            type="button"
            class="btn-icon btn-icon-danger"
            data-testid="clear-old-logs"
            aria-label={$t('logs.manage.clearOld')}
            use:tooltip={$t('logs.manage.clearOld')}
            disabled={clearOldStats.count === 0}
            onclick={() => (clearOldOpen = true)}
          >
            <Icon name="eraser" />
          </button>

          <CloseButton
            onClick={() => (open = false)}
            ariaLabel={$t('logs.close.popoverAriaLabel')}
          />
        </div>
      </div>

      <!-- Header row 2: view settings (left) + level filters (right) -->
      <div
        class="flex items-center gap-4 px-4 py-1.5 border-t text-xs flex-wrap"
        data-tour-ctx="logs-toolbar"
      >
        <span class="text-muted">{$t('logs.toolbar.readCap')}</span>
        <div data-tour-ctx="logs-cap">
          <Select
            class="text-xs"
            ariaLabel={$t('logs.toolbar.readCap')}
            value={capBytes}
            options={CAP_OPTIONS}
            onChange={(v) => onCapChange(Number(v))}
          />
        </div>
        <label class="flex items-center gap-1 cursor-pointer select-none">
          <input type="checkbox" bind:checked={wrap} class="accent-primary" />
          {$t('logs.toolbar.wrapLines')}
        </label>
        <label class="flex items-center gap-1 cursor-pointer select-none">
          <input type="checkbox" bind:checked={fold} class="accent-primary" />
          {$t('logs.toolbar.collapseStacks')}
        </label>
        {#if crashSections !== null}
          <label class="flex items-center gap-1 cursor-pointer select-none">
            <input type="checkbox" bind:checked={rawView} class="accent-primary" />
            {$t('logs.toolbar.rawView')}
          </label>
        {/if}
      </div>

      <!-- Header row 3: severity-level filters (own line — there can be many) -->
      {#if activeLevels.length > 0}
        <div
          class="flex items-center gap-1.5 px-4 py-1.5 border-t text-xs flex-wrap"
          role="group"
          aria-label={$t('logs.toolbar.showLevels')}
        >
          <span class="text-muted mr-1">{$t('logs.toolbar.showLevels')}</span>
          {#each activeLevels as lv (lv)}
            <ToggleChip
              active={!hiddenLevels.has(lv)}
              tone={levelChipTone(lv)}
              label={levelLabel(lv)}
              count={severityCounts.get(lv) ?? 0}
              onToggle={() => toggleLevel(lv)}
            />
          {/each}
        </div>
      {/if}
    </header>

    <!-- Share confirm dialog. Non-destructive (uploads a copy) — keeps the
           amber .btn-warning CTA. -->
    {#if shareConfirm}
      <Modal
        ariaLabelledby="logs-share-title"
        onClose={() => (shareConfirm = false)}
        panelClass="bg-surface rounded shadow-xl p-6 max-w-sm w-full mx-4"
        closeOnBackdrop={!shareUploading}
        closeOnEscape={!shareUploading}
      >
        <h3 id="logs-share-title" class="font-semibold text-sm mb-2">
          {$t('logs.share.dialogTitle')}
        </h3>
        <p class="text-xs text-muted mb-4">
          {$t('logs.share.dialogBody')}
        </p>
        <div class="flex gap-2 justify-end">
          <button
            class="btn-secondary btn-xs"
            disabled={shareUploading}
            onclick={() => (shareConfirm = false)}
          >
            {$t('common.cancel')}
          </button>
          <BusyButton
            busy={shareUploading}
            class="btn-warning btn-xs"
            onclick={() => void doShare()}
          >
            {$t('logs.share.uploadBtn')}
          </BusyButton>
        </div>
      </Modal>
    {/if}

    <!-- Clear old logs confirm dialog. Destructive (deletes files) →
           .btn-danger per DESIGN.md. -->
    {#if clearOldOpen}
      <Modal
        ariaLabelledby="logs-clear-old-title"
        onClose={() => (clearOldOpen = false)}
        panelClass="bg-surface rounded shadow-xl p-6 max-w-sm w-full mx-4"
        dataTestid="clear-old-confirm"
        closeOnBackdrop={!clearingOld}
        closeOnEscape={!clearingOld}
      >
        <h3 id="logs-clear-old-title" class="font-semibold text-sm mb-2">
          {$t('logs.manage.clearOldTitle')}
        </h3>
        <p class="text-xs text-muted mb-4">
          {$t('logs.manage.clearOldBody', {
            count: clearOldStats.count,
            size: formatSize($t, clearOldStats.bytes),
          })}
        </p>
        <div class="flex gap-2 justify-end">
          <button
            class="btn-secondary btn-xs"
            disabled={clearingOld}
            onclick={() => (clearOldOpen = false)}
          >
            {$t('logs.manage.confirmNo')}
          </button>
          <BusyButton
            busy={clearingOld}
            class="btn-danger btn-xs"
            onclick={() => void confirmClearOld()}
          >
            {$t('logs.manage.clearOld')}
          </BusyButton>
        </div>
      </Modal>
    {/if}

    <!-- Delete single log confirm dialog. Destructive (deletes a file) →
           .btn-danger per DESIGN.md. -->
    {#if confirmingDeletePath}
      {@const path = confirmingDeletePath}
      {@const name = files.find((f) => f.path === path)?.name ?? ''}
      <Modal
        ariaLabelledby="logs-delete-title"
        onClose={() => (confirmingDeletePath = null)}
        panelClass="bg-surface rounded shadow-xl p-6 max-w-sm w-full mx-4"
        dataTestid="delete-log-confirm"
        closeOnBackdrop={!deletingPaths.has(path)}
        closeOnEscape={!deletingPaths.has(path)}
      >
        <h3 id="logs-delete-title" class="font-semibold text-sm mb-2">
          {$t('logs.manage.deleteConfirm')}
        </h3>
        <p class="font-mono text-xs text-muted mb-4 break-all">{name}</p>
        <div class="flex gap-2 justify-end">
          <button class="btn-secondary btn-xs" onclick={() => (confirmingDeletePath = null)}>
            {$t('logs.manage.confirmNo')}
          </button>
          <button
            class="btn-danger btn-xs"
            disabled={deletingPaths.has(path)}
            onclick={() => void deleteFile(path)}
          >
            {$t('logs.manage.confirmYes')}
          </button>
        </div>
      </Modal>
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
              <h3 class="px-3 pt-2 pb-0.5 text-xs font-semibold uppercase text-muted">
                {group.label}
              </h3>
              {#if group.hint}
                <p class="px-3 pb-1 text-[11px] leading-snug text-muted/80">{group.hint}</p>
              {/if}
              <ul>
                {#each group.items as f}
                  <li
                    class="group flex items-center hover:bg-subtle {selectedPath === f.path
                      ? 'bg-accent-soft'
                      : ''}"
                  >
                    <button
                      class="flex-1 min-w-0 text-left px-3 py-1"
                      onclick={() => void selectFile(f.path)}
                    >
                      <div class="font-mono text-xs truncate">{f.name}</div>
                      <div class="text-[10px] text-muted">
                        {formatSize($t, f.size_bytes)} · {formatMtime(f.modified_unix_ms)}
                      </div>
                    </button>
                    <button
                      class="btn-icon btn-icon-sm shrink-0 opacity-0 group-hover:opacity-100 group-has-[:focus-visible]:opacity-100"
                      aria-label={$t('logs.share.shareBtn')}
                      use:tooltip={$t('logs.share.shareBtn')}
                      disabled={shareUploading}
                      onclick={() => void shareFile(f.path)}
                    >
                      <Icon name="upload" size={14} />
                    </button>
                    <button
                      class="btn-icon btn-icon-sm shrink-0 opacity-0 group-hover:opacity-100 group-has-[:focus-visible]:opacity-100"
                      aria-label={$t('logs.toolbar.openFolder')}
                      use:tooltip={$t('logs.toolbar.openFolder')}
                      onclick={() => void openLogFolder(f.path)}
                    >
                      <Icon name="folderOpen" size={14} />
                    </button>
                    <button
                      class="btn-icon btn-icon-sm btn-icon-danger mr-1 shrink-0"
                      aria-label={$t('logs.manage.delete')}
                      use:tooltip={$t('logs.manage.delete')}
                      onclick={() => (confirmingDeletePath = f.path)}
                    >
                      <Icon name="trash" size={14} />
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
        <!-- Latest-log diagnosis banner: always bound to latest, independent of selected file -->
        <LogDiagnosisBanner {instanceId} {instanceName} {mcVersion} {loader} {gameRunning} />
        <!-- Search bar -->
        <div class="px-3 py-2 border-b flex items-center gap-2" data-tour-ctx="logs-search">
          <input
            class="flex-1 border rounded px-2 py-1 text-xs"
            placeholder={$t('logs.search.placeholder')}
            aria-label={$t('logs.search.placeholder')}
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
            <span class="text-xs text-muted whitespace-nowrap">{$t('logs.search.noMatches')}</span>
          {/if}
          <button
            class="btn-icon"
            disabled={totalMatches === 0}
            aria-label={$t('logs.search.prevAriaLabel')}
            use:tooltip={$t('logs.search.prevTitle')}
            onclick={prevMatch}
          >
            <Icon name="chevronUp" />
          </button>
          <button
            class="btn-icon"
            disabled={totalMatches === 0}
            aria-label={$t('logs.search.nextAriaLabel')}
            use:tooltip={$t('logs.search.nextTitle')}
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
              {$t('logs.empty.truncated', { cap: formatSize($t, capBytes) })}
            </div>
          {/if}

          <!-- Diagnosis card. When the open file IS the latest log, the top
                 banner already shows this diagnosis (title/explanation/fix), so
                 collapse to just the raw matched line to avoid printing it twice
                 (inlineRedundant); otherwise show the full card. -->
          {#if diagnosis}
            {#if inlineRedundant}
              {#if diagnosis.matched_excerpt}
                <div class="mx-3 mt-3 shrink-0" data-testid="log-diagnosis-inline-excerpt">
                  <p class="text-xs font-semibold text-warning-text select-none">
                    {$t('logs.diagnosis.matchedLine')}
                  </p>
                  <pre
                    class="mt-1 text-xs font-mono bg-surface p-2 rounded border border-warning-text/30 overflow-x-auto whitespace-pre-wrap selectable">{diagnosis.matched_excerpt}</pre>
                </div>
              {/if}
            {:else}
              {@const copy = DIAGNOSIS_COPY[diagnosis.pattern_id]}
              <details
                open
                class="mx-3 mt-3 border border-warning-text/30 bg-warning-bg rounded p-3 shrink-0"
              >
                <summary class="cursor-pointer font-semibold text-warning-text select-none">
                  <span class="flex items-center gap-1.5"
                    ><Icon name="warning" /> {copy ? $t(copy.title) : diagnosis.title}</span
                  >
                </summary>
                <p class="mt-2 text-sm text-warning-text selectable">
                  {copy ? $t(copy.explanation) : diagnosis.explanation}
                </p>
                <p class="mt-2 text-sm text-warning-text selectable">
                  <span class="font-semibold">{$t('logs.diagnosis.whatToTry')}</span>
                  {copy ? $t(copy.recommendation) : diagnosis.recommendation}
                </p>
                {#if diagnosis.matched_excerpt}
                  <pre
                    class="mt-2 text-xs font-mono bg-surface p-2 rounded border border-warning-text/30 overflow-x-auto whitespace-pre-wrap selectable">{diagnosis.matched_excerpt}</pre>
                {/if}
              </details>
            {/if}
          {/if}

          <!-- Log body: structured crash view or standard line view.
                 selectable so the user can drag-copy log lines / crash
                 stacks into a bug report or to mclo.gs. -->
          <div class="flex-1 overflow-auto font-mono text-xs leading-tight bg-base selectable">
            {#if isStructured && crashSections}
              <!-- Path A: structured crash report sections -->
              <div class="px-3 py-2 space-y-2">
                {#each crashSections as section, si}
                  {@const defaultOpen = isSectionDefaultOpen(section)}
                  {@const expanded = sectionExpanded.get(si) ?? defaultOpen}
                  {@const units = sectionRenderUnits(section.body, section.startLine)}
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
                          {@const unitKey = unitKeyFor(si, ui)}
                          {#if unit.kind === 'line'}
                            {@const hintId = annotations.get(unit.index)}
                            <!-- svelte-ignore a11y_no_static_element_interactions -- pointer
                                 handlers only arm/disarm the hover card; the keyboard-accessible
                                 path is the badge button below (focus/blur), same split
                                 LogHintCard.svelte uses. -->
                            <div
                              class={severityLineClass(unit.level, hintGutter)}
                              class:min-w-max={!wrap}
                              class:log-row-hint={hintId}
                              onpointerenter={hintId
                                ? (e) => activateHint(hintId, e.currentTarget as HTMLElement, false)
                                : undefined}
                              onpointerleave={hintId ? () => hints.rowLeave() : undefined}
                            >
                              {#if hintId}
                                <LogHintBadge
                                  onActivate={(el, viaFocus) => activateHint(hintId, el, viaFocus)}
                                  onLeave={() => hints.rowLeave()}
                                />
                              {/if}
                              <span
                                class={wrap ? 'whitespace-pre-wrap break-words' : 'whitespace-pre'}
                                >{@html highlightLine(unit.text, matchesForUnit(unitKey))}</span
                              >
                            </div>
                          {:else}
                            {@const foldKey = unitKey}
                            {@const isExpanded = foldExpanded.get(foldKey) ?? false}
                            <div
                              class={severityLineClass(unit.level, hintGutter)}
                              class:min-w-max={!wrap}
                            >
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
                  {@const unitKey = unitKeyFor(-1, ui)}
                  {#if unit.kind === 'line'}
                    {@const hintId = annotations.get(unit.index)}
                    <!-- svelte-ignore a11y_no_static_element_interactions -- pointer handlers
                         only arm/disarm the hover card; the keyboard-accessible path is the
                         badge button below (focus/blur), same split LogHintCard.svelte uses. -->
                    <div
                      class="log-row {severityLineClass(unit.level, hintGutter)}"
                      class:min-w-max={!wrap}
                      class:log-row-hint={hintId}
                      onpointerenter={hintId
                        ? (e) => activateHint(hintId, e.currentTarget as HTMLElement, false)
                        : undefined}
                      onpointerleave={hintId ? () => hints.rowLeave() : undefined}
                    >
                      {#if hintId}
                        <LogHintBadge
                          onActivate={(el, viaFocus) => activateHint(hintId, el, viaFocus)}
                          onLeave={() => hints.rowLeave()}
                        />
                      {/if}
                      <span class="text-placeholder select-none"
                        >{(ui + 1).toString().padStart(6, ' ')}:
                      </span><span
                        class={wrap ? 'whitespace-pre-wrap break-words' : 'whitespace-pre'}
                        >{@html highlightLine(unit.text, matchesForUnit(unitKey))}</span
                      >
                    </div>
                  {:else}
                    {@const isExpanded = foldExpanded.get(ui) ?? false}
                    <div
                      class="log-row {severityLineClass(unit.level, hintGutter)}"
                      class:min-w-max={!wrap}
                    >
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
    <LogHintCard hover={hints} />
  </Modal>
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

  /* Subtle left-edge tint marking a line with an inline hint annotation.
     Uses the same `--accent` RGB triplet the rest of the design system reads
     through Tailwind's `accent`/`accent-soft` utilities (see app.css).
     The badge's own styles live in LogHintBadge.svelte (scoped styles here
     could not reach a child component's internals). */
  .log-row-hint {
    background-image: linear-gradient(to right, rgb(var(--accent) / 12%), transparent 60%);
  }
</style>
