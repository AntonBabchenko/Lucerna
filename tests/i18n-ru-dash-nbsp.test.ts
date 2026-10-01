import { describe, expect, it } from 'vitest';
import ru from '../src/lib/i18n/locales/ru.json';

// Plan §5d L3 (screenshot 07b): a line of the Play gate began with «—» — «…, но ниже 1.21 /
// — в профиле 1.21.1». In Russian a dash never starts a line: the space before it is a no-break
// one. Pinned for the templates the gate renders: its rows (`mods.preflight.*`, with the ranges of
// `mods.range.*` inside them) and the reasons a failed repair gives (a busy profile, the network,
// the other refusals a mod install or switch can meet).
const GATE_REASON_KEYS = [
  'mods.ops.busy',
  'errors.network',
  'errors.modsNetwork',
  'errors.modsPlatformAuthInvalid',
  'errors.modsPlatformAuthMissing',
  'errors.modVersionNotForInstance',
  'errors.modsSha1Mismatch',
  'errors.modsFilenameConflict',
];

type Json = { [k: string]: string | Json };

function flatten(obj: Json, prefix = ''): [string, string][] {
  return Object.entries(obj).flatMap(([k, v]) => {
    const path = prefix ? `${prefix}.${k}` : k;
    return typeof v === 'string' ? [[path, v] as [string, string]] : flatten(v, path);
  });
}

describe('Russian dashes in the Play gate', () => {
  const all = flatten(ru as Json);
  const inGate = all.filter(
    ([k]) =>
      k.startsWith('mods.preflight.') ||
      k.startsWith('mods.range.') ||
      GATE_REASON_KEYS.includes(k),
  );

  it('covers every key it names', () => {
    const keys = new Set(all.map(([k]) => k));
    expect(GATE_REASON_KEYS.filter((k) => !keys.has(k))).toEqual([]);
  });

  it('never lets a line start with «—»: the space before it does not break', () => {
    const breakable = inGate.filter(([, v]) => /[ \t\n]—/.test(v)).map(([k]) => k);
    expect(breakable).toEqual([]);
  });

  it('still has its dashes, each after a no-break space', () => {
    const withDash = inGate.filter(([, v]) => v.includes('—'));
    expect(withDash.length).toBeGreaterThan(0);
    for (const [k, v] of withDash) expect({ k, ok: /(^| )—/.test(v) }).toEqual({ k, ok: true });
  });
});
