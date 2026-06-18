<script lang="ts">
  // The Installed pane was decomposed into ./installed/* (composables +
  // sub-components) during the 2026-06 refactor. This shim preserves the
  // original import path used by ModBrowserTab and the existing tests.
  import type { LoaderKind } from '$lib/ipc/bindings';
  import Installed from './installed/InstalledModsView.svelte';

  let {
    instanceId,
    mcVersion,
    loader,
    onBrowseFor = (_q: string) => {},
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
    onBrowseFor?: (query: string) => void;
  } = $props();
</script>

<Installed {instanceId} {mcVersion} {loader} {onBrowseFor} />
