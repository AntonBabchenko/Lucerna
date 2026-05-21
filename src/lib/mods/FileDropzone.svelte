<script lang="ts">
  import { dragActive } from '$lib/settings/state.svelte';

  // Visible drop+click affordance, shown on every Mods / Modpacks
  // sub-tab. Presentational only: the actual file drop is detected by
  // MainTabs' single window-level listener (a per-box listener would
  // re-introduce the dual-listener conflict). This box gives the static
  // "you can drop here / click to browse" hint, opens the file picker
  // on click via `onClick`, and highlights while a drag is in progress
  // (the `dragActive` rune, set by MainTabs).
  let {
    label,
    disabled = false,
    disabledLabel,
    onClick,
  }: {
    label: string;
    disabled?: boolean;
    disabledLabel?: string;
    onClick: () => void;
  } = $props();

  function activate() {
    if (!disabled) onClick();
  }
</script>

<div
  class="border-2 border-dashed rounded-lg p-3 text-center text-sm transition-colors"
  class:cursor-pointer={!disabled}
  class:border-blue-400={dragActive.value && !disabled}
  class:bg-blue-50={dragActive.value && !disabled}
  class:border-neutral-300={!dragActive.value || disabled}
  class:hover:border-blue-300={!disabled && !dragActive.value}
  class:opacity-50={disabled}
  onclick={activate}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') activate();
  }}
  role="button"
  tabindex={disabled ? -1 : 0}
  aria-disabled={disabled}
  data-testid="file-dropzone"
>
  <span class="text-neutral-700">{disabled ? (disabledLabel ?? label) : label}</span>
</div>
