import { describe, expect, it } from 'vitest';
import { POSE_NAMES, resolvePose } from '$lib/accounts/skin-editor/poses';

const PARTS = ['head', 'body', 'rightArm', 'leftArm', 'rightLeg', 'leftLeg'] as const;

describe('poses', () => {
  it('exposes the four preset names', () => {
    expect(POSE_NAMES).toEqual(['default', 'tpose', 'walk', 'sit']);
  });

  it('default resolves to all-zero rotations', () => {
    const rot = resolvePose('default');
    for (const p of PARTS) {
      expect(rot[p]).toEqual({ x: 0, y: 0, z: 0 });
    }
  });

  it('resolves every part for every pose with finite angles in [-PI, PI]', () => {
    for (const name of POSE_NAMES) {
      const rot = resolvePose(name);
      for (const p of PARTS) {
        const r = rot[p];
        for (const axis of ['x', 'y', 'z'] as const) {
          expect(Number.isFinite(r[axis])).toBe(true);
          expect(Math.abs(r[axis])).toBeLessThanOrEqual(Math.PI);
        }
      }
    }
  });

  it('tpose rotates the arms and nothing else', () => {
    const rot = resolvePose('tpose');
    expect(rot.rightArm.z).not.toBe(0);
    expect(rot.leftArm.z).not.toBe(0);
    expect(rot.head).toEqual({ x: 0, y: 0, z: 0 });
  });
});
