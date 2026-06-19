<script module lang="ts">
  // Shared tone vocabulary + class builder for chip toggles. Exported from the
  // module context so ToggleChipGroup can render real role="radio" buttons with
  // the identical look (rather than wrapping ToggleChip in a non-clickable span).
  export type ToggleChipTone = 'neutral' | 'accent' | 'success' | 'warning' | 'danger' | 'muted';

  // Active -> soft tone background + tone text + tone border. Inactive always
  // falls back to a de-emphasised neutral so an unselected chip never competes
  // with the selected one.
  const ACTIVE_TONE: Record<ToggleChipTone, string> = {
    neutral: 'bg-subtle text-primary border-border-emphasis',
    accent: 'bg-accent/10 text-accent border-accent',
    success: 'bg-success/10 text-success border-success',
    warning: 'bg-warning-bg text-warning-text border-warning-text',
    danger: 'bg-danger/10 text-danger border-danger',
    muted: 'bg-subtle text-muted border-border-emphasis',
  };
  const INACTIVE = 'bg-transparent text-secondary border-border-subtle hover:text-primary';
  const CHIP_BASE =
    'inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-sm transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2';

  export function toggleChipClass(active: boolean, tone: ToggleChipTone): string {
    return `${CHIP_BASE} ${active ? ACTIVE_TONE[tone] : INACTIVE}`;
  }
</script>

<script lang="ts">
  // A standalone rounded-full toggle pill (multi-select, e.g. Logs level chips).
  // For a single-select group use ToggleChipGroup, which renders radio buttons
  // with the same look via toggleChipClass().
  import { Icon, type IconName } from '$lib/ui/icons';

  let {
    active,
    tone,
    label,
    icon,
    count,
    onToggle,
    testId,
  }: {
    active: boolean;
    tone: ToggleChipTone;
    label: string;
    icon?: IconName;
    count?: number;
    onToggle: () => void;
    testId?: string;
  } = $props();
</script>

<button
  type="button"
  aria-pressed={active}
  data-testid={testId}
  class={toggleChipClass(active, tone)}
  onclick={onToggle}
>
  {#if icon}<Icon name={icon} size={14} />{/if}
  <span>{label}</span>
  {#if count !== undefined}<span class="opacity-70">{count}</span>{/if}
</button>
