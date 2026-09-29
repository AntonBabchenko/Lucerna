<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import InstanceAvatarEdit from '$lib/instances/InstanceAvatarEdit.svelte';
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
    blockingMods,
    attentionCollapsed = false,
    attentionCount = 0,
    onShowAttention = () => {},
  }: {
    instance: InstanceWithStatus;
    running: boolean;
    installing: boolean;
    // Mods that stop the game, from the page pre-flight (the Play gate's verdict); null while it
    // has not answered. No default: a pill that says «Ready to play» must have been told.
    blockingMods: number | null;
    attentionCollapsed?: boolean;
    attentionCount?: number;
    onShowAttention?: () => void;
  } = $props();

  const status = $derived(deriveStatus(instance, running, installing, blockingMods));

  // The restore affordance only makes sense when the panel is collapsed AND
  // there is actually something hidden behind it.
  const showRestore = $derived(attentionCollapsed && attentionCount > 0);

  const PILL_LABEL: Record<StatusKind, TranslationKey> = {
    running: 'page.overview.pillRunning',
    ready: 'page.overview.pillReady',
    needs_install: 'page.overview.pillNeedsInstall',
    pick_version: 'page.overview.pillPickVersion',
    installing: 'page.overview.pillInstalling',
    mods_unknown: 'page.overview.pillModsUnknown',
    mods_blocking: 'page.overview.pillModsBlocking',
  };

  // Every tooltip gets the blocking count; only the blocking one says it — in the attention
  // item's own words, so the pill and «Needs attention» never tell it two ways.
  const PILL_TOOLTIP: Record<StatusKind, TranslationKey> = {
    running: 'page.overview.pillTooltip.running',
    ready: 'page.overview.pillTooltip.ready',
    needs_install: 'page.overview.pillTooltip.needsInstall',
    pick_version: 'page.overview.pillTooltip.pickVersion',
    installing: 'page.overview.pillTooltip.installing',
    mods_unknown: 'page.overview.pillTooltip.modsUnknown',
    mods_blocking: 'page.overview.attnModsBlocking',
  };

  // Blocking is red, as everywhere a mod stops the game (DESIGN.md §9) — on the surface with a
  // danger border, never on the soft danger box, whose red text misses AA (DESIGN.md Known gaps).
  const PILL_TONE: Record<StatusTone, string> = {
    ok: 'bg-success-bg border-success text-success',
    warn: 'bg-warning-bg border-warning-text text-warning-text',
    accent: 'bg-accent-soft border-accent text-accent',
    danger: 'bg-surface border-danger text-danger',
    neutral: 'bg-surface border-border-subtle text-secondary',
  };
</script>

<div class="flex items-center gap-4" data-testid="overview-instance-header">
  <InstanceAvatarEdit
    {instance}
    size={52}
    testId="overview-avatar"
    removeTestId="overview-avatar-remove"
  />

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
      use:tooltip={$t(PILL_TOOLTIP[status.kind], { count: blockingMods ?? 0 })}
    >
      <span class="h-2 w-2 rounded-full bg-current" aria-hidden="true"></span>
      {$t(PILL_LABEL[status.kind])}
    </div>
  </div>
</div>
