<script lang="ts">
  // App-level host for guarded mod operations (DESIGN.md §8, app-level blocking dialogs). The
  // call sites — Installed row, bulk bar, Browse card, imported-pack drawer — live in different
  // subtrees, one inside a Modal, so the question is a module store and this self-gating host
  // renders it. Mounted after </main> in +page.svelte: modals stack by DOM order, and the drawer
  // that may have asked must be under it.
  import { onMount } from 'svelte';
  import OrphanUninstallDialog from '$lib/mods/OrphanUninstallDialog.svelte';
  import {
    answerDialog,
    attachOpsHost,
    opsDialog,
    setOpsActiveInstance,
  } from '$lib/mods/mod-ops.svelte';
  import ModImpactDialog from './ModImpactDialog.svelte';

  let { activeInstanceId = null }: { activeInstanceId?: string | null } = $props();

  // A restore after a profile switch names the profile it lands in (spec §6.1).
  $effect(() => {
    setOpsActiveInstance(activeInstanceId);
  });

  // Once the last host is gone nothing can answer a question: mod-ops then cancels every flow
  // still waiting for one. It decides from its plain module state, never from `$state` read here —
  // a teardown sees pre-batch values.
  onMount(attachOpsHost);

  const dialog = opsDialog();
</script>

{#key dialog.id}
  {#if dialog.view}
    {@const view = dialog.view}
    {#if view.mode === 'orphans'}
      <OrphanUninstallDialog
        removingNames={view.targets.map((x) => x.name)}
        orphans={view.orphans}
        onCancel={() => answerDialog('cancel')}
        onConfirm={(shas) => answerDialog('primary', shas)}
      />
    {:else}
      <ModImpactDialog {view} busy={dialog.busy} onChoose={(c) => answerDialog(c)} />
    {/if}
  {/if}
{/key}
