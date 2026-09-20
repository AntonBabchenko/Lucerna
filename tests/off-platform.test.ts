import { get } from 'svelte/store';
import { beforeAll, describe, expect, it } from 'vitest';
import { locale, t } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import {
  type OffPlatformFacts,
  offPlatformFactsOfError,
  offPlatformLabel,
  offPlatformReason,
  offPlatformRows,
} from '$lib/mods/off-platform';

const BOTH: OffPlatformFacts = {
  versionMc: ['1.20.1', '1.20.2'],
  versionLoaders: ['fabric', 'quilt'],
  instanceMc: '1.21.1',
  instanceLoader: 'neoforge',
};

describe('off-platform wording', () => {
  beforeAll(() => locale.set('en'));

  it('names what differs: Minecraft and loader', () => {
    expect(offPlatformReason(BOTH, get(t))).toBe(
      'built for Minecraft 1.20.1, 1.20.2, this profile runs 1.21.1; built for Fabric, Quilt, this profile uses NeoForge',
    );
  });

  it('names only the axis that differs', () => {
    expect(offPlatformReason({ ...BOTH, versionLoaders: ['neoforge'] }, get(t))).toBe(
      'built for Minecraft 1.20.1, 1.20.2, this profile runs 1.21.1',
    );
    expect(offPlatformReason({ ...BOTH, versionMc: ['1.21.1'] }, get(t))).toBe(
      'built for Fabric, Quilt, this profile uses NeoForge',
    );
  });

  it('claims nothing it does not know when the tags fit or are missing', () => {
    // The platform's own filtered listing left the build out although its tags
    // fit (the filename heuristic), or the build carries no tags at all
    // (CurseForge): say only that it may not be compatible.
    expect(offPlatformReason({ ...BOTH, versionMc: ['1.21.1'], versionLoaders: [] }, get(t))).toBe(
      'may not be compatible',
    );
    expect(offPlatformReason({ ...BOTH, versionMc: [], versionLoaders: [] }, get(t))).toBe(
      'may not be compatible',
    );
  });

  it('labels the build by name and version number without saying the number twice', () => {
    expect(offPlatformLabel('FerriteCore', '6.0.1')).toBe('FerriteCore 6.0.1');
    expect(offPlatformLabel('FerriteCore 6.0.1 for Fabric', '6.0.1')).toBe(
      'FerriteCore 6.0.1 for Fabric',
    );
  });

  it('builds the one row the confirmation shows', () => {
    expect(
      offPlatformRows('FerriteCore 6.0.1', { ...BOTH, versionMc: ['1.21.1'] }, get(t)),
    ).toEqual([
      {
        filename: 'FerriteCore 6.0.1',
        reason: 'built for Fabric, Quilt, this profile uses NeoForge',
      },
    ]);
  });

  it('reads the facts back out of the typed refusal', () => {
    const refusal: IpcError = {
      kind: 'mod_version_not_for_instance',
      version_mc: ['1.20.1', '1.20.2'],
      version_loaders: ['fabric', 'quilt'],
      instance_mc: '1.21.1',
      instance_loader: 'neoforge',
    };
    expect(offPlatformFactsOfError(refusal)).toEqual(BOTH);
  });

  it('is not interested in any other error', () => {
    // (pin)
    expect(offPlatformFactsOfError({ kind: 'mods_not_found', source: 'modrinth' })).toBeNull();
  });
});
