<script lang="ts">
  // Aggregate running-servers indicator: a small "▶ N" pill (this component's
  // trigger) that opens a popover listing every currently-running server with a
  // Stop, Restart, and Open (jump) action. The Servers-mode mirror of
  // RunningInstancesPopover.
  //
  // The pill is always visible in BOTH launcher modes because the ModeSwitcher
  // that hosts it is persistent — it only renders when at least one server runs
  // (the parent guards on runningCount).
  //
  // Unlike the client popover, server runtime state lives in the `serverState`
  // rune module (not page-local), which keeps itself fresh via its own
  // spawn/exit listeners. So the rows come straight from `serverState.list`
  // (reactive) — no lazy backend fetch and no event listeners here — and the
  // per-server busy state is the store's own `actionFor(id)` (shared with the
  // sidebar Start/Stop and the panel Restart), not a local disarm set: reusing
  // it keeps every surface's busy state in lockstep and, because Restart leaves
  // the server running, avoids the "row never leaves the set" permanent-disable
  // a local set would hit on restart.
  //
  // ServerWithStatus carries no process-start timestamp (only config
  // created_unix_ms), so there is no live uptime to show — the sub-label is the
  // uniform "running" status (+ port) instead.
  //
  // Positioning mirrors RunningInstancesPopover / HelpPopover exactly: a
  // `position: fixed` popover measured from the trigger on open (an
  // `overflow-y-auto` host like the sidebar would clip an absolute one),
  // elevated with the shared `--z-popover` and dismissed via the shared
  // attachPopoverDismiss helper (scroll/resize) plus Escape and an
  // outside-click backdrop.
  import { describeStoreError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { attachPopoverDismiss } from '$lib/ui/popover-dismiss';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    runningCount,
    compact = false,
  }: {
    // Reactive number of running servers. Drives the pill's count; the parent
    // only mounts this when it is > 0 (mirrors the client pill's guard).
    runningCount: number;
    // Compact trigger — a tiny superscript count badge (variant F2) pinned over
    // the ModeSwitcher Servers segment's status icon, instead of the standalone
    // h-7 pill.
    compact?: boolean;
  } = $props();

  const GAP = 4;
  const MARGIN = 8;
  const WIDTH = 288;
  const popoverId = `running-servers-popover-${crypto.randomUUID()}`;

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined;
  let listEl: HTMLElement | undefined = $state();
  let popoverTop = $state(0);
  let popoverLeft = $state(0);
  // Effective popover width — shrunk to fit narrow (compact/mini) windows so a
  // fixed 288px popover never overflows the viewport. Computed on open.
  let popWidth = $state(WIDTH);

  // Rows straight from the store — already reactive, kept fresh by the module's
  // own spawn/exit listeners.
  const rows = $derived(serverState.list.filter((s) => s.running));

  function positionPopover() {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    popoverTop = r.bottom + GAP;
    // Never wider than the viewport minus both margins — in compact/mini mode
    // the window shrinks to hug the sidebar, so the full 288px would overflow.
    popWidth = Math.min(WIDTH, window.innerWidth - 2 * MARGIN);
    const maxLeft = window.innerWidth - popWidth - MARGIN;
    // Left-align to the trigger (it sits on the LEFT, in the sidebar), so the
    // popover opens rightward/below — but never past the viewport margins
    // (mirrors RunningInstancesPopover exactly).
    const preferred = r.left;
    popoverLeft = Math.min(Math.max(preferred, MARGIN), Math.max(MARGIN, maxLeft));
  }

  function toggle() {
    if (open) {
      open = false;
    } else {
      positionPopover();
      open = true;
    }
  }

  async function act(id: string, action: 'stop' | 'restart') {
    // Re-entrancy guard: an already-in-flight action is a silent no-op (mirrors
    // the client popover's `if (stopping.has(id)) return`). Without it a rapid
    // double-click re-enters here, runLifecycle's own concurrency guard returns
    // `{ ok: false }` WITHOUT recording an error, and the block below would push
    // a spurious "Stop: null" toast (String(actionErrorFor(id)) === 'null').
    if (serverState.actionFor(id) !== null) return;
    // runLifecycle guards concurrent actions and records the error in the store;
    // surface a failure here like the client popover (the row stays for restart,
    // disappears for a successful stop).
    const r = action === 'stop' ? await serverState.stop(id) : await serverState.restart(id);
    if (!r.ok) {
      pushWarning(action === 'stop' ? $t('servers.action.stop') : $t('servers.action.restart'), [
        describeStoreError(serverState.actionErrorFor(id)),
      ]);
    }
  }

  function jump(id: string) {
    open = false;
    serversUi.setMode('servers');
    serversUi.selectServer(id);
  }

  // Defensive: if the last running server exits while open, close (the parent
  // also unmounts us once runningCount reaches zero — this covers the in-flight
  // frame). rows is derived, so a mid-list stop just drops that row live.
  $effect(() => {
    if (open && rows.length === 0) open = false;
  });

  // While open: wire the shared dismiss behaviour (scroll/resize) plus an
  // Escape handler that also refocuses the trigger (keyboard dismiss). No event
  // listeners / tick — serverState is already reactive and there is no uptime.
  $effect(() => {
    if (!open) return;

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
      window.removeEventListener('keydown', onKeydown);
      detach();
    };
  });
</script>

<div class="relative inline-flex">
  <button
    bind:this={trigger}
    type="button"
    class={compact
      ? 'flex items-center justify-center rounded-full bg-success px-1 text-[10px] font-semibold leading-none text-white h-[15px] min-w-[15px]'
      : 'relative inline-flex items-center gap-1 rounded-full bg-black/20 px-2 h-7 text-xs font-semibold text-secondary transition-colors hover:text-primary'}
    class:z-50={open}
    aria-label={$t('running.pill.label', { count: runningCount })}
    aria-expanded={open}
    aria-controls={popoverId}
    data-testid="running-servers-pill"
    onclick={toggle}
  >
    {#if !compact}
      <Icon name="play" size={10} class="text-success" />
    {/if}
    {runningCount}
  </button>

  {#if open}
    <!-- Click-outside backdrop -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div role="presentation" class="fixed inset-0 z-30" onclick={() => (open = false)}></div>
    <div
      id={popoverId}
      class="fixed z-[var(--z-popover)] bg-surface border border-border-subtle rounded shadow-md"
      style="top: {popoverTop}px; left: {popoverLeft}px; width: {popWidth}px;"
      data-testid="running-servers-popover"
    >
      <div class="flex items-center justify-between px-3 py-2 border-b border-border-subtle">
        <h2 class="text-xs font-semibold text-primary">{$t('running.popover.serversTitle')}</h2>
        <CloseButton onClick={() => (open = false)} ariaLabel={$t('common.close')} />
      </div>
      <ul bind:this={listEl} class="max-h-64 overflow-y-auto py-1">
        {#each rows as row (row.id)}
          <li class="flex items-center gap-2 px-3 py-1.5">
            <div class="min-w-0 flex-1">
              <div class="truncate text-sm text-primary">{row.name}</div>
              <div class="text-[11px] text-muted">
                {$t('servers.status.running')}{row.port ? ' · ' + row.port : ''}
              </div>
            </div>
            <!-- Icon-only actions: three labelled buttons (Open/Restart/Stop)
                 would overflow the mirrored 288px width in RU (Перезапустить /
                 Остановить / Открыть). aria-label + tooltip reuse the same
                 strings, so the row stays compact and locale-robust. -->
            <button
              type="button"
              class="btn-secondary btn-xs flex items-center"
              aria-label={$t('running.popover.open')}
              use:tooltip={{ text: $t('running.popover.open'), describe: false }}
              data-testid="running-servers-open-{row.id}"
              onclick={() => jump(row.id)}
            >
              <Icon name="server" size={12} />
            </button>
            <button
              type="button"
              class="btn-secondary btn-xs flex items-center"
              disabled={serverState.actionFor(row.id) !== null}
              aria-label={$t('servers.action.restart')}
              use:tooltip={{ text: $t('servers.action.restart'), describe: false }}
              data-testid="running-servers-restart-{row.id}"
              onclick={() => void act(row.id, 'restart')}
            >
              <Icon name="refresh" size={12} />
            </button>
            <button
              type="button"
              class="btn-danger btn-xs flex items-center"
              disabled={serverState.actionFor(row.id) !== null}
              aria-label={$t('servers.action.stop')}
              use:tooltip={{ text: $t('servers.action.stop'), describe: false }}
              data-testid="running-servers-stop-{row.id}"
              onclick={() => void act(row.id, 'stop')}
            >
              <Icon name="stop" size={12} />
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>
