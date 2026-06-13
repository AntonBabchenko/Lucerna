// tests/tooltip/position.test.ts
import { describe, expect, it } from 'vitest';
import { computePosition } from '$lib/ui/tooltip/position';

const viewport = { width: 1000, height: 800 };
const bubble = { width: 200, height: 40 };

// A trigger comfortably in the middle of the viewport.
const midTrigger = { top: 400, left: 480, width: 40, height: 20, bottom: 420 };

describe('computePosition', () => {
  it('places the bubble above the trigger by default, horizontally centered', () => {
    const r = computePosition(midTrigger, bubble, 'top', viewport);
    expect(r.placement).toBe('top');
    expect(r.top).toBe(400 - 40 - 6); // trigger.top - bubble.height - GAP
    // centered: left + width/2 - bubble.width/2 = 480 + 20 - 100 = 400
    expect(r.left).toBe(400);
  });

  it('flips to bottom when there is no room above', () => {
    const topTrigger = { top: 10, left: 480, width: 40, height: 20, bottom: 30 };
    const r = computePosition(topTrigger, bubble, 'top', viewport);
    expect(r.placement).toBe('bottom');
    expect(r.top).toBe(30 + 6); // trigger.bottom + GAP
  });

  it('clamps the left edge into the viewport (left overflow)', () => {
    const leftTrigger = { top: 400, left: 0, width: 20, height: 20, bottom: 420 };
    const r = computePosition(leftTrigger, bubble, 'top', viewport);
    expect(r.left).toBe(8); // MARGIN
  });

  it('clamps the right edge into the viewport (right overflow)', () => {
    const rightTrigger = { top: 400, left: 990, width: 20, height: 20, bottom: 420 };
    const r = computePosition(rightTrigger, bubble, 'top', viewport);
    expect(r.left).toBe(1000 - 200 - 8); // viewport.width - bubble.width - MARGIN = 792
  });

  it('flips bottom→top when there is no room below', () => {
    const lowTrigger = { top: 760, left: 480, width: 40, height: 20, bottom: 790 };
    const r = computePosition(lowTrigger, bubble, 'bottom', viewport);
    expect(r.placement).toBe('top');
    expect(r.top).toBe(760 - 40 - 6);
  });
});
