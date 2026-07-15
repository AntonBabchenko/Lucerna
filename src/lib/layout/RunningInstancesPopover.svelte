<script lang="ts">
  // Aggregate running-instances indicator: a small "▶ N" pill (this component's
  // trigger) that opens a popover listing every currently-running instance with
  // a live elapsed-playtime readout, a Stop button, and an Open (jump) action.
  //
  // The pill is always visible in BOTH launcher modes (Client and Servers)
  // because the ModeSwitcher that hosts it is persistent — it only renders when
  // there is at least one running instance (the parent guards on runningCount).
  //
  // Client game state lives page-locally in +page.svelte (NOT a rune module),
  // so this component receives everything it needs by prop: the reactive count,
  // an id→name resolver, and the jump callback. It fetches the authoritative
  // per-instance list from the backend lazily on open (keeping that work off the
  // closed path) and refreshes it on spawn/exit events while open, so Stop and
  // new launches update the list live.
  //
  // Positioning mirrors HelpPopover: a `position: fixed` popover measured from
  // the trigger on open (an `overflow-y-auto` host like the sidebar would clip
  // an absolute one), elevated with the shared `--z-popover` and dismissed via
  // the shared attachPopoverDismiss helper (scroll/resize) plus Escape and an
  // outside-click backdrop.
  import { commands, events, type RunningInstanceInfo } from '$lib/ipc/bindings';
  import { formatDuration } from '$lib/format/duration';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { attachPopoverDismiss } from '$lib/ui/popover-dismiss';
  import { SvelteSet } from 'svelte/reactivity';

  let {
    runningCount,
    instanceName,
    onOpenInstance,
  }: {
    // Reactive number of running instances (the page's `running.size`). Drives
    // the pill's count; the parent only mounts this when it is > 0.
    runningCount: number;
    // Resolve an instance's display name by id (page-local instances list).
    instanceName: (id: string) => string;
    // Jump to an instance: select it and switch to Client mode. Closes the
    // popover. Implemented in +page.svelte.
    onOpenInstance: (id: string) => void;
  } = $props();

  const GAP = 4;
  const MARGIN = 8;
  const WIDTH = 288;
  const popoverId = `running-instances-popover-${crypto.randomUUID()}`;

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined;
  let listEl: HTMLElement | undefined = $state();
  let popoverTop = $state(0);
  let popoverLeft = $state(0);

  let rows = $state<RunningInstanceInfo[]>([]);
  // Ids currently being stopped, so the Stop button disarms until the exit
  // event removes the row (avoids a double-stop and gives immediate feedback).
  let stopping = $state(new SvelteSet<string>());
  // Ticks once per second while open to re-evaluate the live elapsed labels.
  let now = $state(Date.now());

  function positionPopover() {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    popoverTop = r.bottom + GAP;
    const maxLeft = window.innerWidth - WIDTH - MARGIN;
    // Right-align the popover to the pill when possible (the pill sits near the
    // right edge of the switcher), but never past the viewport margins.
    const preferred = r.right - WIDTH;
    popoverLeft = Math.min(Math.max(preferred, MARGIN), Math.max(MARGIN, maxLeft));
  }

  async function refresh() {
    rows = await commands.runningInstances();
    // Prune the stopping-set: a row that's gone (stopped, or exited) must not
    // keep a stale entry, or a relaunched instance would get a permanently
    // -disabled Stop button (the set otherwise only resets on full unmount, i.e.
    // count→0). Iterate a snapshot since we delete during iteration.
    for (const id of [...stopping]) {
      if (!rows.some((r) => r.instance_id === id)) stopping.delete(id);
    }
    // Race guard: if everything exited between open and this resolve, there is
    // nothing to show — close. (The parent also unmounts us once the count
    // reaches zero; this covers the in-flight window.)
    if (rows.length === 0) open = false;
  }

  function toggle() {
    if (open) {
      open = false;
    } else {
      positionPopover();
      now = Date.now();
      open = true;
      void refresh();
    }
  }

  async function stop(id: string) {
    if (stopping.has(id)) return;
    stopping.add(id);
    const result = await commands.stopInstance(id);
    if (result.status === 'error') {
      stopping.delete(id);
      pushWarning($t('sidebar.stop'), [formatError(result.error)]);
      return;
    }
    // Leave the id in `stopping` — the exit event refreshes the list and the
    // row disappears; the SvelteSet is discarded on close either way.
  }

  function jump(id: string) {
    open = false;
    onOpenInstance(id);
  }

  function elapsedLabel(startedMs: number | null): string {
    if (startedMs == null) return '—';
    const secs = Math.max(0, Math.floor((now - startedMs) / 1000));
    return formatDuration($t, secs);
  }

  // While open: keep the list fresh on spawn/exit, tick the live playtime once
  // a second, and wire the shared dismiss behaviour (scroll/resize) plus an
  // Escape handler that also refocuses the trigger (keyboard dismiss).
  $effect(() => {
    if (!open) return;

    let disposed = false;
    const unlisteners: Array<() => void> = [];
    void (async () => {
      const uSpawn = await events.processSpawned.listen(() => void refresh());
      const uExit = await events.processExited.listen(() => void refresh());
      if (disposed) {
        uSpawn();
        uExit();
        return;
      }
      unlisteners.push(uSpawn, uExit);
    })();

    const interval = setInterval(() => {
      now = Date.now();
    }, 1000);

    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        open = false;
        trigger?.focus();
      }
    };
    window.addEventListener('keydown', onKeydown);
    // A scroll inside the internally-scrollable list must not dismiss.
    const detach = attachPopoverDismiss({
      onDismiss: () => (open = false),
      ignoreScrollWithin: () => listEl,
    });

    return () => {
      disposed = true;
      for (const u of unlisteners) u();
      clearInterval(interval);
      window.removeEventListener('keydown', onKeydown);
      detach();
    };
  });
</script>

<div class="relative inline-flex">
  <button
    bind:this={trigger}
    type="button"
    class="relative inline-flex items-center gap-1 rounded-full bg-black/20 px-2 h-7 text-xs font-semibold text-secondary hover:text-primary transition-colors"
    class:z-50={open}
    aria-label={$t('running.pill.label', { count: runningCount })}
    aria-expanded={open}
    aria-controls={popoverId}
    data-testid="running-instances-pill"
    onclick={toggle}
  >
    <Icon name="play" size={10} class="text-success" />
    {runningCount}
  </button>

  {#if open}
    <!-- Click-outside backdrop -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div role="presentation" class="fixed inset-0 z-30" onclick={() => (open = false)}></div>
    <div
      id={popoverId}
      class="fixed z-[var(--z-popover)] bg-surface border border-border-subtle rounded shadow-md"
      style="top: {popoverTop}px; left: {popoverLeft}px; width: {WIDTH}px;"
      data-testid="running-instances-popover"
    >
      <div class="flex items-center justify-between px-3 py-2 border-b border-border-subtle">
        <h2 class="text-xs font-semibold text-primary">{$t('running.popover.title')}</h2>
        <CloseButton onClick={() => (open = false)} ariaLabel={$t('common.close')} />
      </div>
      <ul bind:this={listEl} class="max-h-64 overflow-y-auto py-1">
        {#each rows as row (row.instance_id)}
          <li class="flex items-center gap-2 px-3 py-1.5">
            <div class="min-w-0 flex-1">
              <div class="truncate text-sm text-primary">{instanceName(row.instance_id)}</div>
              <div class="text-[11px] text-muted tabular-nums">
                {elapsedLabel(row.started_unix_ms)}
              </div>
            </div>
            <button
              type="button"
              class="btn-secondary btn-xs flex items-center gap-1"
              data-testid="running-instances-open-{row.instance_id}"
              onclick={() => jump(row.instance_id)}
            >
              <Icon name="monitor" size={12} />
              {$t('running.popover.open')}
            </button>
            <button
              type="button"
              class="btn-danger btn-xs flex items-center gap-1"
              disabled={stopping.has(row.instance_id)}
              data-testid="running-instances-stop-{row.instance_id}"
              onclick={() => void stop(row.instance_id)}
            >
              <Icon name="stop" size={12} />
              {$t('sidebar.stop')}
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>
