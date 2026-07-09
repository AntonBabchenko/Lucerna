import { describe, expect, it } from 'vitest';
import {
  coreToLoaderKind,
  displayCore,
  modCapable,
  pluginCapable,
  switchTargets,
} from '../src/lib/servers/core-display';

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
  it('displayCore uses brand-canonical capitalisation', () => {
    // ServersView renders core names through this map — pin the two most
    // easily-broken spellings (NeoForge's intentional PascalCase, plus a
    // plugin core absent from the client loader map).
    expect(displayCore('neoforge')).toBe('NeoForge');
    expect(displayCore('paper')).toBe('Paper');
  });
  it('coreToLoaderKind maps mod cores through and plugin cores to null', () => {
    // ServerMods feeds ServerModBrowser's LoaderKind prop through this map —
    // a plugin core must never leak a fake loader.
    expect(coreToLoaderKind('paper')).toBe(null);
    expect(coreToLoaderKind('fabric')).toBe('fabric');
  });
});
