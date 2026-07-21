<script lang="ts">
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { navVisual, type NavStatusKind } from '$lib/layout/nav-status';
  import NavStatusIcon from '$lib/layout/NavStatusIcon.svelte';
  import NavFixWrench from '$lib/layout/NavFixWrench.svelte';
  import NavUploadBadge from '$lib/layout/NavUploadBadge.svelte';
  import RunningInstancesPopover from '$lib/layout/RunningInstancesPopover.svelte';
  import RunningServersPopover from '$lib/layout/RunningServersPopover.svelte';

  // Top-level launcher mode switch (Client | Servers). Two aria-pressed
  // buttons styled like SegmentedControl's boxed variant — a dedicated
  // component (not SegmentedControl) because BOTH segments carry live status
  // content (coloured/pulsing icon; the servers segment also has wrench/upload
  // badges) that the shared primitive's string-only options cannot express.
  // Deliberately NOT hideable via hidden_sidebar_buttons: it is the navigation
  // itself.
  //
  // The client's game state lives page-locally in +page.svelte (not a rune
  // module like serverState), so its status arrives as an opaque NavStatusKind
  // prop threaded +page → Sidebar → here; the servers status is read directly
  // from serverState. Defaults to 'idle' so no-prop mounts render unchanged.
  //
  // The aggregate running-instances pill is threaded the same way: `runningCount`
  // (the page's reactive `running.size`), an id→name resolver, and a jump
  // callback. When runningCount > 0 the pill shows in BOTH modes (the switcher is
  // persistent). Defaults keep no-prop mounts (tests) rendering unchanged — with
  // runningCount 0 the pill never mounts.
  let {
    clientNav = 'idle',
    runningCount = 0,
    instanceName = (id: string) => id,
    onOpenInstance = () => {},
  }: {
    clientNav?: NavStatusKind;
    runningCount?: number;
    instanceName?: (id: string) => string;
    onOpenInstance?: (id: string) => void;
  } = $props();

  const clientVisual = $derived(navVisual(clientNav));
  const clientStatusLabel = $derived(
    clientNav === 'running'
      ? $t('sidebar.clientRunning')
      : clientNav === 'crashed'
        ? $t('sidebar.clientCrashed')
        : null,
  );

  const serversNav = $derived(serverState.serversNavStatus);
  const serversVisual = $derived(navVisual(serversNav));
  const anyUploading = $derived(serverState.anyUploading);
  // Servers mirror of the client running-instances count. Read directly from
  // serverState (the servers side needs no prop threading — unlike the client's
  // page-local model). Drives the superscript badge on the Servers segment icon.
  const runningServersCount = $derived(serverState.list.filter((s) => s.running).length);
  const serversStatusLabel = $derived(
    serversNav === 'running'
      ? $t('sidebar.serverRunning')
      : serversNav === 'crashed'
        ? $t('sidebar.serverCrashed')
        : null,
  );
</script>

<!-- Inset-track segmented control: a recessed bg-black/20 gutter (reads as an
     inset over bg-base in BOTH themes) with one aria-hidden bg-surface pill
     sliding under the active segment (transform only — compositor-friendly;
     state lives on aria-pressed, not on the pill). Activity is conveyed by
     elevation + text weight, deliberately not an accent fill.
     Variant F2: each segment's running-count badge (a real button → popover) is
     an ABSOLUTE superscript pinned over the top-right corner of that segment's
     status icon — out of flow, so it never pushes or crowds the label. It sits
     as a SIBLING of the segment button inside the cell (never nested — nested
     buttons are invalid) and only mounts when that segment has a running count. -->
<div
  class="relative flex rounded-lg bg-black/20 p-[3px]"
  role="group"
  aria-label={$t('sidebar.mode.ariaLabel')}
  data-tour-ctx="servers-mode-switch"
>
  <div
    class="absolute inset-y-[3px] left-[3px] w-[calc(50%-3px)] rounded-md bg-surface shadow transition-transform duration-150 ease-out motion-reduce:transition-none {serversUi.mode ===
    'servers'
      ? 'translate-x-full'
      : ''}"
    aria-hidden="true"
  ></div>
  <div class="relative flex flex-1 items-center">
    <button
      type="button"
      class="relative flex-1 h-7 rounded-md flex items-center justify-center gap-1.5 text-sm transition-colors {serversUi.mode ===
      'client'
        ? 'text-primary font-semibold'
        : 'text-muted hover:text-secondary'}"
      aria-pressed={serversUi.mode === 'client'}
      data-testid="mode-switch-client"
      onclick={() => serversUi.setMode('client')}
    >
      <NavStatusIcon
        name="monitor"
        size={14}
        iconClass={clientVisual.iconClass}
        statusLabel={clientStatusLabel}
      />
      {$t('sidebar.mode.client')}
    </button>
    {#if runningCount > 0}
      <!-- Variant F2: count badge pinned to the segment's top-right corner,
             matching the Servers badge (both trailing) — a sibling of the button
             (not nested), out of flow, in the empty corner so it never overlaps
             the centred icon/label. A real button, keyboard-reachable. -->
      <div class="absolute right-0 top-0 z-10">
        <RunningInstancesPopover compact {runningCount} {instanceName} {onOpenInstance} />
      </div>
    {/if}
  </div>
  <div class="relative flex flex-1 items-center">
    <button
      type="button"
      class="relative flex-1 h-7 rounded-md flex items-center justify-center gap-1.5 text-sm transition-colors {serversUi.mode ===
      'servers'
        ? 'text-primary font-semibold'
        : 'text-muted hover:text-secondary'}"
      aria-pressed={serversUi.mode === 'servers'}
      data-testid="mode-switch-servers"
      onclick={() => serversUi.setMode('servers')}
    >
      <NavStatusIcon
        name="server"
        size={14}
        iconClass={serversVisual.iconClass}
        statusLabel={serversStatusLabel}
      />
      {$t('sidebar.servers')}
      {#if serversVisual.wrench}
        <NavFixWrench label={$t('sidebar.serversFixAvailable')} testid="mode-servers-fix-badge" />
      {/if}
      {#if anyUploading}
        <NavUploadBadge label={$t('sidebar.serversUploading')} testid="mode-servers-upload-badge" />
      {/if}
    </button>
    {#if runningServersCount > 0}
      <!-- Variant F2 mirror: count badge pinned to the segment's OUTER (top-right)
             corner — a sibling of the button (not nested), out of flow, in the
             empty corner so it never overlaps the centred icon/"Серверы" label. A
             real button, keyboard-reachable. -->
      <div class="absolute right-0 top-0 z-10">
        <RunningServersPopover compact runningCount={runningServersCount} />
      </div>
    {/if}
  </div>
</div>
