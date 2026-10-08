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
