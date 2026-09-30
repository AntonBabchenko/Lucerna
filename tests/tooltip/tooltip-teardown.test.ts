// tests/tooltip/tooltip-teardown.test.ts
// Plan §5e: closing a toast with its × threw an uncaught Svelte `state_unsafe_mutation`. The ×
// holds focus from the mousedown; the toast list removes it inside its own block effect, the
// browser fires `focusout` on it right then, and the tooltip's blur hide wrote the tooltip's
// $state in the middle of that block. The hide waits for the end of the batch now.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import TooltipTriggerInBlock from '../fixtures/TooltipTriggerInBlock.svelte';

afterEach(() => {
  hideTooltip();
  vi.restoreAllMocks();
});

/** Chromium fires `focusout` on the focused element as it is removed; happy-dom only forgets the
 *  focus. Restores Chromium's order for the elements Svelte removes with `remove()`. */
function focusoutOnRemoval() {
  const remove = Element.prototype.remove;
  vi.spyOn(Element.prototype, 'remove').mockImplementation(function (this: Element) {
    const focused = document.activeElement;
    if (focused instanceof HTMLElement && this.contains(focused)) {
      focused.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
    }
    remove.call(this);
  });
}

describe('a focused tooltip trigger removed by a block', () => {
  it('is hidden after the batch, without writing state in the middle of it', async () => {
    const errors: unknown[] = [];
    const onError = (e: ErrorEvent) => errors.push(e.error ?? e.message);
    window.addEventListener('error', onError);
    try {
      focusoutOnRemoval();
      render(TooltipTriggerInBlock);
      const close = screen.getByRole('button', { name: 'Close' });
      // Keyboard focus (`:focus-visible`, which happy-dom cannot model) shows the tooltip.
      close.matches = () => true;
      close.focus();
      expect(tooltipState.visible).toBe(true);

      await fireEvent.click(close);
      await tick();

      expect(screen.queryByRole('button', { name: 'Close' })).toBeNull();
      expect(errors.map(String)).toEqual([]);
      expect(tooltipState.visible).toBe(false);
    } finally {
      window.removeEventListener('error', onError);
    }
  });
});
