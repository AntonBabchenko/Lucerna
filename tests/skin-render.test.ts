import { describe, expect, it } from 'vitest';
import { compositeSize, frontLayout, SKIN_UV } from '../src/lib/accounts/skin-render';

describe('skin-render UV map', () => {
  it('head/body src rects match the 64x64 layout', () => {
    expect(SKIN_UV.head).toEqual({ sx: 8, sy: 8, sw: 8, sh: 8 });
    expect(SKIN_UV.body).toEqual({ sx: 20, sy: 20, sw: 8, sh: 12 });
  });

  it('classic arms are 4 wide, slim arms are 3 wide', () => {
    const classic = frontLayout('classic');
    const slim = frontLayout('slim');
    expect(classic.find((p) => p.name === 'rightArm')!.dst.w).toBe(4);
    expect(slim.find((p) => p.name === 'rightArm')!.dst.w).toBe(3);
  });

  it('composite width narrows for slim (arms flank the 8-wide body)', () => {
    expect(compositeSize('classic')).toEqual({ w: 16, h: 32 });
    expect(compositeSize('slim')).toEqual({ w: 14, h: 32 });
  });

  it('every part has both a src and a dst rect', () => {
    for (const p of frontLayout('classic')) {
      expect(p.src.sw).toBeGreaterThan(0);
      expect(p.dst.w).toBeGreaterThan(0);
    }
  });

  it('legacy 64x32 mirrors the right arm/leg into the left slots', () => {
    const legacy = frontLayout('classic', 32);
    expect(legacy.find((p) => p.name === 'leftArm')!.src).toEqual(SKIN_UV.rightArm);
    expect(legacy.find((p) => p.name === 'leftLeg')!.src).toEqual(SKIN_UV.rightLeg);
    const modern = frontLayout('classic', 64);
    expect(modern.find((p) => p.name === 'leftArm')!.src).toEqual(SKIN_UV.leftArm);
  });
});
