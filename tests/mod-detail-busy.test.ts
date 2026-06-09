import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// One compatible version (v1). The Overview footer CTA installs the
// recommended version; the Versions tab installs per-row. Both label with
// "Install" so /install/i matches either, and the busy flag is keyed off
// version_id === installingVersionId.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'abc',
          slug: 'sodium',
          name: 'Sodium',
          summary: 'short',
          icon_url: null,
          downloads: 0,
          author: 'a',
          updated_at: null,
        },
        body_html: null,
        gallery: [],
        website_url: null,
      },
    }),
    modsVersions: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'abc',
          version_id: 'v1',
          name: 'S 0.5',
          version_number: '0.5',
          mc_versions: ['1.20.1'],
          loaders: ['fabric'],
          primary_file: {
            filename: 's.jar',
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

import ModDetailModal from '$lib/mods/ModDetailModal.svelte';

describe('ModDetailModal install busy state', () => {
  it('shows a spinner on the recommended CTA for the installing version', async () => {
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'mod',
        installingVersionId: 'v1',
        onInstall: () => {},
        onClose: () => {},
      } as never,
    });
    const btn = await screen.findByRole('button', { name: /install/i });
    expect(btn.querySelector('[role="status"]')).not.toBeNull();
    expect(btn.hasAttribute('disabled')).toBe(true);
  });

  it('shows a spinner on the per-row version button for the installing version', async () => {
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'mod',
        installingVersionId: 'v1',
        onInstall: () => {},
        onClose: () => {},
      } as never,
    });
    // Switch to the Versions tab where the per-row install button lives.
    await fireEvent.click(await screen.findByRole('tab', { name: 'Versions' }));
    const rowBtns = await screen.findAllByRole('button', { name: /install/i });
    const busy = rowBtns.find((b) => b.querySelector('[role="status"]') !== null);
    expect(busy).toBeTruthy();
    expect(busy?.hasAttribute('disabled')).toBe(true);
  });

  it('does not show a spinner when installingVersionId does not match', async () => {
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        kind: 'mod',
        installingVersionId: 'other',
        onInstall: () => {},
        onClose: () => {},
      } as never,
    });
    const btn = await screen.findByRole('button', { name: /install/i });
    expect(btn.querySelector('[role="status"]')).toBeNull();
  });
});
