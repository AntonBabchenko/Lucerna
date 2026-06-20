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

  it('reads a CRLF value without a trailing carriage return (Bug B)', () => {
    expect(getProperty('server-port=25565\r\n', 'server-port')).toBe('25565');
  });

  it('reads a CRLF value mid-file (Minecraft rewrites the whole file as CRLF)', () => {
    const raw = 'motd=A Minecraft Server\r\ngamemode=survival\r\ndifficulty=easy\r\n';
    expect(getProperty(raw, 'gamemode')).toBe('survival');
    expect(getProperty(raw, 'difficulty')).toBe('easy');
  });

  it('reads a bare-CR-free value when the file mixes LF and CRLF', () => {
    // A hand-edited LF file the server later partly rewrites as CRLF.
    expect(getProperty('motd=Hi\nserver-port=25566\r\n', 'server-port')).toBe('25566');
  });

  it('setProperty replaces a CRLF key without corrupting the value', () => {
    // setProperty normalises to LF on write: the trailing CRLF becomes a single
    // trailing LF (same as the existing LF "replaces in place" case), and the
    // replaced value carries no stray \r.
    expect(setProperty('pvp=true\r\n', 'pvp', 'false')).toBe('pvp=false\n');
  });
});
