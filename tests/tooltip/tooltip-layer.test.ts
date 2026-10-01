// tests/tooltip/tooltip-layer.test.ts
import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import TooltipLayer from '$lib/ui/tooltip/TooltipLayer.svelte';
import { hideTooltip, showTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';

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

// Plan §5d L2 (screenshot tt-kind-help-after-trash): a bubble was laid out where the previous one
// stood before it was measured. By the window's right edge its text wrapped to the few pixels left
// there, and it was placed at that squeezed size — «Что / такое / моды?». happy-dom lays nothing
// out: the bubble's width here is what a browser's shrink-to-fit gives it at its `left` — its own
// width, or the room left to the window's edge when that is less.
describe('TooltipLayer measures each bubble at its own size', () => {
  const NATURAL: Record<string, number> = { Delete: 66, 'What are mods?': 117 };
  const own = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth');
  beforeEach(() => {
    Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
      configurable: true,
      get(this: HTMLElement) {
        if (this.id !== 'app-tooltip') return 0;
        const text = this.firstChild?.textContent?.trim() ?? '';
        const left = Number.parseFloat(this.style.left) || 0;
        return Math.min(NATURAL[text] ?? 0, window.innerWidth - left);
      },
    });
  });
  afterEach(() => {
    if (own) Object.defineProperty(HTMLElement.prototype, 'offsetWidth', own);
  });

  // The far-right trash button, then a trigger at x = 100.
  const byTheEdge = () =>
    ({ top: 100, left: window.innerWidth - 30, width: 20, height: 20, bottom: 120 }) as DOMRect;
  const centredLeft = (trigger: DOMRect, width: number) =>
    trigger.left + trigger.width / 2 - width / 2;

  it('is not squeezed by where the previous bubble stood', async () => {
    render(TooltipLayer);
    showTooltip(byTheEdge(), 'Delete', { placement: 'top', immediate: true });
    await tick();
    // Clamped against the edge at its own width: 66 px of room are left where it stands.
    expect(tooltipState.left).toBe(window.innerWidth - 66 - 8);
    hideTooltip();
    await tick();
    showTooltip(rect, 'What are mods?', { placement: 'top', immediate: true });
    await tick();
    expect(tooltipState.left).toBe(centredLeft(rect, NATURAL['What are mods?'] ?? 0));
  });

  it('is measured and placed again when it takes over from one still showing', async () => {
    render(TooltipLayer);
    showTooltip(byTheEdge(), 'Delete', { placement: 'top', immediate: true, owner: 'a' });
    await tick();
    // A trigger inside the first one (a wrapper and its button) opens without the first closing.
    showTooltip(rect, 'What are mods?', { placement: 'top', immediate: true, owner: 'b' });
    await tick();
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.left).toBe(centredLeft(rect, NATURAL['What are mods?'] ?? 0));
  });
});
