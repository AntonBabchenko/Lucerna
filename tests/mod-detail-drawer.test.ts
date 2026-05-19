import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// Mock is declared before the SUT import so vitest hoists it ahead of
// the module graph. Both commands return the tauri-specta
// `{ status, data | error }` shape — see typedError in bindings.ts.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'p',
          slug: 'mod',
          name: 'Demo Mod',
          summary: 'd',
          icon_url: null,
          downloads: 1,
          author: 'a',
          updated_at: null,
        },
        description: 'long body',
        website_url: null,
      },
    }),
    modsVersions: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'p',
          version_id: 'v1',
          name: 'v1',
          version_number: '1.0',
          mc_versions: ['1.20.1'],
          loaders: ['fabric'],
          primary_file: {
            filename: 'x.jar',
            url: '',
            sha1: '',
            size: 0,
            distribution_allowed: true,
          },
          deps: [],
          published_at: null,
        },
      ],
    }),
  },
}));

import ModDetailDrawer from '$lib/mods/ModDetailDrawer.svelte';

describe('ModDetailDrawer', () => {
  it('loads project and renders a version row', async () => {
    render(ModDetailDrawer, {
      props: {
        source: 'modrinth',
        projectId: 'p',
        mcVersion: '1.20.1',
        loader: 'fabric',
        onClose: () => {},
        onInstall: () => {},
      },
    });
    // Yield once so the $effect's load() promise resolves before assertions.
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText('Demo Mod')).toBeTruthy();
    expect(screen.getByText('1.0')).toBeTruthy();
  });
});
