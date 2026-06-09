<script lang="ts">
  // Single wrapper for every UI icon. Consumers reference a semantic name
  // (see registry.ts); they never import Lucide directly.
  //
  // Defaults: 16px, currentColor (inherit from Tailwind text-* tokens, so
  // light/dark theming is automatic), stroke-width 2. Decorative by default
  // (aria-hidden) — pass `label` only when the icon is the sole carrier of
  // meaning (e.g. an icon-only control with no other accessible name).
  import { ICONS, type IconName } from './registry';

  let {
    name,
    size = 16,
    strokeWidth = 2,
    class: klass = '',
    label,
  }: {
    name: IconName;
    size?: number;
    strokeWidth?: number;
    class?: string;
    label?: string;
  } = $props();

  const Cmp = $derived(ICONS[name]);
</script>

<Cmp
  {size}
  {strokeWidth}
  class={klass}
  aria-hidden={label ? undefined : true}
  aria-label={label}
  role={label ? 'img' : undefined}
/>
