<script lang="ts" module>
  export type BannerTone = 'warning' | 'danger' | 'success' | 'accent';
</script>

<script lang="ts">
  // Shared soft severity banner — the `rounded-xl border bg-* p-3` +
  // `flex items-start gap-2` (leading icon · flex-1 body · optional dismiss ×)
  // shell that LogDiagnosisBanner and ServerDiagnosisBanner rendered identically,
  // and that the CurseForge-key gate hand-rolled with drifting radius/border.
  // Severity maps to the §1 token families (§10). The body is a `children`
  // snippet; the caller keeps its own copy, actions, and (for diagnosis banners)
  // the signature-keyed dismiss/restore logic — this only renders the × and
  // calls `onDismiss`.
  import type { Snippet } from 'svelte';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { t } from '$lib/i18n';

  let {
    tone = 'warning',
    icon,
    title,
    role,
    onDismiss,
    dismissLabel,
    dismissTooltip,
    dismissTestid,
    dataTestid,
    dataTourCtx,
    class: extraClass = '',
    children,
  }: {
    tone?: BannerTone;
    /** Leading glyph (tone-coloured). Omit for a no-icon banner. */
    icon?: IconName;
    /** Optional bold, tone-coloured heading rendered above the body. */
    title?: string;
    /** 'alert' for banners that appear after an event (diagnosis); omit for
        always-present gate banners. */
    role?: 'alert' | 'status';
    /** When set, a top-right × is rendered that calls this. The caller owns what
        "dismiss" means (e.g. signature-keyed acknowledgement). */
    onDismiss?: () => void;
    /** Defaults to the standard §10 dismiss copy. */
    dismissLabel?: string;
    dismissTooltip?: string;
    dismissTestid?: string;
    dataTestid?: string;
    dataTourCtx?: string;
    class?: string;
    children: Snippet;
  } = $props();

  // Severity → §1 token families. `dismiss` carries the important-flagged colour
  // the × needs to override .btn-icon's own hover rule.
  const TONE: Record<BannerTone, { box: string; text: string; dismiss: string }> = {
    warning: {
      box: 'border-warning-text bg-warning-bg',
      text: 'text-warning-text',
      dismiss: '!text-warning-text hover:!bg-warning-text/10',
    },
    danger: {
      box: 'border-danger bg-danger-bg',
      text: 'text-danger',
      dismiss: '!text-danger hover:!bg-danger/10',
    },
    success: {
      box: 'border-success bg-success-bg',
      text: 'text-success',
      dismiss: '!text-success hover:!bg-success/10',
    },
    accent: {
      box: 'border-accent bg-accent-soft',
      text: 'text-accent',
      dismiss: '!text-accent hover:!bg-accent/10',
    },
  };
  const tokens = $derived(TONE[tone]);
</script>

<div
  class="rounded-[var(--radius-lg)] border p-3 {tokens.box} {extraClass}"
  {role}
  data-testid={dataTestid}
  data-tour-ctx={dataTourCtx}
>
  <div class="flex items-start gap-2">
    {#if icon}
      <Icon name={icon} class="mt-0.5 shrink-0 {tokens.text}" />
    {/if}
    <div class="flex-1 min-w-0">
      {#if title}
        <p class="font-semibold {tokens.text}">{title}</p>
      {/if}
      {@render children()}
    </div>
    {#if onDismiss}
      <button
        type="button"
        class="btn-icon -my-1 -mr-1 shrink-0 {tokens.dismiss}"
        aria-label={dismissLabel ?? $t('common.dismissWarning')}
        use:tooltip={dismissTooltip ?? $t('common.dismissWarningTooltip')}
        data-testid={dismissTestid}
        onclick={onDismiss}
      >
        <Icon name="close" size={16} />
      </button>
    {/if}
  </div>
</div>
