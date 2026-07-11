<script lang="ts">
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { navVisual, type NavStatusKind } from '$lib/layout/nav-status';
  import NavStatusIcon from '$lib/layout/NavStatusIcon.svelte';
  import NavFixWrench from '$lib/layout/NavFixWrench.svelte';
  import NavUploadBadge from '$lib/layout/NavUploadBadge.svelte';

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
  let { clientNav = 'idle' }: { clientNav?: NavStatusKind } = $props();

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
     elevation + text weight, deliberately not an accent fill. -->
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
</div>
