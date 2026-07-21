// Static, paintable pose presets for the 3D editor model. Each pose is a set of
// fixed limb rotations (radians) applied to skinview3d's BodyPart groups with
// viewer.animation left null — the model holds the pose so it stays paintable.
// Angles are approximate and tuned visually; keep them within [-PI, PI].
import type { Part } from './atlas';

export type PoseName = 'default' | 'tpose' | 'walk' | 'sit';

export interface LimbRotation {
  x: number;
  y: number;
  z: number;
}

type Pose = Partial<Record<Part, Partial<LimbRotation>>>;

const HALF_PI = Math.PI / 2;

const POSES: Record<PoseName, Pose> = {
  default: {},
  tpose: {
    rightArm: { z: -HALF_PI },
    leftArm: { z: HALF_PI },
  },
  walk: {
    rightArm: { x: 0.5 },
    leftArm: { x: -0.5 },
    rightLeg: { x: -0.5 },
    leftLeg: { x: 0.5 },
  },
  sit: {
    rightLeg: { x: -HALF_PI },
    leftLeg: { x: -HALF_PI },
  },
};

export const POSE_NAMES: PoseName[] = ['default', 'tpose', 'walk', 'sit'];

const PARTS: Part[] = ['head', 'body', 'rightArm', 'leftArm', 'rightLeg', 'leftLeg'];

/** Full rotation for every limb under `name`, defaulting each axis to 0. */
export function resolvePose(name: PoseName): Record<Part, LimbRotation> {
  const pose = POSES[name];
  const out = {} as Record<Part, LimbRotation>;
  for (const part of PARTS) {
    const r = pose[part] ?? {};
    out[part] = { x: r.x ?? 0, y: r.y ?? 0, z: r.z ?? 0 };
  }
  return out;
}
