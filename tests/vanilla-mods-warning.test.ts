import { describe, expect, it } from 'vitest';
import { shouldWarnVanillaWithMods } from '$lib/instances/import/vanilla-mods-warning';
import type { ContentEntry } from '$lib/ipc/bindings';

const mods = (n: number): ContentEntry => ({ category: 'mods', file_count: n, total_bytes: 1 });
const saves = (): ContentEntry => ({ category: 'saves', file_count: 1, total_bytes: 1 });

describe('shouldWarnVanillaWithMods', () => {
  it('warns when loader is vanilla and mods are present', () => {
    expect(shouldWarnVanillaWithMods('vanilla', [mods(12)])).toBe(true);
  });
  it('does not warn when a non-vanilla loader is selected', () => {
    expect(shouldWarnVanillaWithMods('fabric', [mods(12)])).toBe(false);
  });
  it('does not warn when there are no mods', () => {
    expect(shouldWarnVanillaWithMods('vanilla', [saves()])).toBe(false);
  });
  it('does not warn when the mods category is empty', () => {
    expect(shouldWarnVanillaWithMods('vanilla', [mods(0)])).toBe(false);
  });
});
