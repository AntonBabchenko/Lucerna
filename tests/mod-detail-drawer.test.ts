import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

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
          summary: 'short summary',
          icon_url: null,
          downloads: 1,
          author: 'a',
          updated_at: null,
        },
        body_html: '<p>long body</p>',
        gallery: [{ url: 'https://x/1.png', title: 'Shot' }],
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

import ModDetailModal from '$lib/mods/ModDetailModal.svelte';

describe('ModDetailModal', () => {
  it('renders Overview with body, gallery, and a recommended-install button', async () => {
    const onInstall = vi.fn();
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'p',
        mcVersion: '1.20.1',
        loader: 'fabric',
        onClose: () => {},
        onInstall,
      },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText('Demo Mod')).toBeTruthy();
    expect(screen.getByText('long body')).toBeTruthy();
    expect(screen.getByText(/any ads or links in it are the author's/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: /View on Modrinth/i })).toBeTruthy();
    expect(screen.getByRole('img')).toBeTruthy();
    const btn = screen.getByRole('button', { name: /Install 1\.0/ });
    await fireEvent.click(btn);
    expect(onInstall).toHaveBeenCalledWith(expect.objectContaining({ version_id: 'v1' }));
  });

  it('lists versions on the Versions tab', async () => {
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'p',
        mcVersion: '1.20.1',
        loader: 'fabric',
        onClose: () => {},
        onInstall: () => {},
      },
    });
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole('tab', { name: 'Versions' }));
    expect(screen.getByText('1.0')).toBeTruthy();
  });
});
