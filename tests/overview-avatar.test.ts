import { describe, expect, it } from 'vitest';
import { deriveAvatar } from '$lib/overview/avatar';

const base = { name: 'Skyblock', loader: 'fabric' as const, mrpack_source: null };

describe('deriveAvatar', () => {
  it('takes the uppercased first letter of the name', () => {
    expect(deriveAvatar(base).letter).toBe('S');
  });

  it('trims leading whitespace before taking the letter', () => {
    expect(deriveAvatar({ ...base, name: '  zen' }).letter).toBe('Z');
  });

  it('falls back to "?" for an empty name', () => {
    expect(deriveAvatar({ ...base, name: '' }).letter).toBe('?');
  });

  it('handles a unicode first character by codepoint', () => {
    expect(deriveAvatar({ ...base, name: '🌍 world' }).letter).toBe('🌍');
  });

  it('keys the tone off the loader for non-pack instances', () => {
    expect(deriveAvatar({ ...base, loader: 'neoforge' }).tone).toBe('neoforge');
  });

  it('keys the tone off the source for pack instances', () => {
    expect(deriveAvatar({ ...base, loader: 'forge', mrpack_source: 'modrinth' }).tone).toBe(
      'modrinth',
    );
  });
});
