import { describe, expect, it } from 'vitest';
import { CONTENT_KINDS, canInstallContent } from '$lib/mods/content-kind';

describe('canInstallContent', () => {
  it('mods require a non-vanilla selected instance', () => {
    expect(canInstallContent('mod', 'inst', 'fabric')).toBe(true);
    expect(canInstallContent('mod', 'inst', 'vanilla')).toBe(false);
    expect(canInstallContent('mod', null, 'fabric')).toBe(false);
  });

  it('resource packs and shaders only require a selected instance (vanilla OK)', () => {
    expect(canInstallContent('resource_pack', 'inst', 'vanilla')).toBe(true);
    expect(canInstallContent('shader', 'inst', 'vanilla')).toBe(true);
    expect(canInstallContent('shader', null, 'vanilla')).toBe(false);
  });

  it('exposes the three kinds in display order', () => {
    expect(CONTENT_KINDS).toEqual(['mod', 'resource_pack', 'shader']);
  });
});
