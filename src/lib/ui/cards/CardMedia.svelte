<script lang="ts">
  import { Icon, type IconName } from '$lib/ui/icons';

  // Card avatar: the project's icon_url when present, otherwise a content-kind
  // placeholder glyph. Size is named so density/variant pick a consistent box.
  let {
    iconUrl = null,
    placeholder = 'puzzle',
    size = 'md',
  }: {
    iconUrl?: string | null;
    placeholder?: IconName;
    size?: 'sm' | 'md' | 'lg';
  } = $props();

  const BOX: Record<'sm' | 'md' | 'lg', string> = {
    sm: 'w-6 h-6',
    md: 'w-8 h-8',
    lg: 'w-10 h-10',
  };
  const GLYPH: Record<'sm' | 'md' | 'lg', number> = { sm: 14, md: 16, lg: 18 };
</script>

{#if iconUrl}
  <img src={iconUrl} alt="" class={`${BOX[size]} rounded object-cover flex-shrink-0`} />
{:else}
  <div
    class={`${BOX[size]} rounded bg-subtle flex items-center justify-center text-placeholder flex-shrink-0`}
    aria-hidden="true"
  >
    <Icon name={placeholder} size={GLYPH[size]} />
  </div>
{/if}
