import { describe, expect, it } from 'vitest';
import type { InstalledMod } from '$lib/ipc/bindings';
import { detectInstalledShaderLoaders, shaderLoaderOptions } from '$lib/mods/shader-loader-hint';

describe('shaderLoaderOptions', () => {
  it('offers only Iris on Fabric and Quilt', () => {
    expect(shaderLoaderOptions('fabric')).toEqual(['iris']);
    expect(shaderLoaderOptions('quilt')).toEqual(['iris']);
  });

  it('offers Oculus and OptiFine on Forge', () => {
    expect(shaderLoaderOptions('forge')).toEqual(['oculus', 'optifine']);
  });

  it('offers Iris and Oculus on NeoForge', () => {
    expect(shaderLoaderOptions('neoforge')).toEqual(['iris', 'oculus']);
  });

  it('offers only OptiFine on vanilla', () => {
    expect(shaderLoaderOptions('vanilla')).toEqual(['optifine']);
  });

  it('offers all three when no instance is selected (loader null)', () => {
    expect(shaderLoaderOptions(null)).toEqual(['iris', 'oculus', 'optifine']);
  });
});

function mod(partial: Partial<InstalledMod>): InstalledMod {
  return {
    filename: 'unknown.jar',
    sha1: 'x',
    source: null,
    project_id: null,
    version_id: null,
    name: 'Unknown',
    version_number: null,
    installed_at: '2026-06-14T00:00:00Z',
    enabled: true,
    ...partial,
  };
}

describe('detectInstalledShaderLoaders', () => {
  const ALL: ('iris' | 'oculus' | 'optifine')[] = ['iris', 'oculus', 'optifine'];

  it('detects Iris by canonical Modrinth project id', () => {
    const installed = [mod({ source: 'modrinth', project_id: 'YL57xq9U', filename: 'x.jar' })];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual(['iris']);
  });

  it('detects Iris by filename (CurseForge / manual jar)', () => {
    const installed = [mod({ source: 'curseforge', filename: 'iris-mc1.21.1-1.7.5.jar' })];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual(['iris']);
  });

  it('detects Oculus by id and OptiFine by filename', () => {
    const installed = [
      mod({ source: 'modrinth', project_id: 'GchcoXML', filename: 'y.jar' }),
      mod({ source: null, filename: 'OptiFine_1.20.1_HD_U_I6.jar' }),
    ];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual(['oculus', 'optifine']);
  });

  it('ignores unrelated mods and mid-name "iris" matches', () => {
    const installed = [
      mod({ filename: 'sodium-fabric-0.5.8.jar' }),
      mod({ source: 'modrinth', project_id: 'AANobbMI', filename: 'sodium.jar' }),
      mod({ filename: 'modern-iris-decor.jar' }), // contains "iris" but not at start
      mod({ filename: 'better-optifine-compat-1.0.jar' }), // references optifine mid-name
    ];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual([]);
  });

  it('only returns ids present in the applicable set', () => {
    // An Iris jar sitting in a Forge instance must not suppress the hint:
    // iris is not applicable on Forge.
    const installed = [mod({ source: 'modrinth', project_id: 'YL57xq9U', filename: 'iris.jar' })];
    expect(detectInstalledShaderLoaders(installed, ['oculus', 'optifine'])).toEqual([]);
  });

  it('counts a disabled shader loader as installed', () => {
    const installed = [mod({ filename: 'iris-mc1.20.1-1.7.0.jar', enabled: false })];
    expect(detectInstalledShaderLoaders(installed, ['iris'])).toEqual(['iris']);
  });
});
