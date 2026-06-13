// tests/tooltip/tooltip-action.test.ts
import { afterEach, describe, expect, it, vi } from 'vitest';
import { tooltip } from '$lib/ui/tooltip/tooltip';
import { hideTooltip, TOOLTIP_ID, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';

function mount(param: Parameters<typeof tooltip>[1]) {
  const node = document.createElement('button');
  document.body.appendChild(node);
  const handle = tooltip(node, param);
  return { node, handle };
}

afterEach(() => {
  hideTooltip();
  document.body.innerHTML = '';
  vi.useRealTimers();
});

describe('use:tooltip action', () => {
  it('shows immediately on focus and sets aria-describedby when no aria-label', () => {
    const { node } = mount('Grid view');
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Grid view');
    expect(node.getAttribute('aria-describedby')).toBe(TOOLTIP_ID);
  });

  it('skips aria-describedby when the node already has an aria-label', () => {
    const { node } = mount('Grid view');
    node.setAttribute('aria-label', 'Grid view');
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
    expect(node.hasAttribute('aria-describedby')).toBe(false);
  });

  it('hides and clears aria-describedby on blur', () => {
    const { node } = mount('Grid view');
    node.dispatchEvent(new FocusEvent('focusin'));
    node.dispatchEvent(new FocusEvent('focusout'));
    expect(tooltipState.visible).toBe(false);
    expect(node.hasAttribute('aria-describedby')).toBe(false);
  });

  it('does nothing for a null / empty param', () => {
    const { node } = mount(null);
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
  });

  it('whenOverflowing suppresses the tooltip while the text fits', () => {
    const { node } = mount({ text: 'Full name', whenOverflowing: true });
    // happy-dom reports 0/0; stub the clip check: not overflowing.
    Object.defineProperty(node, 'scrollWidth', { value: 50, configurable: true });
    Object.defineProperty(node, 'clientWidth', { value: 50, configurable: true });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
  });

  it('whenOverflowing shows the tooltip when the text is clipped', () => {
    const { node } = mount({ text: 'Full name', whenOverflowing: true });
    Object.defineProperty(node, 'scrollWidth', { value: 200, configurable: true });
    Object.defineProperty(node, 'clientWidth', { value: 50, configurable: true });
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
  });

  it('update(null) hides an open tooltip', () => {
    const { node, handle } = mount('Hi');
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(true);
    handle?.update?.(null);
    expect(tooltipState.visible).toBe(false);
  });

  it('destroy removes listeners and hides', () => {
    const { node, handle } = mount('Hi');
    handle?.destroy?.();
    node.dispatchEvent(new FocusEvent('focusin'));
    expect(tooltipState.visible).toBe(false);
  });
});
