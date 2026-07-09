import { describe, expect, it } from 'vitest';
import { capeFrontRect } from '../src/lib/accounts/cape-render';

describe('capeFrontRect', () => {
  it('returns the standard front-face crop for a 64x32 cape', () => {
    expect(capeFrontRect(64, 32)).toEqual({ sx: 1, sy: 1, sw: 10, sh: 16 });
  });

  it('scales the crop for a 2x HD cape (128x64)', () => {
    expect(capeFrontRect(128, 64)).toEqual({ sx: 2, sy: 2, sw: 20, sh: 32 });
  });

  it('returns null for a non-cape aspect ratio', () => {
    expect(capeFrontRect(50, 50)).toBeNull();
  });
});
