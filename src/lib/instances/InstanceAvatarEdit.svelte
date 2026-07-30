<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import InstanceAvatar from '$lib/instances/InstanceAvatar.svelte';
  import { iconDialog } from '$lib/instances/instance-icon-dialog.svelte';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  type EditableInstance = Pick<
    InstanceWithStatus,
    'id' | 'name' | 'loader' | 'mrpack_source' | 'has_icon'
  >;

  let {
    instance,
    size = 52,
    testId,
    removeTestId,
  }: {
    instance: EditableInstance;
    size?: number;
    testId?: string;
    removeTestId?: string;
  } = $props();

  // Mirrors InstanceAvatar's computed rounding so the overlay and the focus
  // ring follow the picture's corners at any size.
  const radius = $derived(Math.round(size * 0.22));
</script>

<!-- The avatar IS the change affordance: click opens the OS file picker
     directly (the crop dialog appears once a file decodes). When a custom
     picture exists, hovering (or keyboard focus) reveals a corner trash badge
     — a sibling, not nested, so both stay real buttons. -->
<div class="group relative flex-none">
  <button
    type="button"
    class="relative block focus-visible:outline focus-visible:outline-2
      focus-visible:outline-accent focus-visible:outline-offset-2"
    style="border-radius:{radius}px"
    onclick={() => iconDialog.pick(instance.id)}
    use:tooltip={$t('instance.icon.editTooltip')}
    aria-label={$t('instance.icon.editTooltip')}
    data-testid={testId}
  >
    <InstanceAvatar {instance} {size} />
    <!-- :focus-visible (not :focus-within) so a mouse click that leaves the
         button focused — e.g. after cancelling the OS file picker — does not
         pin the overlay; only keyboard focus reveals it. -->
    <span
      class="absolute inset-0 flex items-center justify-center bg-black/45 opacity-0
        transition-opacity group-hover:opacity-100 group-has-[:focus-visible]:opacity-100"
      style="border-radius:{radius}px"
      aria-hidden="true"
    >
      <Icon name="edit" size={Math.round(size * 0.35)} class="text-white" />
    </span>
  </button>
  {#if instance.has_icon}
    <button
      type="button"
      class="btn-icon btn-icon-sm btn-icon-danger absolute -right-2 -top-2 z-10 opacity-0
        transition-opacity group-hover:opacity-100 group-has-[:focus-visible]:opacity-100"
      onclick={() => iconDialog.requestRemove(instance.id)}
      aria-label={$t('instance.icon.remove')}
      use:tooltip={$t('instance.icon.remove')}
      data-testid={removeTestId}
    >
      <Icon name="trash" size={13} />
    </button>
  {/if}
</div>
