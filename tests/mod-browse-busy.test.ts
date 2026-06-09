import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// vi.mock is hoisted to the top of the file, so its factory cannot reference
// module-scope variables. Everything the factory needs is defined inside it.
// The deferred install promise's resolver is published via a global so the
// test body can resolve it mid-flight.
declare global {
  // eslint-disable-next-line no-var
  var __resolveInstall: ((v: unknown) => void) | undefined;
}

vi.mock('$lib/ipc/bindings', () => {
  const hit = {
    source: 'modrinth',
    project_id: 'abc',
    slug: 'sodium',
    name: 'Sodium',
    summary: 'Fast rendering',
    icon_url: null,
    downloads: 0,
    author: 'caffeine',
    updated_at: null,
  };
  const version = {
    source: 'modrinth',
    project_id: 'abc',
    version_id: 'v1',
    name: 'Sodium 0.5',
    version_number: '0.5',
    loaders: ['fabric'],
    primary_file: { distribution_allowed: true },
  };
  return {
    commands: {
      // ModBrowseView (Browse branch) on mount
      modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
      modsSearch: vi.fn().mockResolvedValue({
        status: 'ok',
        data: { hits: [hit], total: 1, offset: 0, page_size: 20 },
      }),
      modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
      modsProject: vi.fn().mockResolvedValue({ status: 'ok', data: { summary: hit } }),
      // Install flow
      modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [version] }),
      modsResolveInstallPlan: vi.fn().mockResolvedValue({
        status: 'ok',
        data: {
          required: [],
          optional: [],
          incompatible: [],
          unresolvable: [],
          loader_requirements: [],
        },
      }),
      // Never-resolving so the busy state is observable mid-flight; the test
      // resolves it via the published resolver to confirm the spinner clears.
      modsInstallWithDeps: vi.fn().mockReturnValue(
        new Promise((res) => {
          globalThis.__resolveInstall = res as (v: unknown) => void;
        }),
      ),
      // Other commands a sibling/this view may touch — inert resolved.
      assetsList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    },
    events: {
      modInstalled: { listen: () => Promise.resolve(() => {}) },
      modUninstalled: { listen: () => Promise.resolve(() => {}) },
      modToggle: { listen: () => Promise.resolve(() => {}) },
    },
  };
});

import ModBrowseView from '$lib/mods/ModBrowseView.svelte';

afterEach(() => vi.clearAllMocks());

describe('ModBrowseView install busy state', () => {
  it('disables the card Install + shows a spinner until install resolves', async () => {
    render(ModBrowseView, {
      props: {
        source: 'modrinth',
        instanceId: 'i',
        instanceName: 'Test',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'mod',
      } as never,
    });

    const installBtn = await screen.findByRole('button', { name: /install/i });
    await fireEvent.click(installBtn);

    await waitFor(() => {
      expect(installBtn.hasAttribute('disabled')).toBe(true);
      expect(installBtn.querySelector('[role="status"]')).not.toBeNull();
    });

    globalThis.__resolveInstall?.({ status: 'ok', data: null });
    await waitFor(() => {
      expect(installBtn.querySelector('[role="status"]')).toBeNull();
    });
  });
});
