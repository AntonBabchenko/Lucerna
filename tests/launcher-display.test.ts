import { describe, it, expect } from 'vitest';
import { displayLauncher } from '$lib/instances/launcher-display';

describe('displayLauncher', () => {
  it('maps each ForeignLauncher to a label', () => {
    expect(displayLauncher('mojang_launcher')).toBe('Minecraft Launcher');
    expect(displayLauncher('tlauncher')).toBe('TLauncher');
    expect(displayLauncher('prism')).toBe('Prism Launcher');
    expect(displayLauncher('raw_minecraft')).toBe('Minecraft');
  });
});
