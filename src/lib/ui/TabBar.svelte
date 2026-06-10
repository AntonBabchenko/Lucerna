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

  // The rendered tab buttons, in DOM order, so arrow-key navigation can move
  // focus to a sibling. Bound via the each-block index below.
  let tabEls = $state<(HTMLButtonElement | null)[]>([]);

  // WAI-ARIA tabs pattern keyboard support: ArrowLeft/Right move (with wrap),
  // Home/End jump to the ends. Activation follows focus (selecting on move)
  // which is the expected behaviour for a tablist whose panels are already
  // mounted. Up/Down are left to the browser so vertical scroll still works.
  function onKeydown(e: KeyboardEvent) {
    const current = tabs.findIndex((t) => t.id === active);
    if (current === -1) return;
    let next = current;
    if (e.key === 'ArrowRight') next = (current + 1) % tabs.length;
    else if (e.key === 'ArrowLeft') next = (current - 1 + tabs.length) % tabs.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = tabs.length - 1;
    else return;
    e.preventDefault();
    const target = tabs[next];
    if (!target) return;
    onChange(target.id);
    tabEls[next]?.focus();
  }
</script>

<!-- The tablist is a container; the roving-tabindex tabs inside hold focus, so
     the list itself takes no tabindex. The keydown handler only routes arrow
     keys to those focusable children. -->
<!-- svelte-ignore a11y_interactive_supports_focus -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  role="tablist"
  aria-label={ariaLabel}
  data-testid={testid}
  class="border-b flex gap-1"
  onkeydown={onKeydown}
>
  {#each tabs as tab, i (tab.id)}
    <button
      bind:this={tabEls[i]}
      type="button"
      role="tab"
      aria-selected={active === tab.id}
      tabindex={active === tab.id ? 0 : -1}
      class="px-3 py-2 text-sm border-b-2 -mb-px inline-flex items-center gap-1.5"
      class:border-accent={active === tab.id}
      class:text-primary={active === tab.id}
      class:font-semibold={active === tab.id}
      class:border-transparent={active !== tab.id}
      class:text-muted={active !== tab.id}
      onclick={() => onChange(tab.id)}
    >
      {#if tab.icon}<Icon name={tab.icon} size={14} />{/if}{tab.label}
    </button>
  {/each}
</div>
