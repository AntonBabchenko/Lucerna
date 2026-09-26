// The server data pack catalog on a world with only level.dat_old: the backend
// refuses every add, switch and removal there until the server has restored
// level.dat. The host hands the browser the reason (`blockedReason`), and every
// card action and the detail's Install are off and say why — instead of a
// live button whose click ends in a red error that replaces the results.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const cmd = vi.hoisted(() => ({
  modsSearch: vi.fn(),
  serverListDatapacks: vi.fn(),
  modsDatapackVersions: vi.fn(),
  serverInstallDatapackVersion: vi.fn(),
  serverSetDatapackEnabled: vi.fn(),
  serverRemoveDatapack: vi.fn(),
  modsProject: vi.fn(),
  modsGetCurseforgeKeyStatus: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: cmd }));

import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
import ServerDatapackBrowser from '$lib/servers/datapacks/ServerDatapackBrowser.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { revealTooltip } from './test-utils/reveal-tooltip';

const REASON = 'Start the server once to restore level.dat';

function hit(project_id: string, name: string) {
  return {
    source: 'modrinth' as const,
    project_id,
    slug: project_id,
    name,
    summary: `${name} summary`,
    icon_url: null,
    downloads: 10,
    author: 'someone',
    updated_at: null,
  };
}

beforeAll(() => locale.set('en'));
beforeEach(() => {
  browserPrefs.layout = 'list';
  cmd.modsSearch.mockResolvedValue({
    status: 'ok',
    data: { hits: [hit('fresh', 'Fresh Pack'), hit('have', 'Owned Pack')], total: 2 },
  });
  cmd.serverListDatapacks.mockResolvedValue({
    status: 'ok',
    data: {
      level_dat: 'only_old',
      entries: [
        {
          record: {
            filename: 'owned.zip',
            sha1: 'a'.repeat(40),
            source: 'modrinth',
            project_id: 'have',
            version_id: 'v1',
            name: 'Owned Pack',
            version_number: '1.0',
            enrich_attempted: false,
          },
          state: 'enabled',
          present: true,
          is_folder: false,
          ignored_reason: null,
        },
      ],
    },
  });
});
afterEach(() => {
  hideTooltip();
  vi.clearAllMocks();
});

function mount(blockedReason: string | null) {
  render(ServerDatapackBrowser, {
    props: {
      serverId: 's1',
      mcVersion: '1.21.1',
      onInstalled: () => {},
      showSourcePicker: false,
      blockedReason,
    },
  });
}

describe('ServerDatapackBrowser — a world that refuses changes', () => {
  it('a card cannot be installed, and says why', async () => {
    mount(REASON);
    const install = (await screen.findByRole('button', { name: 'Install' })) as HTMLButtonElement;
    expect(install.disabled).toBe(true);
    revealTooltip(install.closest('span') as HTMLElement);
    expect(tooltipState.text).toBe(REASON);
    await fireEvent.click(install);
    expect(cmd.modsDatapackVersions).not.toHaveBeenCalled();
  });

  it("an installed card's switch and removal are off too", async () => {
    mount(REASON);
    const toggle = (await screen.findByRole('button', { name: 'Disable' })) as HTMLButtonElement;
    const remove = screen.getByRole('button', { name: 'Remove' }) as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    expect(remove.disabled).toBe(true);
    await fireEvent.click(toggle);
    await fireEvent.click(remove);
    expect(cmd.serverSetDatapackEnabled).not.toHaveBeenCalled();
    expect(cmd.serverRemoveDatapack).not.toHaveBeenCalled();
  });

  it('the detail says why and offers no Install', async () => {
    cmd.modsProject.mockResolvedValue({
      status: 'ok',
      data: { summary: hit('fresh', 'Fresh Pack'), body_html: '', gallery: [], website_url: null },
    });
    cmd.modsDatapackVersions.mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'fresh',
          version_id: 'v2',
          name: '2.0',
          version_number: '2.0',
          mc_versions: ['1.21.1'],
          loaders: ['datapack'],
          primary_file: {
            filename: 'fresh.zip',
            url: 'https://cdn.example/fresh.zip',
            distribution_allowed: true,
          },
          deps: [],
          published_at: null,
        },
      ],
    });
    mount(REASON);
    await fireEvent.click(await screen.findByText('Fresh Pack'));
    await fireEvent.click(await screen.findByRole('tab', { name: /versions/i }));
    const installs = (await screen.findAllByRole('button', {
      name: 'Install',
    })) as HTMLButtonElement[];
    // The detail's own Install (the card behind it is off already).
    const inDetail = installs.filter((b) => b.closest('[role="dialog"]') !== null);
    expect(inDetail.length).toBeGreaterThan(0);
    for (const b of inDetail) expect(b.disabled).toBe(true);
    expect(screen.getByTestId('server-content-detail-blocked').textContent).toContain(REASON);
    expect(cmd.serverInstallDatapackVersion).not.toHaveBeenCalled();
  });

  it('with no reason, a card installs as before', async () => {
    cmd.modsDatapackVersions.mockResolvedValue({ status: 'ok', data: [] });
    mount(null);
    const install = (await screen.findByRole('button', { name: 'Install' })) as HTMLButtonElement;
    expect(install.disabled).toBe(false);
    await fireEvent.click(install);
    await waitFor(() => expect(cmd.modsDatapackVersions).toHaveBeenCalled());
  });
});
