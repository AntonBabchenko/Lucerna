import { describe, expect, it } from 'vitest';
import { nameKey } from '$lib/mods/name-match';

describe('nameKey', () => {
  it('lowercases the input', () => {
    expect(nameKey('Sodium')).toBe('sodium');
  });

  it('strips loader noise words', () => {
    // "Cloth Config API (Forge)" and "Cloth Config Fabric Edition" should
    // both reduce to the same key as "Cloth Config".
    const base = nameKey('Cloth Config');
    expect(nameKey('Cloth Config API (Forge)')).toBe(base);
    expect(nameKey('Cloth Config Fabric Edition')).toBe(base);
  });

  it('strips all listed noise words', () => {
    // Each noise word individually must be stripped.
    expect(nameKey('Foo API')).toBe('foo');
    expect(nameKey('Foo Fabric')).toBe('foo');
    expect(nameKey('Foo Forge')).toBe('foo');
    expect(nameKey('Foo NeoForge')).toBe('foo');
    expect(nameKey('Foo Quilt')).toBe('foo');
    expect(nameKey('Foo Mod')).toBe('foo');
    expect(nameKey('Foo Edition')).toBe('foo');
    expect(nameKey('Foo for Bar')).toBe('foobar');
  });

  it('removes non-alphanumeric characters', () => {
    expect(nameKey('Hello, World!')).toBe('helloworld');
    expect(nameKey('A (B) [C]')).toBe('abc');
  });

  it('returns empty string for a name that collapses to nothing', () => {
    // "Forge API" → noise words removed → '' → no alphanumerics remain
    expect(nameKey('Forge API')).toBe('');
  });

  it('returns empty string for an already-empty name', () => {
    expect(nameKey('')).toBe('');
  });

  it('treats noise words as whole words only (not substrings)', () => {
    // "Modernity" contains "mod" but is not the word "mod".
    expect(nameKey('Modernity')).toBe('modernity');
    // "Forgery" contains "forge" but is not the word "forge".
    expect(nameKey('Forgery')).toBe('forgery');
  });
});
