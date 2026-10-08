import { describe, expect, it } from 'vitest';
import { popoverStyle } from '../src/lib/onboarding/tour-placement';

const rect = (top: number, bottom: number, left = 100, width = 200) =>
  ({
    top,
    bottom,
    left,
    right: left + width,
    width,
    height: bottom - top,
    x: left,
    y: top,
  }) as DOMRect;

describe('popoverStyle — below', () => {
  it('puts the card under the anchor when it fits', () => {
    expect(popoverStyle(rect(100, 150), 'below', 1280, 800)).toBe('top:162px; left:100px;');
  });

  it('flips the card above the anchor when only above fits', () => {
    expect(popoverStyle(rect(500, 700), 'below', 1280, 800)).toBe('bottom:312px; left:100px;');
  });

  // A tall anchor in a short window (the deps tour's Requires block at the 820×520 minimum): a
  // flip above would put the card's top off the window, its buttons out of reach. It is pinned
  // to the window's bottom edge instead, over the spotlight's lower part (spec §6.2).
  it('pins the card inside the window when it fits neither below nor above', () => {
    expect(popoverStyle(rect(120, 400), 'below', 820, 520)).toBe('bottom:16px; left:100px;');
  });

  // Found live (PR #496): the RU Requires step's card is 290 px tall, the budget 220. Judged by
  // the budget it "fitted" below a block ending at 480 in a 725 px window and its buttons were cut
  // off below the window. The card's own height decides the side once it is known.
  it("judges the side by the card's real height when it is known", () => {
    expect(popoverStyle(rect(425, 480, 274), 'below', 820, 725, 290)).toBe(
      'bottom:312px; left:274px;',
    );
    expect(popoverStyle(rect(425, 480, 274), 'below', 820, 725, 200)).toBe(
      'top:492px; left:274px;',
    );
  });

  // Found live too: the deps tour's anchors sit in a long list, and the user may scroll it during
  // the tour. The card followed its row off the top of the window, buttons and all.
  it('keeps the card inside the window when its anchor is scrolled above it', () => {
    expect(popoverStyle(rect(-1405, -1381, 292), 'below', 820, 725, 250)).toBe(
      'top:16px; left:292px;',
    );
  });

  it('keeps the card inside the right edge', () => {
    expect(popoverStyle(rect(100, 150, 700), 'below', 820, 520)).toBe('top:162px; left:484px;');
  });
});

describe('popoverStyle — no anchor', () => {
  it('centres the card', () => {
    expect(popoverStyle(null, 'below', 1280, 800)).toBe(
      'top:50%; left:50%; transform:translate(-50%,-50%);',
    );
  });
});

describe('popoverStyle — right', () => {
  it('keeps the card inside the window when its anchor is scrolled above it', () => {
    expect(popoverStyle(rect(-300, -260, 100, 100), 'right', 1280, 800, 250)).toBe(
      'top:16px; left:216px;',
    );
  });
});
