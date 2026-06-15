import { describe, expect, it } from 'vitest';
import { computePopoverPlacement } from '$lib/ui/select-placement';

const OPTS = { gap: 4, margin: 8, maxHeight: 240 };

describe('computePopoverPlacement', () => {
  it('opens downward at full height when there is ample room below', () => {
    // Tall viewport, trigger near the top.
    const p = computePopoverPlacement(
      { top: 100, bottom: 132, left: 12, width: 300 },
      { width: 1000, height: 800 },
      OPTS,
    );
    expect(p.flipUp).toBe(false);
    expect(p.top).toBe(136); // bottom + gap
    expect(p.maxHeight).toBe(240); // capped at the preferred max, not clamped
  });

  it('clamps the height to the room below in a short (compact) viewport', () => {
    // Compact: the window is short. Trigger near the top → little room below,
    // even less above, so it stays downward but must shrink + scroll.
    const p = computePopoverPlacement(
      { top: 110, bottom: 142, left: 12, width: 300 },
      { width: 326, height: 300 },
      OPTS,
    );
    expect(p.flipUp).toBe(false);
    // spaceBelow = 300 - 142 - 4 - 8 = 146 → clamped below the 240 preferred cap.
    expect(p.maxHeight).toBe(146);
  });

  it('flips up when below is cramped and above has more room', () => {
    // Trigger near the bottom of the viewport.
    const p = computePopoverPlacement(
      { top: 600, bottom: 632, left: 12, width: 300 },
      { width: 1000, height: 700 },
      OPTS,
    );
    expect(p.flipUp).toBe(true);
    expect(p.top).toBe(596); // trigger.top - gap (caller anchors the bottom here)
    // spaceAbove = 600 - 4 - 8 = 588 → still capped at the 240 preferred max.
    expect(p.maxHeight).toBe(240);
  });

  it('clamps the upward height too when neither side is tall', () => {
    // Short viewport, trigger low → flips up into limited room above.
    const p = computePopoverPlacement(
      { top: 200, bottom: 232, left: 12, width: 300 },
      { width: 326, height: 260 },
      OPTS,
    );
    expect(p.flipUp).toBe(true);
    // spaceAbove = 200 - 4 - 8 = 188 → clamped under the preferred max.
    expect(p.maxHeight).toBe(188);
  });

  it('keeps the popover within the horizontal margin', () => {
    // Trigger pushed off the right edge → left clamps so it stays on screen.
    const p = computePopoverPlacement(
      { top: 100, bottom: 132, left: 320, width: 300 },
      { width: 326, height: 800 },
      OPTS,
    );
    // maxLeft = 326 - 300 - 8 = 18 → left clamped to 18, not the trigger's 320.
    expect(p.left).toBe(18);
    expect(p.width).toBe(300);
  });
});
