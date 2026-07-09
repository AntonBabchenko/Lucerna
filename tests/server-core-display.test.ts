import { describe, expect, it } from 'vitest';
import { modCapable, pluginCapable, switchTargets } from '../src/lib/servers/core-display';

describe('server core capability/switch matrix', () => {
  it('plugin cores are exactly paper+purpur', () => {
    expect(pluginCapable('paper')).toBe(true);
    expect(pluginCapable('purpur')).toBe(true);
    expect(pluginCapable('vanilla')).toBe(false);
    expect(pluginCapable('forge')).toBe(false);
  });
  it('switch matrix mirrors the Rust core_switch_allowed matrix', () => {
    expect(switchTargets('vanilla')).toEqual(['paper', 'purpur']);
    expect(switchTargets('paper')).toEqual(['purpur']);
    expect(switchTargets('purpur')).toEqual(['paper']);
    expect(switchTargets('fabric')).toEqual([]);
  });
  it('mod cores exclude vanilla and plugin cores', () => {
    expect(modCapable('vanilla')).toBe(false);
    expect(modCapable('paper')).toBe(false);
    expect(modCapable('neoforge')).toBe(true);
  });
});
