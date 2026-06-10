<script lang="ts">
  // Small shared tab strip — the single underline-tab visual used across the
  // app (detail modals, Browse|Imported / Browse|Installed sub-tabs, and the
  // Add-ons content-kind switch). `ariaLabel` names the tablist for screen
  // readers; `testid` is handy when several tablists coexist.
  import { Icon, type IconName } from '$lib/ui/icons';

  type Tab = { id: string; label: string; icon?: IconName };
  let {
    tabs,
    active,
    onChange,
    ariaLabel = undefined,
    testid = undefined,
  }: {
    tabs: Tab[];
    active: string;
    onChange: (id: string) => void;
    ariaLabel?: string | undefined;
    testid?: string | undefined;
  } = $props();
</script>

<div role="tablist" aria-label={ariaLabel} data-testid={testid} class="border-b flex gap-1">
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      role="tab"
      aria-selected={active === tab.id}
      class="px-3 py-2 text-sm border-b-2 -mb-px inline-flex items-center gap-1.5"
      class:border-accent={active === tab.id}
      class:text-primary={active === tab.id}
      class:font-semibold={active === tab.id}
      class:border-transparent={active !== tab.id}
      class:text-placeholder={active !== tab.id}
      onclick={() => onChange(tab.id)}
    >
      {#if tab.icon}<Icon name={tab.icon} size={14} />{/if}{tab.label}
    </button>
  {/each}
</div>
