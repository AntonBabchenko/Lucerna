<script lang="ts">
  // The inline log-hint hover card. One instance per surface (client log
  // viewer / server console). Rich hover card (HelpPopover-style,
  // deliberately NOT role=tooltip — the app's tooltip singleton stays
  // unique; DESIGN.md carve-out lands with this feature's docs task).
  // Fixed-position family: measures itself, flips top/bottom, clamps to
  // the viewport, closes on captured scroll/resize and Escape. Content is
  // plain localized text — no actions in v1 (the diagnosis banner keeps
  // the repair flow). Hover-persistent by design: the pointer can travel
  // onto the card and select text.
  import { t } from '$lib/i18n';
  import { computePosition } from '$lib/ui/tooltip/position';
  import type { LogHintHover } from './log-hints.svelte';

  let { hover }: { hover: LogHintHover } = $props();

  let card = $state<HTMLDivElement | null>(null);
  let top = $state(0);
  let left = $state(0);

  // Listener lifecycle keys on open/closed only — retargeting between
  // annotated rows must not tear down and re-add the window listeners.
  const open = $derived(hover.active !== null);

  // Measure + position after render whenever the active hint changes.
  $effect(() => {
    const hint = hover.active;
    if (!hint || !card) return;
    const pos = computePosition(
      hint.anchor,
      { width: card.offsetWidth, height: card.offsetHeight },
      'bottom',
      { width: window.innerWidth, height: window.innerHeight },
    );
    top = pos.top;
    left = pos.left;
  });

  // Fixed-popover family behavior: close on any captured scroll/resize
  // (the log body scrolls under the card otherwise) and on Escape.
  $effect(() => {
    if (!open) return;
    const close = () => hover.close();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') hover.close();
    };
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('keydown', onKey);
    };
  });
</script>

{#if hover.active}
  {@const hint = hover.active}
  <!-- svelte-ignore a11y_no_static_element_interactions -- pointer handlers only keep
       the hover bridge open; the keyboard path is the gutter badge + Escape
       (later tasks), so the card itself is deliberately non-interactive. -->
  <div
    bind:this={card}
    aria-label={$t('logs.hints.cardAriaLabel')}
    data-testid="log-hint-card"
    class="fixed z-[210] w-max max-w-sm rounded-md border border-border-subtle bg-surface
           p-3 text-xs leading-snug shadow-lg selectable"
    style="top: {top}px; left: {left}px;"
    onpointerenter={() => hover.cardEnter()}
    onpointerleave={() => hover.cardLeave()}
  >
    <p class="font-semibold text-primary">{hint.copy.title}</p>
    <p class="mt-1.5 text-secondary">{hint.copy.explanation}</p>
    <p class="mt-1.5 text-secondary">
      <span class="font-semibold text-primary">{$t('logs.diagnosis.whatToTry')}</span>
      {hint.copy.recommendation}
    </p>
  </div>
{/if}
