<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { deriveAvatar, type AvatarTone } from '$lib/overview/avatar';
  import { loadInstanceIcon } from './instance-icon-cache';

  type AvatarInstance = Pick<
    InstanceWithStatus,
    'id' | 'name' | 'loader' | 'mrpack_source' | 'has_icon'
  >;

  let { instance, size = 52 }: { instance: AvatarInstance; size?: number } = $props();

  const avatar = $derived(deriveAvatar(instance));
  let iconUrl = $state<string | null>(null);

  // Load (cache-backed) whenever the instance/has_icon changes; ignore a stale
  // resolve if the instance changed mid-flight.
  $effect(() => {
    const id = instance.id;
    const wants = instance.has_icon;
    let cancelled = false;
    iconUrl = null;
    if (wants) {
      loadInstanceIcon(id).then((u) => {
        if (!cancelled) iconUrl = u;
      });
    }
    return () => {
      cancelled = true;
    };
  });

  // Loader/source tint for the letter fallback. Brand-ish, theme-agnostic.
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
</script>

{#if iconUrl}
  <img
    src={iconUrl}
    alt={$t('instance.avatarAlt')}
    class="flex-none object-cover"
    style="width:{size}px;height:{size}px;border-radius:{Math.round(size * 0.22)}px"
    onerror={() => (iconUrl = null)}
  />
{:else}
  <div
    class="flex-none flex items-center justify-center font-extrabold {TONE_BG[avatar.tone]}"
    style="width:{size}px;height:{size}px;font-size:{Math.round(
      size * 0.42,
    )}px;border-radius:{Math.round(size * 0.22)}px"
    aria-hidden="true"
  >
    {avatar.letter}
  </div>
{/if}
