<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { navVisual } from '$lib/layout/nav-status';
  import NavStatusIcon from '$lib/layout/NavStatusIcon.svelte';
  import NavFixWrench from '$lib/layout/NavFixWrench.svelte';
  import NavUploadBadge from '$lib/layout/NavUploadBadge.svelte';

  // Top-level launcher mode switch (Client | Servers). Two aria-pressed
  // buttons styled like SegmentedControl's boxed variant — a dedicated
  // component (not SegmentedControl) because the servers segment carries live
  // status content (coloured/pulsing icon + wrench/upload badges) that the
  // shared primitive's string-only options cannot express. Deliberately NOT
  // hideable via hidden_sidebar_buttons: it is the navigation itself.
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

<div
  class="flex rounded border border-border-subtle overflow-hidden"
  role="group"
  aria-label={$t('sidebar.mode.ariaLabel')}
  data-tour-ctx="servers-mode-switch"
>
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none border-0 flex-1 flex items-center justify-center gap-1.5 {serversUi.mode ===
    'client'
      ? 'btn-primary'
      : ''}"
    aria-pressed={serversUi.mode === 'client'}
    data-testid="mode-switch-client"
    onclick={() => serversUi.setMode('client')}
  >
    <Icon name="monitor" size={14} />
    {$t('sidebar.mode.client')}
  </button>
  <button
    type="button"
    class="btn-secondary btn-sm rounded-none border-0 flex-1 flex items-center justify-center gap-1.5 {serversUi.mode ===
    'servers'
      ? 'btn-primary'
      : ''}"
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
