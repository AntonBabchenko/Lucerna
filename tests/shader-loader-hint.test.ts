import { describe, expect, it } from 'vitest';
import type { InstalledMod } from '$lib/ipc/bindings';
import {
  type BannerInput,
  checkPlan,
  detectInstalledShaderLoaders,
  type InstallAvailability,
  type InstallTarget,
  installedDetectionSet,
  type Loader,
  optifineForgeBuild,
  type ShaderAvailability,
  shaderBannerState,
} from '$lib/mods/shader-loader-hint';

describe('checkPlan', () => {
  it('asks Iris for Fabric', () => {
    expect(checkPlan('fabric')).toEqual({
      shader: [{ shader: 'iris', loader: 'fabric' }],
      install: [],
    });
  });

  it('asks Iris for Quilt and, to tell "not marked for Quilt" from "none", for Fabric', () => {
    expect(checkPlan('quilt')).toEqual({
      shader: [
        { shader: 'iris', loader: 'quilt' },
        { shader: 'iris', loader: 'fabric' },
      ],
      install: [],
    });
  });

  it('asks Iris and Oculus for NeoForge, Oculus for Forge', () => {
    expect(checkPlan('neoforge').shader).toEqual([
      { shader: 'iris', loader: 'neoforge' },
      { shader: 'oculus', loader: 'neoforge' },
    ]);
    expect(checkPlan('forge')).toEqual({
      shader: [{ shader: 'oculus', loader: 'forge' }],
      install: [],
    });
  });

  it('asks every route and every mod loader install for vanilla', () => {
    expect(checkPlan('vanilla')).toEqual({
      shader: [
        { shader: 'iris', loader: 'fabric' },
        { shader: 'iris', loader: 'neoforge' },
        { shader: 'oculus', loader: 'forge' },
      ],
      install: ['fabric', 'neoforge', 'forge'],
    });
  });

  it('asks nothing without a profile', () => {
    expect(checkPlan(null)).toEqual({ shader: [], install: [] });
  });
});

describe('installedDetectionSet', () => {
  it('maps each loader to the shader loaders whose presence hides the hint', () => {
    expect(installedDetectionSet('fabric')).toEqual(['iris']);
    expect(installedDetectionSet('quilt')).toEqual(['iris']);
    expect(installedDetectionSet('neoforge')).toEqual(['iris', 'oculus']);
    expect(installedDetectionSet('forge')).toEqual(['oculus', 'optifine']);
    expect(installedDetectionSet('vanilla')).toEqual([]);
    expect(installedDetectionSet(null)).toEqual([]);
  });
});

describe('optifineForgeBuild', () => {
  it('knows release and preview-only Forge builds', () => {
    expect(optifineForgeBuild('1.20.1')).toBe('release');
    expect(optifineForgeBuild('1.21.11')).toBe('release');
    expect(optifineForgeBuild('1.21.10')).toBe('preview');
    expect(optifineForgeBuild('1.20')).toBe('preview');
  });

  it('has nothing where OptiFine has no Forge build or no build at all', () => {
    expect(optifineForgeBuild('1.21.9')).toBeNull();
    expect(optifineForgeBuild('1.21')).toBeNull();
    expect(optifineForgeBuild('26.2')).toBeNull();
    expect(optifineForgeBuild('1.21.2')).toBeNull();
  });

  it('keys on Mojang ids, not the site headings', () => {
    expect(optifineForgeBuild('1.9')).toBe('release');
    expect(optifineForgeBuild('1.8')).toBe('release');
    expect(optifineForgeBuild('1.9.0')).toBeNull();
    expect(optifineForgeBuild('1.8.0')).toBeNull();
  });

  it('never matches a snapshot', () => {
    expect(optifineForgeBuild('26.4-snapshot-3')).toBeNull();
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

type ShaderAnswers = Partial<Record<string, ShaderAvailability>>;

function input(
  loader: Loader,
  mc: string,
  shader: ShaderAnswers | null,
  install: Partial<Record<InstallTarget, InstallAvailability>> = {},
  installed: InstalledMod[] | null = [],
): BannerInput {
  return {
    instanceSelected: true,
    loader,
    mcVersion: mc,
    installed,
    outcomes:
      shader === null ? null : { shader: shader as Record<string, ShaderAvailability>, install },
  };
}

describe('shaderBannerState — profile and readiness', () => {
  it('N without a profile', () => {
    expect(
      shaderBannerState({
        instanceSelected: false,
        loader: null,
        mcVersion: null,
        installed: null,
        outcomes: null,
      }),
    ).toEqual({ kind: 'no_instance' });
  });

  it('G for a profile without a Minecraft version', () => {
    expect(shaderBannerState({ ...input('fabric', '', null), mcVersion: '' })).toEqual({
      kind: 'generic',
    });
  });

  it('hidden while a modded profile’s installed list is unread', () => {
    expect(
      shaderBannerState(input('fabric', '1.21.1', { 'iris:fabric': 'available' }, {}, null)),
    ).toEqual({
      kind: 'hidden',
    });
  });

  it('hidden when an applicable shader loader is installed, whatever the checks say', () => {
    const iris = mod({ source: 'modrinth', project_id: 'YL57xq9U' });
    expect(
      shaderBannerState(input('fabric', '1.21.1', { 'iris:fabric': 'no_build' }, {}, [iris])),
    ).toEqual({
      kind: 'hidden',
    });
  });

  it('pending while answers are missing', () => {
    expect(shaderBannerState(input('fabric', '1.21.1', null))).toEqual({ kind: 'pending' });
    expect(
      shaderBannerState(input('neoforge', '1.21.1', { 'iris:neoforge': 'available' })),
    ).toEqual({
      kind: 'pending',
    });
    expect(
      shaderBannerState(
        input('vanilla', '1.21.1', {
          'iris:fabric': 'available',
          'iris:neoforge': 'available',
          'oculus:forge': 'no_build',
        }),
      ),
    ).toEqual({ kind: 'pending' });
  });
});

describe('shaderBannerState — modded profiles', () => {
  it('M1 with only Iris on NeoForge 1.21.1 (Oculus has no version) — the reported bug', () => {
    expect(
      shaderBannerState(
        input('neoforge', '1.21.1', {
          'iris:neoforge': 'available',
          'oculus:neoforge': 'no_build',
        }),
      ),
    ).toEqual({ kind: 'options', buttons: ['iris'], optifine: null, unchecked: [] });
  });

  it('M1 with Oculus and a released OptiFine on Forge 1.20.1', () => {
    expect(shaderBannerState(input('forge', '1.20.1', { 'oculus:forge': 'available' }))).toEqual({
      kind: 'options',
      buttons: ['oculus', 'optifine'],
      optifine: 'release',
      unchecked: [],
    });
  });

  it('M1 with only a preview OptiFine on Forge 1.21.10', () => {
    expect(shaderBannerState(input('forge', '1.21.10', { 'oculus:forge': 'no_build' }))).toEqual({
      kind: 'options',
      buttons: ['optifine'],
      optifine: 'preview',
      unchecked: [],
    });
  });

  it('M1 names what could not be checked next to what was confirmed', () => {
    expect(
      shaderBannerState(
        input('neoforge', '1.21.1', { 'iris:neoforge': 'available', 'oculus:neoforge': 'unknown' }),
      ),
    ).toEqual({ kind: 'options', buttons: ['iris'], optifine: null, unchecked: ['oculus'] });
    expect(shaderBannerState(input('forge', '1.21.1', { 'oculus:forge': 'unknown' }))).toEqual({
      kind: 'options',
      buttons: ['optifine'],
      optifine: 'release',
      unchecked: ['oculus'],
    });
  });

  it('M2 when nothing is confirmed and something could not be checked', () => {
    expect(
      shaderBannerState(
        input('neoforge', '1.21.9', { 'iris:neoforge': 'no_build', 'oculus:neoforge': 'unknown' }),
      ),
    ).toEqual({ kind: 'unchecked', unchecked: ['oculus'] });
  });

  it('M3 per loader when every check says no version', () => {
    expect(shaderBannerState(input('fabric', '1.14.4', { 'iris:fabric': 'no_build' }))).toEqual({
      kind: 'none_found',
      variant: 'iris',
    });
    expect(
      shaderBannerState(
        input('neoforge', '1.21.9', { 'iris:neoforge': 'no_build', 'oculus:neoforge': 'no_build' }),
      ),
    ).toEqual({ kind: 'none_found', variant: 'iris_oculus' });
    expect(shaderBannerState(input('forge', '1.21.9', { 'oculus:forge': 'no_build' }))).toEqual({
      kind: 'none_found',
      variant: 'forge',
    });
  });

  it('Quilt: Iris for Quilt → M1; Fabric-only Iris → Q; neither → M3; Fabric unknown → M2', () => {
    expect(
      shaderBannerState(
        input('quilt', '1.21.11', { 'iris:quilt': 'available', 'iris:fabric': 'unknown' }),
      ),
    ).toEqual({ kind: 'options', buttons: ['iris'], optifine: null, unchecked: [] });
    expect(
      shaderBannerState(
        input('quilt', '26.3', { 'iris:quilt': 'no_build', 'iris:fabric': 'available' }),
      ),
    ).toEqual({ kind: 'quilt_fabric_only' });
    expect(
      shaderBannerState(
        input('quilt', '1.14.4', { 'iris:quilt': 'no_build', 'iris:fabric': 'no_build' }),
      ),
    ).toEqual({ kind: 'none_found', variant: 'iris' });
    expect(
      shaderBannerState(
        input('quilt', '26.3', { 'iris:quilt': 'no_build', 'iris:fabric': 'unknown' }),
      ),
    ).toEqual({ kind: 'unchecked', unchecked: ['iris'] });
  });
});

describe('shaderBannerState — vanilla profiles', () => {
  const ALL_INSTALLABLE = {
    fabric: 'installable',
    neoforge: 'installable',
    forge: 'installable',
  } as const;

  it('V1 lists the mod loaders that would work, in order, with OptiFine merged into Forge', () => {
    expect(
      shaderBannerState(
        input(
          'vanilla',
          '1.20.1',
          { 'iris:fabric': 'available', 'iris:neoforge': 'no_build', 'oculus:forge': 'available' },
          ALL_INSTALLABLE,
        ),
      ),
    ).toEqual({
      kind: 'vanilla_options',
      lines: [
        { loader: 'fabric', shaders: ['iris'], optifine: null },
        { loader: 'forge', shaders: ['oculus', 'optifine'], optifine: 'release' },
      ],
      unchecked: [],
    });
  });

  it('V1 marks a preview OptiFine', () => {
    expect(
      shaderBannerState(
        input(
          'vanilla',
          '1.21.10',
          { 'iris:fabric': 'available', 'iris:neoforge': 'available', 'oculus:forge': 'no_build' },
          ALL_INSTALLABLE,
        ),
      ),
    ).toMatchObject({
      kind: 'vanilla_options',
      lines: [
        { loader: 'fabric' },
        { loader: 'neoforge' },
        { loader: 'forge', shaders: ['optifine'], optifine: 'preview' },
      ],
    });
  });

  it('V1 drops a mod loader Lucerna cannot install, and names one it could not check', () => {
    expect(
      shaderBannerState(
        input(
          'vanilla',
          '1.21.1',
          { 'iris:fabric': 'available', 'iris:neoforge': 'available', 'oculus:forge': 'no_build' },
          { fabric: 'installable', neoforge: 'not_installable', forge: 'unknown' },
        ),
      ),
    ).toEqual({
      kind: 'vanilla_options',
      lines: [{ loader: 'fabric', shaders: ['iris'], optifine: null }],
      unchecked: ['forge'],
    });
  });

  it('V2 when nothing is confirmed and something could not be checked', () => {
    expect(
      shaderBannerState(
        input(
          'vanilla',
          '1.21.9',
          { 'iris:fabric': 'unknown', 'iris:neoforge': 'no_build', 'oculus:forge': 'no_build' },
          ALL_INSTALLABLE,
        ),
      ),
    ).toEqual({ kind: 'vanilla_unchecked' });
  });

  it('V3 when every mod loader is ruled out', () => {
    expect(
      shaderBannerState(
        input(
          'vanilla',
          '1.13.2',
          { 'iris:fabric': 'no_build', 'iris:neoforge': 'no_build', 'oculus:forge': 'no_build' },
          ALL_INSTALLABLE,
        ),
      ),
    ).toEqual({ kind: 'vanilla_none_found' });
    expect(
      shaderBannerState(
        input(
          'vanilla',
          '1.21.1',
          { 'iris:fabric': 'available', 'iris:neoforge': 'available', 'oculus:forge': 'no_build' },
          { fabric: 'not_installable', neoforge: 'not_installable', forge: 'not_installable' },
        ),
      ),
    ).toEqual({ kind: 'vanilla_none_found' });
  });

  it('ignores installed mods on vanilla', () => {
    expect(shaderBannerState(input('vanilla', '1.21.1', null, {}, null))).toEqual({
      kind: 'pending',
    });
  });
});

describe('detectInstalledShaderLoaders', () => {
  const ALL: ('iris' | 'oculus' | 'optifine')[] = ['iris', 'oculus', 'optifine'];

  it('detects Iris by canonical Modrinth project id', () => {
    const installed = [mod({ source: 'modrinth', project_id: 'YL57xq9U', filename: 'x.jar' })];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual(['iris']);
  });

  it('detects Iris from CurseForge by project id', () => {
    const installed = [
      mod({ source: 'curseforge', project_id: '455508', filename: 'iris-1.7.6.jar' }),
    ];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual(['iris']);
  });

  it('detects Oculus by id and OptiFine by filename', () => {
    const installed = [
      mod({ source: 'modrinth', project_id: 'GchcoXML', filename: 'y.jar' }),
      mod({ source: null, filename: 'OptiFine_1.20.1_HD_U_I6.jar' }),
    ];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual(['oculus', 'optifine']);
  });

  it('detects Oculus from CurseForge by project id', () => {
    const installed = [mod({ source: 'curseforge', project_id: '581495' })];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual(['oculus']);
  });

  it('does NOT detect iris/oculus-named addon mods that are not the loader', () => {
    const installed = [
      mod({ source: 'curseforge', project_id: '999999', filename: 'irislowka-1.2.jar' }),
      mod({ source: null, filename: 'iris-oculus-shader-folder-1.0.jar' }),
      mod({ source: 'modrinth', project_id: 'someOtherId', filename: 'iris-flywheel-compat.jar' }),
    ];
    expect(detectInstalledShaderLoaders(installed, ALL)).toEqual([]);
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
    const installed = [mod({ source: 'modrinth', project_id: 'YL57xq9U', enabled: false })];
    expect(detectInstalledShaderLoaders(installed, ['iris'])).toEqual(['iris']);
  });
});
