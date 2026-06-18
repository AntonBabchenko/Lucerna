import { describe, expect, it } from 'vitest';
import { getProperty, setProperty } from '$lib/servers/properties-edit';

describe('properties-edit', () => {
  it('reads a value', () => {
    expect(getProperty('motd=Hi\nserver-port=25565\n', 'server-port')).toBe('25565');
  });

  it('returns null when absent', () => {
    expect(getProperty('motd=Hi\n', 'pvp')).toBeNull();
  });

  it('replaces in place', () => {
    expect(setProperty('motd=Hi\npvp=true\n', 'pvp', 'false')).toBe('motd=Hi\npvp=false\n');
  });

  it('appends when absent', () => {
    expect(setProperty('motd=Hi\n', 'difficulty', 'hard')).toBe('motd=Hi\ndifficulty=hard\n');
  });

  it('reads the last value when a key appears twice', () => {
    expect(getProperty('pvp=true\npvp=false\n', 'pvp')).toBe('false');
  });

  it('ignores comment lines', () => {
    expect(getProperty('# pvp=true\nmotd=Hi\n', 'pvp')).toBeNull();
  });

  it('handles empty raw string for getProperty', () => {
    expect(getProperty('', 'motd')).toBeNull();
  });

  it('appends to empty string', () => {
    expect(setProperty('', 'motd', 'Hi')).toBe('motd=Hi');
  });
});
