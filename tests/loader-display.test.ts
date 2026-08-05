import { describe, expect, it } from 'vitest';
import { displayLoaderTag } from '$lib/instances/loader-display';

describe('displayLoaderTag', () => {
  it('maps known platform tags to their display names', () => {
    expect(displayLoaderTag('fabric')).toBe('Fabric');
    expect(displayLoaderTag('neoforge')).toBe('NeoForge');
    expect(displayLoaderTag('quilt')).toBe('Quilt');
    expect(displayLoaderTag('forge')).toBe('Forge');
  });

  it('is case-insensitive — FTB targets are not guaranteed lowercase', () => {
    expect(displayLoaderTag('NeoForge')).toBe('NeoForge');
  });

  // An unrecognised tag is shown verbatim rather than guessed at or dropped:
  // a wrong loader name is worse than an unfamiliar one.
  it('passes an unknown tag through unchanged', () => {
    expect(displayLoaderTag('babric')).toBe('babric');
  });
});
