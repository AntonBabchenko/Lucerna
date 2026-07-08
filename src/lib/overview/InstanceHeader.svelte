<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import InstanceAvatar from '$lib/instances/InstanceAvatar.svelte';
  import { iconDialog } from '$lib/instances/instance-icon-dialog.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { deriveStatus, type StatusKind, type StatusTone } from './status';

  let {
    instance,
    running,
    installing,
    attentionCollapsed = false,
    attentionCount = 0,
    onShowAttention = () => {},
  }: {
    instance: InstanceWithStatus;
    running: boolean;
    installing: boolean;
    attentionCollapsed?: boolean;
    attentionCount?: number;
    onShowAttention?: () => void;
  } = $props();

  const status = $derived(deriveStatus(instance, running, installing));

  // The restore affordance only makes sense when the panel is collapsed AND
  // there is actually something hidden behind it.
  const showRestore = $derived(attentionCollapsed && attentionCount > 0);

  const PILL_LABEL: Record<StatusKind, TranslationKey> = {
    running: 'page.overview.pillRunning',
    ready: 'page.overview.pillReady',
    needs_install: 'page.overview.pillNeedsInstall',
    pick_version: 'page.overview.pillPickVersion',
    installing: 'page.overview.pillInstalling',
  };

  const PILL_TOOLTIP: Record<StatusKind, TranslationKey> = {
    running: 'page.overview.pillTooltip.running',
    ready: 'page.overview.pillTooltip.ready',
    needs_install: 'page.overview.pillTooltip.needsInstall',
    pick_version: 'page.overview.pillTooltip.pickVersion',
    installing: 'page.overview.pillTooltip.installing',
  };

  const PILL_TONE: Record<StatusTone, string> = {
    ok: 'bg-success-bg border-success text-success',
    warn: 'bg-warning-bg border-warning-text text-warning-text',
    accent: 'bg-accent-soft border-accent text-accent',
  };
</script>

<div class="flex items-center gap-4" data-testid="overview-instance-header">
  <!-- The avatar is the change affordance: click opens the OS file picker
       directly (crop dialog appears once a file decodes). When a custom
       picture exists, hovering (or keyboard focus) reveals a corner trash
       badge — siblings, not nested, so both stay real buttons. -->
  <div class="group relative flex-none">
    <button
      type="button"
      class="block rounded-xl focus-visible:outline focus-visible:outline-2
        focus-visible:outline-accent focus-visible:outline-offset-2"
      onclick={() => iconDialog.pick(instance.id)}
      use:tooltip={$t('instance.icon.editTooltip')}
      aria-label={$t('instance.icon.editTooltip')}
      data-testid="overview-avatar"
    >
      <InstanceAvatar {instance} size={52} />
    </button>
    {#if instance.has_icon}
      <button
        type="button"
        class="btn-icon btn-icon-sm btn-icon-danger absolute -right-2 -top-2 z-10 opacity-0
          transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
        onclick={() => iconDialog.requestRemove(instance.id)}
        aria-label={$t('instance.icon.remove')}
        use:tooltip={$t('instance.icon.remove')}
        data-testid="overview-avatar-remove"
      >
        <Icon name="trash" size={13} />
      </button>
    {/if}
  </div>

  <div class="min-w-0">
    <div
      class="text-xl font-bold text-primary truncate"
      use:tooltip={{ text: instance.name, whenOverflowing: true }}
    >
      {instance.name}
    </div>
    <div class="flex gap-1.5 mt-2 flex-wrap text-xs">
      <span class="rounded-full border border-border-subtle bg-base px-2.5 py-0.5 text-secondary">
        Minecraft {instance.mc_version || $t('page.overview.notSet')}
      </span>
      <span class="rounded-full border border-border-subtle bg-base px-2.5 py-0.5 text-secondary">
        {displayLoader(instance.loader)}{#if instance.loader_version}
          {' '}{instance.loader_version}{/if}
      </span>
      <span class="rounded-full border border-border-subtle bg-base px-2.5 py-0.5 text-secondary">
        {instance.max_heap_mb}
        {$t('format.unit.megabyte')}
      </span>
    </div>
  </div>

  <div class="ml-auto flex items-center gap-2">
    {#if showRestore}
      <button
        type="button"
        class="flex items-center rounded-full border border-warning-text bg-warning-bg px-3 py-2
          text-warning-text transition-colors hover:bg-warning-text/10
          focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2"
        data-testid="overview-attention-restore"
        aria-label={$t('page.overview.attentionShow')}
        use:tooltip={$t('page.overview.attentionShow')}
        onclick={() => onShowAttention()}
      >
        <Icon name="warning" size={16} />
      </button>
    {/if}
    <div
      class="flex items-center gap-2 rounded-full border px-3.5 py-2 text-xs font-semibold {PILL_TONE[
        status.tone
      ]}"
      data-testid="overview-status-pill"
      data-status={status.kind}
      use:tooltip={$t(PILL_TOOLTIP[status.kind])}
    >
      <span class="h-2 w-2 rounded-full bg-current" aria-hidden="true"></span>
      {$t(PILL_LABEL[status.kind])}
    </div>
  </div>
</div>
