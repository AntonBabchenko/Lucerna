<script lang="ts">
  import { onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { AnnotateResult, ServerLogInfo } from '$lib/ipc/bindings';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import type { SelectOption } from '$lib/ui/Select.svelte';
  import ToggleChip from '$lib/ui/ToggleChip.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import Spinner from '$lib/ui/Spinner.svelte';
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
    countLevels,
    filterLevels,
    findMatches,
    highlightConsoleLine,
    levelChipTone,
    presentLevels,
    tagConsoleLines,
    type Severity,
  } from '$lib/servers/console-filter';

  // The live `server-latest.log` filename (matches serverlog::LATEST on the backend).
  const LATEST_LOG = 'server-latest.log';

  let { serverId }: { serverId: string } = $props();

  // ---------------------------------------------------------------------------
  // Live-console state
  // ---------------------------------------------------------------------------
  const lines = $derived(serverState.lines(serverId));
  const running = $derived(serverState.running(serverId));

  let draft = $state('');
  let busy = $state(false);
  let sendError = $state<string | null>(null);
  let container = $state<HTMLDivElement | null>(null);
  // Tail of `server-latest.log`, shown in the live view when we have NO captured
  // output (panel reopened, or the launcher was restarted while the server kept
  // running so its stdout isn't being streamed). Live events take over once they
  // arrive.
  let backfillText = $state<string | null>(null);

  $effect(() => {
    // Read lines.length / backfillText to re-run whenever new output appears.
    void lines.length;
    void backfillText;
    // Don't fight the user: only stick to the bottom on the live view and when
    // not actively navigating search matches (which scrolls to the hit instead).
    if (container && archivedText === null && !debouncedSearch) {
      container.scrollTop = container.scrollHeight;
    }
  });

  // Load the backfill once per server while the live buffer is empty + running.
  $effect(() => {
    const hasLive = serverState.lines(serverId).length > 0;
    const isRunning = serverState.running(serverId);
    if (isRunning && !hasLive && backfillText === null) {
      void commands.serverReadLog(serverId, LATEST_LOG).then((r) => {
        if (r.status === 'ok') backfillText = r.data;
      });
    }
  });

  async function send() {
    const cmd = draft.trim();
    if (!cmd || busy || !running) return;
    busy = true;
    sendError = null;
    try {
      const res = await commands.serverSendCommand(serverId, cmd);
      if (res.status === 'ok') {
        draft = '';
      } else {
        sendError = formatError(res.error);
      }
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void send();
    }
  }

  // ---------------------------------------------------------------------------
  // Past-sessions log picker state
  // ---------------------------------------------------------------------------
  let logs = $state<ServerLogInfo[]>([]);
  let selectedFile = $state<string | null>(null);
  let archivedText = $state<string | null>(null);
  let readError = $state<string | null>(null);
  let loadingText = $state(false);
  let prevRunning: boolean | null = null;

  const LIVE_VALUE = '__live__';

  async function loadLogs(): Promise<void> {
    const r = await serverState.listLogs(serverId);
    if (r.ok && r.list) {
      logs = r.list;
    }
  }

  // Load on mount.
  $effect(() => {
    void loadLogs();
  });

  // Reload when the server transitions from running → stopped (a new archive was just created).
  $effect(() => {
    const cur = serverState.running(serverId);
    if (prevRunning === true && !cur) void loadLogs();
    prevRunning = cur;
  });

  // Reset archive view when serverId changes so stale content can't linger.
  $effect(() => {
    void serverId;
    selectedFile = null;
    archivedText = null;
    readError = null;
    backfillText = null;
    annotateResult = null;
    hints.close();
  });

  // Build picker options: "Current session" + one entry per archive.
  const pickerOptions = $derived<SelectOption[]>([
    { value: LIVE_VALUE, label: $t('servers.logs.latest') },
    ...logs.filter((l) => !l.is_latest).map((l) => ({ value: l.file_name, label: l.file_name })),
  ]);

  async function onPickerChange(value: string | number): Promise<void> {
    const file = String(value);
    readError = null;

    if (file === LIVE_VALUE) {
      selectedFile = null;
      archivedText = null;
      return;
    }

    selectedFile = file;
    archivedText = null;
    loadingText = true;
    try {
      const r = await commands.serverReadLog(serverId, selectedFile);
      if (r.status === 'ok') {
        archivedText = r.data;
      } else {
        readError = formatError(r.error);
      }
    } finally {
      loadingText = false;
    }
  }

  function backToLive(): void {
    selectedFile = null;
    archivedText = null;
    readError = null;
  }

  // ---------------------------------------------------------------------------
  // Level filter + in-log search (#14) — applied to the live stream AND archives.
  // ---------------------------------------------------------------------------
  let hiddenLevels = $state<Set<Severity>>(new Set());
  let search = $state('');
  // Debounced mirror of `search`: the input stays responsive while the O(N)
  // match scan + highlight key off the debounced value (jank-free on long logs).
  let debouncedSearch = $state('');
  const SEARCH_DEBOUNCE_MS = 120;
  let currentMatchIndex = $state(0);

  // The raw lines backing the CURRENT view (archive blob, live buffer, or the
  // saved-log backfill), before filtering.
  const viewLines = $derived.by<string[]>(() => {
    if (archivedText !== null) return archivedText.length > 0 ? archivedText.split('\n') : [];
    if (lines.length > 0) return lines;
    if (backfillText && backfillText.length > 0) return backfillText.split('\n');
    return [];
  });
  // True when the live view is falling back to the saved log (dim styling).
  const showingBackfill = $derived(archivedText === null && lines.length === 0);

  const tagged = $derived(tagConsoleLines(viewLines));
  const levelCounts = $derived(countLevels(tagged));
  const present = $derived(presentLevels(tagged));
  const visible = $derived(filterLevels(tagged, hiddenLevels));
  const matches = $derived(
    findMatches(
      visible.map((l) => l.text),
      debouncedSearch,
    ),
  );
  const totalMatches = $derived(matches.length);

  function toggleLevel(lv: Severity): void {
    const next = new Set(hiddenLevels);
    if (next.has(lv)) next.delete(lv);
    else next.add(lv);
    hiddenLevels = next;
  }

  // Trailing-debounce search → debouncedSearch.
  $effect(() => {
    const s = search;
    const id = setTimeout(() => {
      debouncedSearch = s;
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(id);
  });

  // Reset the active match when the committed query or the underlying view changes.
  $effect(() => {
    void debouncedSearch;
    void viewLines.length;
    void selectedFile;
    currentMatchIndex = 0;
  });

  // Keep the active match in range when the match set shrinks (e.g. toggling a
  // level chip off mid-search) so the counter never shows an impossible "9/3"
  // and the highlight stays on a real match. Clamps to the last match rather
  // than snapping to the first, preserving the user's position.
  $effect(() => {
    if (currentMatchIndex >= totalMatches) {
      currentMatchIndex = totalMatches === 0 ? 0 : totalMatches - 1;
    }
  });

  function scrollToCurrentMatch(): void {
    requestAnimationFrame(() => {
      const el = container?.querySelector('[data-match-active="true"]');
      // Guard scrollIntoView — not implemented in every test DOM (happy-dom).
      if (el && typeof el.scrollIntoView === 'function') {
        const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
        el.scrollIntoView({ block: 'center', behavior: reduce ? 'auto' : 'smooth' });
      }
    });
  }

  function nextMatch(): void {
    if (totalMatches === 0) return;
    currentMatchIndex = (currentMatchIndex + 1) % totalMatches;
    scrollToCurrentMatch();
  }

  function prevMatch(): void {
    if (totalMatches === 0) return;
    currentMatchIndex = (currentMatchIndex - 1 + totalMatches) % totalMatches;
    scrollToCurrentMatch();
  }

  function onSearchKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      search = '';
      debouncedSearch = ''; // flush so highlights/counter clear at once
    } else if (e.key === 'Enter') {
      debouncedSearch = search; // flush so Enter navigates exactly what's typed
      if (e.shiftKey) prevMatch();
      else nextMatch();
      e.preventDefault();
    }
  }

  // ---------------------------------------------------------------------------
  // Inline hint annotations — debounced re-annotation of whatever lines the
  // view currently shows (live buffer ≤500 lines, archive blob, or backfill).
  // 500 ms trailing: the live console appends every tick; per-append IPC would
  // be waste. Best-effort — failures degrade to "no badges".
  // ---------------------------------------------------------------------------
  let annotateResult = $state<AnnotateResult | null>(null);
  const annotations = $derived(annotationMap(annotateResult));
  const hintFallbacks = $derived(fallbackCopyMap(annotateResult));
  // When the current view has any annotations, every row reserves a uniform
  // left gutter and the badge sits absolutely inside it — annotated lines'
  // text must not shift out of column alignment.
  const hintGutter = $derived(annotations.size > 0);
  const hints = createLogHintHover();
  onDestroy(() => hints.dispose());

  const ANNOTATE_DEBOUNCE_MS = 500;
  $effect(() => {
    const text = viewLines.join('\n');
    if (!text) {
      annotateResult = null;
      return;
    }
    const id = setTimeout(() => {
      void commands.annotateLogText(text, 'server').then((r) => {
        if (r.status !== 'ok') {
          // biome-ignore lint/suspicious/noConsole: best-effort UI degradation when IPC fails
          console.warn('[ServerConsole] annotate_log_text failed:', r.error);
          return;
        }
        // Stale guard: the debounced result may land after `viewLines` moved on
        // (e.g. archive → live switch mid-flight) — only commit if the view
        // still shows the same text.
        if (viewLines.join('\n') === text) annotateResult = r.data;
      });
    }, ANNOTATE_DEBOUNCE_MS);
    return () => clearTimeout(id);
  });

  function activateHint(patternId: string, el: HTMLElement, viaFocus: boolean): void {
    const copy = resolveHintCopy(patternId, hintFallbacks, get(t));
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

  // ---------------------------------------------------------------------------
  // Share to mclo.gs (reuses the instance share command — content is anonymised
  // server-side before upload). Shares the archive being viewed, or the latest
  // session when on the live console.
  // ---------------------------------------------------------------------------
  let busyShare = $state(false);
  let shareUrl = $state<string | null>(null);
  let shareError = $state<string | null>(null);

  async function shareLog(): Promise<void> {
    busyShare = true;
    shareError = null;
    shareUrl = null;
    try {
      // Prefer the already-loaded archive text; otherwise read the latest log.
      let content = archivedText;
      if (content === null) {
        const r = await commands.serverReadLog(serverId, LATEST_LOG);
        if (r.status !== 'ok') {
          shareError = formatError(r.error);
          return;
        }
        content = r.data;
      }
      if (!content || content.length === 0) {
        shareError = get(t)('servers.logs.shareEmpty');
        return;
      }
      const res = await commands.shareLogToMclogs(content);
      if (res.status === 'ok') {
        shareUrl = res.data;
        // Best-effort copy; the URL is also shown for manual copy.
        try {
          await navigator.clipboard?.writeText(res.data);
        } catch {
          // clipboard unavailable — the URL is still displayed below
        }
        pushSuccess(get(t)('servers.logs.shareCopied'));
      } else {
        shareError = formatError(res.error);
      }
    } finally {
      busyShare = false;
    }
  }

  const archives = $derived(logs.filter((l) => !l.is_latest));

  async function openLogsFolder() {
    const r = await serverState.openLogsFolder(serverId);
    if (!r.ok) {
      pushWarning(get(t)('servers.logs.openFolder'), [formatError(r.error)]);
    }
  }
</script>

{#snippet lineList(dim: boolean)}
  {#each visible as line, i (i)}
    {@const hintId = annotations.get(line.index)}
    <!-- svelte-ignore a11y_no_static_element_interactions -- pointer handlers only arm/disarm
         the hover card; the keyboard-accessible path is the badge button below (focus/blur),
         same split LogHintCard.svelte uses. -->
    <div
      class="whitespace-pre-wrap break-all leading-5 {dim ? 'text-secondary' : ''}"
      class:relative={hintGutter}
      class:pl-5={hintGutter}
      class:console-row-hint={hintId}
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
      {#if debouncedSearch}
        <!-- highlightConsoleLine HTML-escapes the text; only its own <mark> wrappers are raw. -->
        {@html highlightConsoleLine(line.text, i, matches, currentMatchIndex)}
      {:else}
        {line.text}
      {/if}
    </div>
  {/each}
{/snippet}

<div class="flex flex-col gap-2">
  <!-- Log controls row: open-folder + share + past-sessions picker -->
  <div class="flex items-center gap-2">
    <button
      type="button"
      class="btn-secondary btn-sm shrink-0 inline-flex items-center gap-1"
      onclick={openLogsFolder}
    >
      <Icon name="folderOpen" size={14} />{$t('servers.logs.openFolder')}
    </button>

    <BusyButton
      class="btn-ghost btn-sm shrink-0"
      data-testid="server-log-share"
      busy={busyShare}
      onclick={() => void shareLog()}
    >
      {$t('servers.logs.share')}
    </BusyButton>

    {#if archives.length > 0}
      <div class="flex-1">
        <Select
          value={selectedFile ?? LIVE_VALUE}
          options={pickerOptions}
          onChange={onPickerChange}
          ariaLabel={$t('servers.logs.pastSessions')}
        />
      </div>
    {/if}
  </div>

  {#if shareError}
    <p class="text-xs text-danger" role="alert" data-testid="server-log-share-error">
      {shareError}
    </p>
  {/if}
  {#if shareUrl}
    <p class="text-xs text-secondary" data-testid="server-log-share-url">
      {$t('servers.logs.sharedAt')}
      <span class="select-all break-all font-mono text-primary">{shareUrl}</span>
    </p>
  {/if}

  <!-- Filter + search toolbar: only shown when there is output to work on. -->
  {#if viewLines.length > 0}
    <div class="flex flex-wrap items-center gap-2" data-testid="server-console-filterbar">
      <div class="flex min-w-[12rem] flex-1 items-center gap-1">
        <input
          type="text"
          class="filter-control flex-1"
          placeholder={$t('servers.console.searchPlaceholder')}
          bind:value={search}
          onkeydown={onSearchKeydown}
          data-testid="server-console-search"
        />
        {#if debouncedSearch && totalMatches > 0}
          <span class="whitespace-nowrap text-xs text-muted">
            {$t('servers.console.matchCounter', {
              current: currentMatchIndex + 1,
              total: totalMatches,
            })}
          </span>
        {:else if debouncedSearch}
          <span
            class="whitespace-nowrap text-xs text-muted"
            data-testid="server-console-no-matches"
          >
            {$t('servers.console.noMatches')}
          </span>
        {/if}
        <button
          type="button"
          class="btn-icon btn-icon-sm"
          disabled={totalMatches === 0}
          aria-label={$t('servers.console.searchPrev')}
          use:tooltip={$t('servers.console.searchPrev')}
          onclick={prevMatch}
        >
          <Icon name="chevronUp" size={14} />
        </button>
        <button
          type="button"
          class="btn-icon btn-icon-sm"
          disabled={totalMatches === 0}
          aria-label={$t('servers.console.searchNext')}
          use:tooltip={$t('servers.console.searchNext')}
          onclick={nextMatch}
        >
          <Icon name="chevronDown" size={14} />
        </button>
      </div>

      {#if present.length > 1}
        <div
          class="flex flex-wrap items-center gap-1"
          role="group"
          aria-label={$t('servers.console.showLevels')}
        >
          {#each present as lv (lv)}
            <ToggleChip
              active={!hiddenLevels.has(lv)}
              tone={levelChipTone(lv)}
              label={lv.toUpperCase()}
              count={levelCounts.get(lv) ?? 0}
              onToggle={() => toggleLevel(lv)}
              testId="server-console-level-{lv}"
            />
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Console body: live stream OR read-only archive viewer -->
  {#if selectedFile !== null}
    <!-- Read-only archive viewer -->
    <p class="text-xs text-muted">{$t('servers.logs.viewing', { name: selectedFile })}</p>

    <div
      bind:this={container}
      class="h-80 overflow-y-auto rounded border border-border-subtle bg-base p-2 font-mono text-xs"
    >
      {#if loadingText}
        <Spinner size="sm" labelPlacement="right" label={$t('common.loading')} class="text-muted" />
      {:else if readError}
        <span class="text-danger">{readError}</span>
      {:else if archivedText !== null && archivedText.length === 0}
        <span class="text-muted">{$t('servers.logs.noLogs')}</span>
      {:else if visible.length > 0}
        {@render lineList(false)}
      {:else if viewLines.length > 0}
        <span class="text-muted">{$t('servers.console.noMatchingLines')}</span>
      {/if}
    </div>

    <div class="flex items-center gap-2">
      <button type="button" class="btn-ghost btn-sm" onclick={backToLive}>
        {$t('servers.logs.backToLive')}
      </button>
    </div>
  {:else}
    <!-- Live console -->
    <div
      bind:this={container}
      class="h-80 overflow-y-auto rounded border border-border-subtle bg-base p-2 font-mono text-xs"
    >
      {#if visible.length > 0}
        {@render lineList(showingBackfill)}
      {:else if viewLines.length > 0}
        <span class="text-muted">{$t('servers.console.noMatchingLines')}</span>
      {:else}
        <span class="text-muted">{$t('servers.console.empty')}</span>
      {/if}
    </div>

    {#if !running}
      <p class="text-xs text-muted">{$t('servers.console.notRunning')}</p>
    {/if}

    <!-- Console-local: a failed chat/command send, not a launch failure the
         diagnosis banner can classify, so it stays inline here by design. -->
    {#if sendError}
      <p class="text-xs text-danger" role="alert" data-testid="server-send-error">{sendError}</p>
    {/if}

    <div class="flex gap-2">
      <input
        type="text"
        class="h-8 flex-1 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary disabled:opacity-50"
        placeholder={$t('servers.console.inputPlaceholder')}
        bind:value={draft}
        disabled={!running || busy}
        onkeydown={onKeydown}
      />
      <button
        type="button"
        class="btn-primary btn-sm"
        disabled={!running || busy || draft.trim().length === 0}
        onclick={() => void send()}
      >
        {$t('servers.console.send')}
      </button>
    </div>
  {/if}
  <LogHintCard hover={hints} />
</div>

<style>
  /* Subtle left-edge tint marking a line with an inline hint annotation.
     Uses the same `--accent` RGB triplet the rest of the design system reads
     through Tailwind's `accent`/`accent-soft` utilities (see app.css).
     The badge's own styles live in LogHintBadge.svelte (scoped styles here
     could not reach a child component's internals). */
  .console-row-hint {
    background-image: linear-gradient(to right, rgb(var(--accent) / 12%), transparent 60%);
  }
</style>
