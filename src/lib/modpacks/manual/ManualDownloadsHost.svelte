<script lang="ts">
  // App-level host for Finish install (DESIGN §8): asked from the toasts, the drawer, the Overview
  // and the Play gate, so the request is a module store and this self-gating host renders it.
  // Mounted after </main>, after ModOpsHost: it can open over the Modpacks modal and the drawer.
  import ManualDownloadsDialog from './ManualDownloadsDialog.svelte';
  import { closeManualDownloads, manualDownloads } from './manual-downloads.svelte';
</script>

{#if manualDownloads.instanceId !== null}
  {#key manualDownloads.instanceId}
    <ManualDownloadsDialog instanceId={manualDownloads.instanceId} onClose={closeManualDownloads} />
  {/key}
{/if}
