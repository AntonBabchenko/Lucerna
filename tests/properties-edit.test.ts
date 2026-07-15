// tests/properties-edit.test.ts
import { describe, expect, it } from 'vitest';
import { buildPropertiesText, getProperty } from '$lib/servers/properties-edit';

describe('buildPropertiesText', () => {
  it('omits an absent key left at its default', () => {
    const out = buildPropertiesText('', { 'view-distance': '10' }, { 'view-distance': '10' });
    expect(getProperty(out, 'view-distance')).toBeNull();
  });

  it('writes an absent key changed from its default', () => {
    const out = buildPropertiesText('', { 'view-distance': '12' }, { 'view-distance': '10' });
    expect(getProperty(out, 'view-distance')).toBe('12');
  });

  it('rewrites a present key even when equal to default', () => {
    const raw = 'view-distance=10\n';
    const out = buildPropertiesText(raw, { 'view-distance': '10' }, { 'view-distance': '10' });
    expect(getProperty(out, 'view-distance')).toBe('10');
  });

  it('preserves unknown keys and comments and order', () => {
    const raw = '#header\nmotd=Hi\ncustom-key=42\n';
    const out = buildPropertiesText(
      raw,
      { motd: 'Bye', 'custom-key': '7' },
      { motd: 'A Minecraft Server' },
    );
    expect(out.startsWith('#header\n')).toBe(true);
    expect(getProperty(out, 'motd')).toBe('Bye');
    expect(getProperty(out, 'custom-key')).toBe('7');
  });

  it('leaves a raw key that is absent from values untouched', () => {
    const raw = 'motd=Hi\nkeep-me=untouched\n';
    const out = buildPropertiesText(raw, { motd: 'Bye' }, { motd: 'A Minecraft Server' });
    expect(getProperty(out, 'keep-me')).toBe('untouched');
    expect(getProperty(out, 'motd')).toBe('Bye');
  });
});
