import { describe, expect, it, vi } from 'vitest';
import type { InstallPlan, ModVersion } from '$lib/ipc/bindings';
import { decideModInstall } from '$lib/mods/dep-prompt';

// ---------------------------------------------------------------------------
// Minimal stubs
// ---------------------------------------------------------------------------

function mv(projectId: string, versionId: string, loaders: string[] = []): ModVersion {
  return {
    source: 'modrinth',
    project_id: projectId,
    version_id: versionId,
    name: `${projectId}-release`,
    version_number: '1.0',
    mc_versions: ['1.20.1'],
    loaders: loaders as ModVersion['loaders'],
    primary_file: { filename: '', url: '', sha1: null, size: 0, distribution_allowed: true },
    deps: [],
    published_at: null,
  };
}

function emptyPlan(): InstallPlan {
  return {
    required: [],
    optional: [],
    incompatible: [],
    unresolvable: [],
    loader_requirements: [],
  };
}

// Simple trivial displayLoader
const displayLoader = (l: string) => l;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('decideModInstall', () => {
  it('returns kind=install on fast path (empty plan, loaders match)', async () => {
    const primary = mv('main', 'v1', ['fabric']);
    const fetchProjectName = vi.fn().mockResolvedValue('Main Mod');
    const fetchVersions = vi.fn().mockResolvedValue([]);

    const decision = await decideModInstall(emptyPlan(), primary, {
      loader: 'fabric',
      mcVersion: '1.20.1',
      manifestOrder: ['1.20.1'],
      displayLoader,
      fetchProjectName,
      fetchVersions,
    });

    expect(decision.kind).toBe('install');
    if (decision.kind === 'install') {
      expect(decision.primaryProjectName).toBe('Main Mod');
    }
  });

  it('falls back to primary.name when fetchProjectName returns null', async () => {
    const primary = mv('main', 'v1', ['fabric']);
    const fetchProjectName = vi.fn().mockResolvedValue(null);
    const fetchVersions = vi.fn().mockResolvedValue([]);

    const decision = await decideModInstall(emptyPlan(), primary, {
      loader: 'fabric',
      mcVersion: '1.20.1',
      manifestOrder: ['1.20.1'],
      displayLoader,
      fetchProjectName,
      fetchVersions,
    });

    expect(decision.kind).toBe('install');
    if (decision.kind === 'install') {
      // Falls back to the ModVersion.name field
      expect(decision.primaryProjectName).toBe('main-release');
    }
  });

  it('returns kind=prompt when loaderMismatch forces the dialog open (even if dep lists are empty)', async () => {
    // primary reports loaders=['forge'] but instance runs fabric
    const primary = mv('main', 'v1', ['forge']);
    const fetchProjectName = vi.fn().mockResolvedValue('Main Mod');
    const fetchVersions = vi.fn().mockResolvedValue([]);

    const decision = await decideModInstall(emptyPlan(), primary, {
      loader: 'fabric',
      mcVersion: '1.20.1',
      manifestOrder: ['1.20.1'],
      displayLoader,
      fetchProjectName,
      fetchVersions,
    });

    expect(decision.kind).toBe('prompt');
    if (decision.kind === 'prompt') {
      expect(decision.prompt.loaderMismatch).toEqual({
        instanceLoader: 'fabric',
        modLoaders: ['forge'],
      });
      expect(decision.prompt.primaryProjectName).toBe('Main Mod');
    }
  });

  it('returns kind=prompt with enriched required dep names when required deps are present', async () => {
    const primary = mv('main', 'v1', ['fabric']);
    const reqVersion = mv('dep-a', 'va', ['fabric']);
    const plan: InstallPlan = {
      ...emptyPlan(),
      required: [{ version: reqVersion, selection_reason: 'newest_no_pin' }],
    };

    const fetchProjectName = vi.fn().mockImplementation(async (_source, id: string) => {
      if (id === 'main') return 'Main Mod';
      if (id === 'dep-a') return 'Dep A';
      return null;
    });
    const fetchVersions = vi.fn().mockResolvedValue([]);

    const decision = await decideModInstall(plan, primary, {
      loader: 'fabric',
      mcVersion: '1.20.1',
      manifestOrder: ['1.20.1'],
      displayLoader,
      fetchProjectName,
      fetchVersions,
    });

    expect(decision.kind).toBe('prompt');
    if (decision.kind === 'prompt') {
      expect(decision.prompt.required).toHaveLength(1);
      expect(decision.prompt.required[0]?.projectName).toBe('Dep A');
      expect(decision.prompt.primaryProjectName).toBe('Main Mod');
    }
  });

  it('does not set loaderMismatch when primary.loaders is empty', async () => {
    // An empty loaders array means "no loader restriction" — no mismatch.
    const primary = mv('main', 'v1', []); // loaders = []
    const fetchProjectName = vi.fn().mockResolvedValue('Main Mod');
    const fetchVersions = vi.fn().mockResolvedValue([]);

    const decision = await decideModInstall(emptyPlan(), primary, {
      loader: 'forge',
      mcVersion: '1.20.1',
      manifestOrder: ['1.20.1'],
      displayLoader,
      fetchProjectName,
      fetchVersions,
    });

    expect(decision.kind).toBe('install');
  });
});
