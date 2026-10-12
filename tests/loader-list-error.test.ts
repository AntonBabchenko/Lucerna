import { beforeAll, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import { formatLoaderListError } from '$lib/instances/loader-list-error';

describe('formatLoaderListError', () => {
  beforeAll(() => locale.set('en'));

  const forgeTimeout = {
    kind: 'network' as const,
    url: 'https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml',
    failure: { kind: 'timeout' as const },
    details:
      'Network { url: "https://maven.minecraftforge.net/…", details: "error sending request for url (…)" }',
  };

  it('says which list did not load, then why — with the site, never the dump', () => {
    expect(formatLoaderListError(forgeTimeout, 'forge')).toBe(
      "Couldn't load the Forge version list. maven.minecraftforge.net didn't respond in time — try again.",
    );
  });

  it('never shows the URL or the transport detail', () => {
    const msg = formatLoaderListError(forgeTimeout, 'neoforge');
    expect(msg).toContain('NeoForge');
    expect(msg).not.toContain('https://');
    expect(msg).not.toContain('Network {');
    expect(msg).not.toContain('error sending request');
  });

  it('words a mod-platform network failure the same way', () => {
    const msg = formatLoaderListError({ ...forgeTimeout, kind: 'mods_network' }, 'forge');
    expect(msg).toBe(
      "Couldn't load the Forge version list. maven.minecraftforge.net didn't respond in time — try again.",
    );
  });

  it('keeps the short wording for an unsupported Minecraft version', () => {
    expect(
      formatLoaderListError(
        { kind: 'loader_unavailable', loader: 'quilt', mc_version: '1.20.1' },
        'quilt',
      ),
    ).toBe('Quilt does not support Minecraft 1.20.1');
  });
});
