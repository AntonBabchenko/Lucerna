import { describe, expect, it } from 'vitest';
import { ndcFromPointer, uvToTexel } from '$lib/accounts/skin-editor/paint3d';

const rect = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height }) as DOMRect;

describe('paint3d math', () => {
  it('maps pointer to normalised device coords using the canvas rect', () => {
    const r = rect(0, 0, 200, 200);
    expect(ndcFromPointer(100, 100, r)).toEqual({ x: 0, y: 0 });
    expect(ndcFromPointer(0, 0, r)).toEqual({ x: -1, y: 1 });
    expect(ndcFromPointer(200, 200, r)).toEqual({ x: 1, y: -1 });
  });

  it('accounts for the canvas offset in the page', () => {
    const r = rect(50, 30, 100, 100);
    expect(ndcFromPointer(100, 80, r)).toEqual({ x: 0, y: 0 });
  });

  it('maps uv to a clamped texel with V flip', () => {
    // Default orientation py = floor((1-v)*64); verified empirically in T11.
    expect(uvToTexel({ x: 0, y: 1 })).toEqual({ x: 0, y: 0 });
    expect(uvToTexel({ x: 0.5, y: 0.5 })).toEqual({ x: 32, y: 32 });
    expect(uvToTexel({ x: 1, y: 0 })).toEqual({ x: 63, y: 63 }); // clamp uv.x*64=64 -> 63
    expect(uvToTexel({ x: 0.999, y: 0.001 })).toEqual({ x: 63, y: 63 });
  });
});
