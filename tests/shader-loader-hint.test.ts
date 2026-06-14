import { describe, expect, it } from 'vitest';
import { shaderLoaderOptions } from '$lib/mods/shader-loader-hint';

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
