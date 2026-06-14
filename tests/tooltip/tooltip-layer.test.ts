// tests/tooltip/tooltip-layer.test.ts
import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import TooltipLayer from '$lib/ui/tooltip/TooltipLayer.svelte';
import { hideTooltip, showTooltip } from '$lib/ui/tooltip/tooltip-controller.svelte';

// happy-dom does not implement element.animate (used by svelte/transition fade).
// Stub it so the transition is a no-op and tests run without unhandled errors.
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

const rect = { top: 100, left: 100, width: 40, height: 20, bottom: 120 } as DOMRect;

afterEach(() => hideTooltip());

describe('TooltipLayer', () => {
  it('renders nothing while no tooltip is visible', () => {
    render(TooltipLayer);
    expect(screen.queryByRole('tooltip')).toBeNull();
  });

  it('renders the bubble with role=tooltip and the controller text when visible', async () => {
    render(TooltipLayer);
    showTooltip(rect, 'Grid view', { placement: 'top', immediate: true });
    const bubble = await screen.findByRole('tooltip');
    expect(bubble.id).toBe('app-tooltip');
    expect(bubble.textContent?.trim()).toBe('Grid view');
  });
});
