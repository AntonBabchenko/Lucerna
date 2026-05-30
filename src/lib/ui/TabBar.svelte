<script lang="ts">
  // Small shared tab strip for the detail modals. Visual style mirrors the
  // existing Browse|Imported strip in ModpacksTab.
  type Tab = { id: string; label: string };
  let { tabs, active, onChange }: { tabs: Tab[]; active: string; onChange: (id: string) => void } =
    $props();
</script>

<div role="tablist" class="border-b flex gap-1">
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      role="tab"
      aria-selected={active === tab.id}
      class="px-3 py-2 text-sm border-b-2 -mb-px"
      class:border-accent={active === tab.id}
      class:text-primary={active === tab.id}
      class:font-semibold={active === tab.id}
      class:border-transparent={active !== tab.id}
      class:text-placeholder={active !== tab.id}
      onclick={() => onChange(tab.id)}
    >
      {tab.label}
    </button>
  {/each}
</div>
