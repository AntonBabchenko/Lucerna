// tests/tooltip/tooltip-unplaced.test.ts
// A showing whose measure step never places it — TooltipLayer's $effect dead after an uncaught
// error elsewhere (docs/UI-TESTING.md) — shows nothing, never a bubble at the window's origin.
// Its own file: the measure step is mocked out for the whole module.
import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeAll, expect, it, vi } from 'vitest';
import TooltipLayer from '$lib/ui/tooltip/TooltipLayer.svelte';
import {
  hideTooltip,
  positionTooltip,
  showTooltip,
  tooltipState,
} from '$lib/ui/tooltip/tooltip-controller.svelte';

vi.mock('$lib/ui/tooltip/tooltip-controller.svelte', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/ui/tooltip/tooltip-controller.svelte')>()),
  positionTooltip: vi.fn(),
}));

// happy-dom does not implement element.animate (the bubble's entrance transition).
beforeAll(() => {
  if (typeof Element !== 'undefined' && !Element.prototype.animate) {
    Element.prototype.animate = () =>
      ({
        finished: Promise.resolve(),
        cancel: () => {},
        pause: () => {},
        play: () => {},
        reverse: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
      }) as unknown as Animation;
  }
});

afterEach(() => hideTooltip());

it('keeps a bubble that was never placed out of sight', async () => {
  render(TooltipLayer);
  const rect = { top: 100, left: 100, width: 40, height: 20, bottom: 120 } as DOMRect;
  showTooltip(rect, 'Grid view', { placement: 'top', immediate: true });
  const bubble = await screen.findByRole('tooltip');
  await tick();
  // The layer asked; nothing placed the bubble.
  expect(positionTooltip).toHaveBeenCalled();
  expect(tooltipState.placed).toBe(false);
  expect(bubble.classList.contains('invisible')).toBe(true);
});
