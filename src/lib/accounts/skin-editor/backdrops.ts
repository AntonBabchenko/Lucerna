// Procedural pixel-art panoramas for the skin editor's backgrounds: plains, the Nether and a
// stronghold room. Every texel of an equirectangular image is a ray from the eye (1.62 blocks
// above the floor, as in the game) into a tiny scene — floor and ceiling planes, a round wall,
// sky, billboards — and the surface it hits is painted from 16×16 block textures drawn here.
// Nothing is a Mojang asset: the art is ours, drawn by code, so it ships under the project's
// licence. Pure and deterministic (fixed seeds, no DOM), so it is unit-testable; the editor
// turns the pixels into a canvas for skinview3d's loadPanorama().
//
// Mapping matches three.js's equirectangular lookup: u = atan2(z, x) / 2π + 0.5,
// v = asin(y) / π + 0.5, row 0 at the top. The editor's camera looks along −z, so the view
// behind the model is centred on θ = −π/2.

export type BackdropKind = 'plains' | 'nether' | 'stronghold';
export type Backdrop = { width: number; height: number; data: Uint8ClampedArray };

type RGB = readonly [number, number, number];
type Hit = { color: RGB; dist: number };

const EYE = 1.62;
const FRONT = -Math.PI / 2;

// --- deterministic noise ----------------------------------------------------------------

function hash(a: number, b: number, c: number): number {
  let h =
    Math.imul(a | 0, 0x27d4eb2d) ^ Math.imul(b | 0, 0x165667b1) ^ Math.imul(c | 0, 0x2545f491);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

function valueNoise(x: number, y: number, seed: number): number {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const xf = x - xi;
  const yf = y - yi;
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const a = hash(xi, yi, seed);
  const b = hash(xi + 1, yi, seed);
  const c = hash(xi, yi + 1, seed);
  const d = hash(xi + 1, yi + 1, seed);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

function fbm(x: number, y: number, seed: number): number {
  return (
    (valueNoise(x, y, seed) * 4 +
      valueNoise(x * 2, y * 2, seed + 1) * 2 +
      valueNoise(x * 4, y * 4, seed + 2)) /
    7
  );
}

/** Noise around the horizon circle, so a profile drawn from it has no seam at θ = ±π. */
function ringNoise(theta: number, freq: number, seed: number): number {
  return fbm(Math.cos(theta) * freq + 7, Math.sin(theta) * freq + 7, seed);
}

/** A deterministic sequence of uniforms for placing billboards. */
function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// --- colour ---------------------------------------------------------------------------

const clamp01 = (x: number) => (x < 0 ? 0 : x > 1 ? 1 : x);
const smooth = (e0: number, e1: number, x: number) => {
  const t = clamp01((x - e0) / (e1 - e0));
  return t * t * (3 - 2 * t);
};
const mix = (a: RGB, b: RGB, t: number): RGB => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];
const tint = (a: RGB, k: RGB): RGB => [a[0] * k[0], a[1] * k[1], a[2] * k[2]];
const shade = (a: RGB, k: number): RGB => [a[0] * k, a[1] * k, a[2] * k];
const pick = (pal: readonly RGB[], r: number): RGB =>
  pal[Math.min(pal.length - 1, Math.floor(r * pal.length))];
const angleDiff = (a: number, b: number) => {
  let d = a - b;
  while (d > Math.PI) d -= 2 * Math.PI;
  while (d < -Math.PI) d += 2 * Math.PI;
  return d;
};
/** Integer block and the 0..15 texture pixel inside it, for a world coordinate. */
const cell = (w: number): [number, number] => {
  const b = Math.floor(w);
  return [b, Math.min(15, Math.floor((w - b) * 16))];
};

// --- block textures (px, py: 0..15, py = 0 at the top of the face) ----------------------

const GRASS: readonly RGB[] = [
  [112, 164, 66],
  [119, 172, 70],
  [125, 178, 74],
  [131, 184, 79],
];
function grassTop(bx: number, bz: number, px: number, pz: number, near: boolean): RGB {
  const flower = hash(bx, bz, 12);
  if (!near && flower < 0.045 && px >= 7 && px <= 8 && pz >= 7 && pz <= 8) {
    const centre = (px === 7 || px === 8) && (pz === 7 || pz === 8);
    if (hash(bx, bz, 13) < 0.5) return centre ? [74, 40, 22] : [206, 42, 40];
    return centre ? [232, 168, 30] : [250, 222, 58];
  }
  const clump = valueNoise((bx * 16 + px) / 2.5, (bz * 16 + pz) / 2.5, 14);
  const c = pick(GRASS, clamp01(clump * 0.6 + hash(bx * 16 + px, bz * 16 + pz, 11) * 0.5));
  return shade(c, 0.95 + hash(bx, bz, 15) * 0.1);
}

const STONE: readonly RGB[] = [
  [112, 112, 114],
  [119, 119, 121],
  [125, 125, 127],
  [131, 131, 133],
];
const MOSS: readonly RGB[] = [
  [78, 98, 54],
  [90, 112, 60],
  [102, 124, 66],
];
/** Stone bricks: two courses per block, the lower one offset by half a block. */
function stoneBricks(
  variant: 'plain' | 'mossy' | 'cracked',
  bx: number,
  by: number,
  px: number,
  py: number,
): RGB {
  const course = py < 8 ? 0 : 1;
  const ly = py & 7;
  const lx = course === 0 ? px : (px + 8) & 15;
  if (ly === 7 || lx === 15)
    return pick(
      [
        [82, 82, 84],
        [88, 88, 90],
      ] as const,
      hash(px, py, bx * 31 + by),
    );
  let c = pick(STONE, hash(bx * 16 + px, by * 16 + py, 21));
  if (ly === 0 || lx === 0) c = shade(c, 1.12);
  else if (ly === 6 || lx === 14) c = shade(c, 0.9);
  if (variant === 'mossy') {
    const reach = 0.18 + (py / 15) * 0.42;
    if (valueNoise((bx * 16 + px) / 3, (by * 16 + py) / 3, 23) < reach) {
      return pick(MOSS, hash(bx * 16 + px, by * 16 + py, 24));
    }
  }
  if (variant === 'cracked') {
    let x = 3 + Math.floor(hash(bx, by, 25) * 10);
    for (let y = 0; y <= py; y += 1) {
      if (y === py && (x === px || (hash(x, y, bx + by) < 0.3 && x + 1 === px)))
        return [70, 70, 72];
      x += Math.floor(hash(bx * 7 + y, by, 26) * 3) - 1;
    }
  }
  return c;
}

const NETHERRACK: readonly RGB[] = [
  [74, 26, 26],
  [88, 32, 32],
  [102, 42, 40],
  [116, 52, 48],
  [128, 62, 56],
];
function netherrack(bx: number, by: number, px: number, py: number): RGB {
  const clump = valueNoise((bx * 16 + px) / 2.2, (by * 16 + py) / 2.2, 31);
  return pick(NETHERRACK, clamp01(clump * 0.75 + hash(bx * 16 + px, by * 16 + py, 32) * 0.35));
}

const GLOWSTONE: readonly RGB[] = [
  [168, 104, 48],
  [214, 146, 66],
  [246, 196, 104],
  [255, 232, 160],
];
function glowstone(bx: number, by: number, px: number, py: number): RGB {
  const clump = valueNoise((bx * 16 + px) / 2, (by * 16 + py) / 2, 41);
  return pick(GLOWSTONE, clamp01(clump * 0.8 + hash(bx * 16 + px, by * 16 + py, 42) * 0.3));
}

/** Lava, sampled per texture pixel in world units so pools and falls read as one liquid. */
function lava(wx: number, wy: number): RGB {
  const qx = Math.floor(wx * 16) / 16;
  const qy = Math.floor(wy * 16) / 16;
  const n = fbm(qx * 0.9, qy * 0.9, 51);
  if (n < 0.42) return [190, 66, 12];
  if (n < 0.55) return [222, 98, 18];
  if (n < 0.68) return [246, 140, 28];
  return [255, 190, 64];
}

const LOG: readonly RGB[] = [
  [82, 62, 36],
  [96, 74, 44],
  [108, 84, 50],
  [120, 94, 58],
];
function oakLog(seed: number, px: number, py: number): RGB {
  const stripe = hash(px, seed, 61);
  return pick(LOG, clamp01(stripe * 0.7 + hash(px, py + seed * 16, 62) * 0.35));
}

const LEAVES: readonly RGB[] = [
  [46, 92, 30],
  [58, 112, 38],
  [70, 132, 44],
  [84, 150, 52],
];
function oakLeaves(bx: number, by: number, px: number, py: number): RGB {
  const clump = valueNoise((bx * 16 + px) / 2.4, (by * 16 + py) / 2.4, 71);
  if (clump < 0.2) return [32, 66, 22];
  const c = pick(
    LEAVES,
    clamp01((clump - 0.2) * 1.1 + hash(bx * 16 + px, by * 16 + py, 72) * 0.25),
  );
  return py > 11 ? shade(c, 0.9) : c;
}

// --- plains -----------------------------------------------------------------------------

const SKY_HORIZON: RGB = [192, 216, 255];
const SKY_ZENITH: RGB = [112, 160, 255];
const PLAINS_HAZE: RGB = [184, 210, 238];
const CLOUD_HEIGHT = 64;
const SUN = unit(FRONT + 0.46, 0.36);

function unit(theta: number, phi: number): [number, number, number] {
  return [Math.cos(phi) * Math.cos(theta), Math.sin(phi), Math.cos(phi) * Math.sin(theta)];
}

type Tree = { theta: number; dist: number; halfWidth: number; id: number };
const TREES: Tree[] = (() => {
  const r = rng(7);
  const out: Tree[] = [];
  for (let id = 0; id < 34; id += 1) {
    const theta = r() * 2 * Math.PI - Math.PI;
    let dist = 10 + 60 * r() * r();
    // Keep the trees right behind the model a little back, so its silhouette stays clear.
    if (Math.abs(angleDiff(theta, FRONT)) < 0.32 && dist < 22) dist += 14;
    out.push({ theta, dist, halfWidth: Math.atan(2.8 / dist), id });
  }
  return out.sort((a, b) => a.dist - b.dist);
})();

/** An oak seen side-on, in block units from its base: a trunk and a two-step crown. */
function oakAt(id: number, x: number, y: number): RGB | null {
  if (y < 0 || y >= 7) return null;
  const ax = Math.abs(x);
  const by = Math.floor(y);
  const bx = Math.floor(x + 2.5);
  if (by >= 3 && by < 5 && ax < 2.5) {
    const corner =
      ax >= 1.5 && (by === 4 || hash(bx, by, id) < 0.35) && hash(bx, by, id + 9) < 0.55;
    if (!corner) return oakLeaves(bx + id * 5, by, cell(x + 2.5)[1], 15 - cell(y)[1]);
  }
  if (by >= 5 && by < 7 && ax < 1.5) {
    const corner = ax >= 0.5 && by === 6 && hash(bx, by, id) < 0.6;
    if (!corner) return shade(oakLeaves(bx + id * 5, by, cell(x + 2.5)[1], 15 - cell(y)[1]), 1.06);
  }
  if (ax < 0.5 && by < 5) return oakLog(id, cell(x + 0.5)[1], 15 - cell(y)[1]);
  return null;
}

function plainsTrees(theta: number, phi: number): Hit | null {
  // The nearest tree (10 blocks off, 7 tall) spans about −0.16..0.49 rad.
  if (phi > 0.5 || phi < -0.17) return null;
  for (const t of TREES) {
    const d = angleDiff(theta, t.theta);
    if (Math.abs(d) > t.halfWidth) continue;
    const lateral = t.dist * Math.tan(d);
    const height = (t.dist * Math.tan(phi)) / Math.cos(d) + EYE;
    const c = oakAt(t.id, lateral, height);
    if (c) return { color: c, dist: t.dist };
  }
  return null;
}

/** Blocky hills on the horizon: a far hazy range and a nearer one with tree crowns. */
function plainsHills(theta: number, phi: number): RGB | null {
  if (phi < -0.012 || phi > 0.09) return null;
  const col = Math.floor(((theta + Math.PI) / (2 * Math.PI)) * 900);
  const step = 0.0045;
  const q = (e: number) => Math.round(e / step) * step;
  const near = q(0.006 + 0.05 * (ringNoise(theta, 3.2, 81) - 0.42));
  const crown = hash(col >> 1, 0, 82) < 0.28 ? 0.009 : 0;
  if (phi <= near + crown && near + crown > 0) {
    const base: RGB = phi > near ? [64, 116, 46] : [86, 138, 62];
    return mix(shade(base, 0.94 + hash(col, Math.floor(phi / step), 83) * 0.12), PLAINS_HAZE, 0.38);
  }
  const far = q(0.012 + 0.07 * (ringNoise(theta + 1.3, 2.1, 84) - 0.38));
  if (phi <= far) {
    return mix(
      shade([104, 146, 96], 0.95 + hash(col, Math.floor(phi / step), 85) * 0.1),
      PLAINS_HAZE,
      0.62,
    );
  }
  return null;
}

function plains(dx: number, dy: number, dz: number, theta: number, phi: number): RGB {
  const tree = plainsTrees(theta, phi);
  if (tree) return mix(tree.color, PLAINS_HAZE, smooth(16, 120, tree.dist));
  const hills = plainsHills(theta, phi);
  if (hills) return hills;
  if (dy < 0) {
    const t = EYE / -dy;
    const [bx, px] = cell(dx * t);
    const [bz, pz] = cell(dz * t);
    const dist = t * Math.cos(phi);
    return mix(grassTop(bx, bz, px, pz, dist < 7), PLAINS_HAZE, smooth(14, 130, dist));
  }
  let sky = mix(SKY_HORIZON, SKY_ZENITH, smooth(0, 0.9, phi) ** 0.8);
  // The sun: a square facing the eye, with a soft halo.
  const s = SUN;
  const c = dx * s[0] + dy * s[1] + dz * s[2];
  if (c > 0.9) {
    const rx = -s[2];
    const rz = s[0];
    const rn = Math.hypot(rx, rz);
    const a = (dx * rx + dz * rz) / rn / c;
    const ux = s[1] * rz;
    const uy = s[2] * rx - s[0] * rz;
    const uz = -s[1] * rx;
    const b = (dx * ux + dy * uy + dz * uz) / Math.hypot(ux, uy, uz) / c;
    const m = Math.max(Math.abs(a), Math.abs(b));
    if (m < 0.032) return [255, 255, 236];
    if (m < 0.05) return [255, 246, 176];
    sky = mix(sky, [255, 250, 220], 0.35 * (1 - smooth(0.05, 0.16, m)));
  }
  if (dy > 0.012) {
    const t = CLOUD_HEIGHT / dy;
    const cx = Math.floor((dx * t) / 12);
    const cz = Math.floor((dz * t) / 12);
    const cloud = (x: number, z: number) => valueNoise(x * 0.42, z * 0.42, 91) > 0.62;
    if (cloud(cx, cz)) {
      const face: RGB = cloud(cx, cz + 1) ? [255, 255, 255] : [234, 240, 250];
      return mix(face, sky, smooth(260, 900, t * Math.cos(phi)));
    }
  }
  return sky;
}

// --- nether -----------------------------------------------------------------------------

const NETHER_FOG: RGB = [84, 22, 16];
const NETHER_CEILING = 11;

type Column = {
  theta: number;
  dist: number;
  halfWidth: number;
  width: number;
  kind: 'rock' | 'lava';
  id: number;
};
const COLUMNS: Column[] = (() => {
  const r = rng(13);
  const out: Column[] = [];
  for (let id = 0; id < 12; id += 1) {
    const kind = id % 3 === 2 ? 'lava' : 'rock';
    const width = kind === 'lava' ? 1 + Math.floor(r() * 2) : 3 + Math.floor(r() * 4);
    const theta = r() * 2 * Math.PI - Math.PI;
    let dist = 12 + 34 * r();
    if (Math.abs(angleDiff(theta, FRONT)) < 0.3 && dist < 20) dist += 12;
    out.push({ theta, dist, halfWidth: Math.atan((width / 2 + 0.2) / dist), width, kind, id });
  }
  return out.sort((a, b) => a.dist - b.dist);
})();

function netherColumns(theta: number, phi: number): Hit | null {
  for (const c of COLUMNS) {
    const d = angleDiff(theta, c.theta);
    if (Math.abs(d) > c.halfWidth) continue;
    const lateral = c.dist * Math.tan(d);
    if (Math.abs(lateral) >= c.width / 2) continue;
    const y = (c.dist * Math.tan(phi)) / Math.cos(d);
    if (y <= -EYE || y >= NETHER_CEILING) continue;
    const wx = lateral + c.width / 2 + c.id * 13;
    if (c.kind === 'lava') return { color: lava(wx, y * 0.5 - 0.0), dist: c.dist * 0.4 };
    const [bx, px] = cell(wx);
    const [by, py] = cell(y + EYE);
    const glow = y > NETHER_CEILING - 3 && valueNoise(bx * 0.7, by * 0.7, 101 + c.id) > 0.55;
    const rock = glow
      ? glowstone(bx, by, px, 15 - py)
      : shade(netherrack(bx, by, px, 15 - py), 0.92);
    return { color: rock, dist: glow ? c.dist * 0.45 : c.dist };
  }
  return null;
}

/** Far cave walls: a blocky netherrack ridge around the horizon, deep in the haze. */
function netherCliffs(theta: number, phi: number): RGB | null {
  if (phi < -0.03 || phi > 0.34) return null;
  const step = 0.011;
  const col = Math.floor(((theta + Math.PI) / (2 * Math.PI)) * 700);
  const colTheta = ((Math.floor(col / 6) * 6 + 3) / 700) * 2 * Math.PI - Math.PI;
  const ridge = Math.round((0.05 + 0.26 * ringNoise(colTheta, 2.6, 141) ** 1.6) / step) * step;
  if (phi > ridge) return null;
  const row = Math.floor(phi / step);
  const rock = netherrack(col >> 2, row >> 2, (col & 3) * 4, (row & 3) * 4);
  const fall = hash(col >> 3, 0, 142) < 0.06;
  return mix(fall ? lava(col / 6, row / 3) : rock, NETHER_FOG, fall ? 0.35 : 0.55);
}

function nether(dx: number, dy: number, dz: number, theta: number, phi: number): RGB {
  const column = netherColumns(theta, phi);
  if (column) return mix(column.color, NETHER_FOG, smooth(4, 70, column.dist));
  const cliff = netherCliffs(theta, phi);
  if (cliff) return cliff;
  if (Math.abs(dy) < 1e-4) return NETHER_FOG;
  const floor = dy < 0;
  const t = floor ? EYE / -dy : NETHER_CEILING / dy;
  const wx = dx * t;
  const wz = dz * t;
  const dist = t * Math.cos(phi);
  const [bx, px] = cell(wx);
  const [bz, pz] = cell(wz);
  if (floor) {
    const pool = fbm(bx * 0.06, bz * 0.06, 111);
    // Solid netherrack underfoot: the pools start a few blocks out.
    if (pool > 0.6 + 0.3 * (1 - smooth(6, 12, dist)))
      return mix(lava(wx, wz), NETHER_FOG, smooth(20, 110, dist));
    const rim = smooth(0.5, 0.6, pool);
    const rock = mix(shade(netherrack(bx, bz, px, pz), 0.82), [150, 64, 34], rim * 0.5);
    return mix(rock, NETHER_FOG, smooth(4, 60, dist));
  }
  const glow = fbm(bx * 0.2, bz * 0.2, 121) > 0.7;
  const roof = glow ? glowstone(bx, bz, px, pz) : shade(netherrack(bx, bz, px, pz), 0.66);
  return mix(roof, NETHER_FOG, smooth(4, glow ? 90 : 55, dist));
}

// --- stronghold -------------------------------------------------------------------------

const ROOM_CIRCUMFERENCE = 40; // whole blocks, so the brickwork closes without a seam
const ROOM_RADIUS = ROOM_CIRCUMFERENCE / (2 * Math.PI);
const ROOM_FLOOR = -EYE;
const ROOM_CEILING = 6 - EYE;
const DOORS = [0, 20];
const WINDOW = 10; // the iron-barred window behind the model
/** Wall coordinate in blocks around the room; block 10 is centred right behind the model. */
const wallU = (theta: number) =>
  (((theta + Math.PI) / (2 * Math.PI)) * ROOM_CIRCUMFERENCE + 0.5) % ROOM_CIRCUMFERENCE;
const wallTheta = (u: number) => ((u - 0.5) / ROOM_CIRCUMFERENCE) * 2 * Math.PI - Math.PI;
/** Blocks between two wall columns, the short way round. */
const wallGap = (a: number, b: number) => {
  const d = (((a - b) % ROOM_CIRCUMFERENCE) + ROOM_CIRCUMFERENCE) % ROOM_CIRCUMFERENCE;
  return Math.min(d, ROOM_CIRCUMFERENCE - d);
};
const TORCHES = [7, 13, 27, 33].map((u) => {
  const a = wallTheta(u + 0.5);
  const r = ROOM_RADIUS - 0.4;
  return { u, x: Math.cos(a) * r, y: ROOM_FLOOR + 2.6, z: Math.sin(a) * r };
});
const WARM: RGB = [1, 0.8, 0.55];

function torchLight(x: number, y: number, z: number): RGB {
  let warm = 0;
  for (const t of TORCHES) {
    const d2 = (x - t.x) ** 2 + (y - t.y) ** 2 + (z - t.z) ** 2;
    warm += 0.75 / (1 + d2 * 0.35);
  }
  const ambient = 0.62;
  return [ambient + warm * WARM[0], ambient + warm * WARM[1], ambient + warm * WARM[2]];
}

function brickVariant(bx: number, by: number): 'plain' | 'mossy' | 'cracked' {
  const r = hash(bx, by, 131);
  if (r < (by === 0 ? 0.3 : 0.14)) return 'mossy';
  if (r > 0.84) return 'cracked';
  return 'plain';
}

function torchSprite(px: number, py: number): RGB | null {
  if ((px === 7 || px === 8) && py >= 6 && py <= 15)
    return px === 7 ? [128, 96, 54] : [100, 74, 40];
  if ((px === 7 || px === 8) && (py === 4 || py === 5)) return [255, 196, 72];
  if ((px === 7 || px === 8) && py === 3) return [255, 244, 180];
  return null;
}

/** A corridor leaving the room through a doorway: stone bricks that fade into the dark. */
function corridor(dx: number, dy: number, dz: number, door: number): RGB {
  const a = wallTheta(door + 0.5);
  const nx = Math.cos(a);
  const nz = Math.sin(a);
  const along = dx * nx + dz * nz;
  const across = -dx * nz + dz * nx;
  let t = Math.abs(across) > 1e-6 ? 1.5 / Math.abs(across) : Number.POSITIVE_INFINITY;
  let surface: 'side' | 'floor' | 'roof' = 'side';
  if (dy < 0 && ROOM_FLOOR / dy < t) {
    t = ROOM_FLOOR / dy;
    surface = 'floor';
  } else if (dy > 0 && (ROOM_FLOOR + 3) / dy < t) {
    t = (ROOM_FLOOR + 3) / dy;
    surface = 'roof';
  }
  const depth = t * along - ROOM_RADIUS;
  if (!Number.isFinite(t) || depth > 26) return [8, 8, 10];
  const [bu, px] = cell(t * along);
  let c: RGB;
  if (surface === 'side') {
    const [bv, fy] = cell(t * dy - ROOM_FLOOR);
    c = stoneBricks(brickVariant(bu + door * 3, bv), bu, bv, px, 15 - fy);
  } else {
    const [bw, pw] = cell(t * across + 1.5);
    c = shade(
      stoneBricks(brickVariant(bu, bw + 90), bu, bw, px, pw),
      surface === 'roof' ? 0.75 : 1,
    );
  }
  return shade(c, 0.62 * Math.exp(-Math.max(0, depth) * 0.2));
}

function strongholdWall(dx: number, dy: number, dz: number, theta: number, y: number): RGB {
  const u = wallU(theta);
  const [bu, px] = cell(u);
  const [bv, fy] = cell(y - ROOM_FLOOR);
  const py = 15 - fy;
  const x = Math.cos(theta) * ROOM_RADIUS;
  const z = Math.sin(theta) * ROOM_RADIUS;
  for (const door of DOORS) {
    if (wallGap(bu, door) <= 1 && bv <= 2) return corridor(dx, dy, dz, door);
  }
  if (wallGap(bu, WINDOW) <= 1 && (bv === 1 || bv === 2)) {
    const bar = px % 4 === 1 || px % 4 === 2 || (bv === 2 && py <= 1) || (bv === 1 && py >= 14);
    if (!bar) return [16, 16, 18];
    return tint(px % 4 === 1 ? [150, 150, 154] : [92, 92, 96], torchLight(x, y, z));
  }
  if (bv === 2) {
    for (const t of TORCHES) {
      if (t.u === bu) {
        const sprite = torchSprite(px, py);
        if (sprite) return sprite;
      }
    }
  }
  return tint(stoneBricks(brickVariant(bu, bv), bu, bv, px, py), torchLight(x, y, z));
}

function stronghold(dx: number, dy: number, dz: number, theta: number, phi: number): RGB {
  const wallY = ROOM_RADIUS * Math.tan(phi);
  if (wallY > ROOM_FLOOR && wallY < ROOM_CEILING) return strongholdWall(dx, dy, dz, theta, wallY);
  const floor = dy < 0;
  const y = floor ? ROOM_FLOOR : ROOM_CEILING;
  const t = y / dy;
  const wx = dx * t;
  const wz = dz * t;
  const [bx, px] = cell(wx);
  const [bz, pz] = cell(wz);
  const brick = stoneBricks(brickVariant(bx, bz + 50), bx, bz, px, pz);
  return tint(shade(brick, floor ? 0.92 : 0.7), torchLight(wx, y, wz));
}

// --- render -----------------------------------------------------------------------------

type Scene = (dx: number, dy: number, dz: number, theta: number, phi: number) => RGB;
const SCENES: Record<BackdropKind, Scene> = { plains, nether, stronghold };

export const BACKDROP_KINDS: readonly BackdropKind[] = ['plains', 'nether', 'stronghold'];

/** Paint rows [y0, y1) of a width × height panorama into `data` (RGBA, the whole image). The
 *  editor calls it in slices and yields between them, so a render never freezes the window. */
export function renderBackdropRows(
  kind: BackdropKind,
  width: number,
  height: number,
  y0: number,
  y1: number,
  data: Uint8ClampedArray,
): void {
  const scene = SCENES[kind];
  for (let y = y0; y < y1; y += 1) {
    const phi = (0.5 - (y + 0.5) / height) * Math.PI;
    const cp = Math.cos(phi);
    const sp = Math.sin(phi);
    for (let x = 0; x < width; x += 1) {
      const theta = ((x + 0.5) / width - 0.5) * 2 * Math.PI;
      const c = scene(cp * Math.cos(theta), sp, cp * Math.sin(theta), theta, phi);
      const i = (y * width + x) * 4;
      data[i] = c[0];
      data[i + 1] = c[1];
      data[i + 2] = c[2];
      data[i + 3] = 255;
    }
  }
}

export function renderBackdrop(kind: BackdropKind, width = 2048, height = 1024): Backdrop {
  const data = new Uint8ClampedArray(width * height * 4);
  renderBackdropRows(kind, width, height, 0, height, data);
  return { width, height, data };
}
