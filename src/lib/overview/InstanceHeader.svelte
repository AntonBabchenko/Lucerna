<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { deriveAvatar, type AvatarTone } from './avatar';
  import { deriveStatus, type StatusKind, type StatusTone } from './status';

  let {
    instance,
    running,
    installing,
  }: { instance: InstanceWithStatus; running: boolean; installing: boolean } = $props();

  const avatar = $derived(deriveAvatar(instance));
  const status = $derived(deriveStatus(instance, running, installing));

  // Avatar tint per loader / source. Brand-ish accents, theme-agnostic.
  const TONE_BG: Record<AvatarTone, string> = {
    vanilla: 'bg-gradient-to-br from-emerald-500 to-emerald-700 text-emerald-50',
    fabric: 'bg-gradient-to-br from-amber-400 to-amber-600 text-amber-950',
    quilt: 'bg-gradient-to-br from-fuchsia-400 to-fuchsia-600 text-fuchsia-50',
    forge: 'bg-gradient-to-br from-slate-400 to-slate-600 text-slate-50',
    neoforge: 'bg-gradient-to-br from-orange-400 to-orange-600 text-orange-950',
    modrinth: 'bg-gradient-to-br from-green-400 to-green-600 text-green-950',
    curseforge: 'bg-gradient-to-br from-orange-500 to-red-600 text-orange-50',
    ftb: 'bg-gradient-to-br from-sky-400 to-sky-600 text-sky-950',
    atlauncher: 'bg-gradient-to-br from-indigo-400 to-indigo-600 text-indigo-50',
  };

  const PILL_LABEL: Record<StatusKind, TranslationKey> = {
    running: 'page.overview.pillRunning',
    ready: 'page.overview.pillReady',
    needs_install: 'page.overview.pillNeedsInstall',
    pick_version: 'page.overview.pillPickVersion',
    installing: 'page.overview.pillInstalling',
  };

  const PILL_TONE: Record<StatusTone, string> = {
    ok: 'bg-success-bg border-success text-success',
    warn: 'bg-warning-bg border-warning-text text-warning-text',
    accent: 'bg-accent-soft border-accent text-accent',
  };
</script>

<div class="flex items-center gap-4" data-testid="overview-instance-header">
  <div
    class="flex-none rounded-xl flex items-center justify-center text-2xl font-extrabold {TONE_BG[
      avatar.tone
    ]}"
    style="width:3.25rem;height:3.25rem;"
    role="img"
    aria-label={$t('page.overview.avatarAlt')}
    data-testid="overview-avatar"
  >
    {avatar.letter}
  </div>

  <div class="min-w-0">
    <div class="text-xl font-bold text-primary truncate">{instance.name}</div>
    <div class="flex gap-1.5 mt-2 flex-wrap text-xs">
      <span class="rounded-full border border-border-subtle bg-base px-2.5 py-0.5 text-secondary">
        Minecraft {instance.mc_version || $t('page.overview.notSet')}
      </span>
      <span class="rounded-full border border-border-subtle bg-base px-2.5 py-0.5 text-secondary">
        {displayLoader(instance.loader)}{#if instance.loader_version}
          {' '}{instance.loader_version}{/if}
      </span>
      <span class="rounded-full border border-border-subtle bg-base px-2.5 py-0.5 text-secondary">
        {instance.max_heap_mb} MB
      </span>
    </div>
  </div>

  <div
    class="ml-auto flex items-center gap-2 rounded-full border px-3.5 py-2 text-xs font-semibold {PILL_TONE[
      status.tone
    ]}"
    data-testid="overview-status-pill"
    data-status={status.kind}
  >
    <span class="h-2 w-2 rounded-full bg-current" aria-hidden="true"></span>
    {$t(PILL_LABEL[status.kind])}
  </div>
</div>
