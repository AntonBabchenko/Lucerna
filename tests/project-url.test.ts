import { describe, expect, it } from 'vitest';
import { modProjectUrl } from '../src/lib/mods/project-url';

describe('modProjectUrl', () => {
  it('builds a Modrinth mod URL from a slug', () => {
    expect(modProjectUrl('modrinth', 'sodium')).toBe('https://modrinth.com/mod/sodium');
  });

  it('builds a CurseForge mod URL from a slug', () => {
    expect(modProjectUrl('curseforge', 'jei')).toBe(
      'https://www.curseforge.com/minecraft/mc-mods/jei',
    );
  });

  it('falls back to the raw id when no slug is available', () => {
    expect(modProjectUrl('modrinth', 'AABBCCDD')).toBe('https://modrinth.com/mod/AABBCCDD');
  });
});
