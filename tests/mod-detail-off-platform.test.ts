import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { modsProject, modsVersions } = vi.hoisted(() => ({
  modsProject: vi.fn(),
  modsVersions: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: { modsProject, modsVersions } }));

import ModDetailModal from '$lib/mods/ModDetailModal.svelte';

const ok = <T>(data: T) => ({ status: 'ok', data }) as const;

function build(id: string, over: Record<string, unknown> = {}) {
  return {
    source: 'modrinth',
    project_id: 'p',
    version_id: id,
    name: `FerriteCore ${id}`,
    version_number: id,
    mc_versions: ['1.21.1'],
    loaders: ['neoforge'],
    primary_file: {
      filename: `ferritecore-${id}.jar`,
      url: '',
      sha1: `sha-${id}`,
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
    ...over,
  };
}

// What the platform lists for a NeoForge 1.21.1 instance, and a build of the
// same project made for another Minecraft and loader.
const FITS = build('7.0.3');
const FOREIGN = build('6.0.1', { mc_versions: ['1.20.1'], loaders: ['fabric'] });

// `as never`: the same escape hatch `tests/mod-detail-busy.test.ts` uses for
// this component's props — the fixtures are deliberately loose.
const props = (over: Record<string, unknown> = {}) =>
  ({
    source: 'modrinth',
    projectId: 'p',
    mcVersion: '1.21.1',
    loader: 'neoforge',
    onClose: () => {},
    onInstall: vi.fn(),
    ...over,
  }) as never;

/** `filtered` answers the instance-filtered call; the unfiltered (show-all)
 *  call always lists both builds, fitting one first. */
function versionsAnswer(filtered: unknown) {
  modsVersions.mockImplementation((_s: unknown, _p: unknown, mc: string | null) =>
    mc === null ? Promise.resolve(ok([FITS, FOREIGN])) : filtered,
  );
}

async function openVersionsShowingAll() {
  await fireEvent.click(await screen.findByRole('tab', { name: 'Versions' }));
  await fireEvent.click(screen.getByTestId('mod-detail-show-all'));
  await screen.findByText('6.0.1');
}

/** The per-row install buttons, in list order: [FITS, FOREIGN]. */
const rowButtons = () => screen.getAllByRole('button', { name: 'Install' });

beforeEach(() => {
  vi.clearAllMocks();
  modsProject.mockResolvedValue(
    ok({
      summary: {
        source: 'modrinth',
        project_id: 'p',
        slug: 'ferrite-core',
        name: 'FerriteCore',
        summary: 'Memory usage optimizations',
        icon_url: null,
        downloads: 1,
        author: 'malte0811',
        updated_at: null,
      },
      body_html: null,
      gallery: [],
      website_url: null,
    }),
  );
  versionsAnswer(Promise.resolve(ok([FITS])));
});

describe('ModDetailModal — a build made for another Minecraft', () => {
  it('marks a build the platform does not list for this instance, in show-all mode only', async () => {
    render(ModDetailModal, { props: props() });
    await openVersionsShowingAll();

    expect(screen.getAllByText('Not for this profile')).toHaveLength(1);
    // The foreign row also says WHICH loaders it is for; the fitting row keeps
    // its plain line.
    expect(screen.getByText('MC: 1.20.1 · Fabric')).toBeTruthy();
    expect(screen.getByText('MC: 1.21.1')).toBeTruthy();

    // Back to the compatible list: nothing to mark.
    await fireEvent.click(screen.getByTestId('mod-detail-show-all'));
    expect(screen.queryByText('Not for this profile')).toBeNull();
  });

  it('marks nothing while the list for this instance is still loading, and installs straight through', async () => {
    // (pin) Unknown is not foreign: the modal does not know, so it does not
    // say. The backend still refuses without consent (D6 catches it).
    versionsAnswer(new Promise(() => {}));
    const onInstall = vi.fn();
    render(ModDetailModal, { props: props({ onInstall }) });
    await openVersionsShowingAll();

    expect(screen.queryByText('Not for this profile')).toBeNull();
    await fireEvent.click(rowButtons()[1]);
    expect(onInstall).toHaveBeenCalledWith(expect.objectContaining({ version_id: '6.0.1' }));
  });

  it('marks nothing when the list for this instance could not be loaded', async () => {
    // (pin) The trap: a FAILED filtered fetch collapses to an empty list in
    // this modal. Badging against that would mark every row of a list it
    // never saw.
    versionsAnswer(
      Promise.resolve({
        status: 'error',
        error: { kind: 'mods_network', url: 'https://api.modrinth.com', details: 'timeout' },
      }),
    );
    render(ModDetailModal, { props: props() });
    await openVersionsShowingAll();

    expect(screen.queryByText('Not for this profile')).toBeNull();
  });

  it('asks before a foreign build is installed, and installs nothing until answered', async () => {
    const onInstall = vi.fn();
    render(ModDetailModal, { props: props({ onInstall }) });
    await openVersionsShowingAll();

    await fireEvent.click(rowButtons()[1]);

    const dialog = await screen.findByRole('dialog', { name: 'Mod compatibility warning' });
    expect(dialog.textContent).toContain('FerriteCore 6.0.1');
    expect(dialog.textContent).toContain('built for Minecraft 1.20.1, this profile runs 1.21.1');
    expect(dialog.textContent).toContain('built for Fabric, this profile uses NeoForge');
    expect(onInstall).not.toHaveBeenCalled();
  });

  it('installs the foreign build with the consent flag once the user agrees', async () => {
    const onInstall = vi.fn();
    render(ModDetailModal, { props: props({ onInstall }) });
    await openVersionsShowingAll();
    await fireEvent.click(rowButtons()[1]);

    await fireEvent.click(await screen.findByRole('button', { name: /install anyway/i }));

    expect(onInstall).toHaveBeenCalledTimes(1);
    expect(onInstall).toHaveBeenCalledWith(expect.objectContaining({ version_id: '6.0.1' }), {
      allowOffPlatform: true,
    });
    expect(screen.queryByRole('dialog', { name: 'Mod compatibility warning' })).toBeNull();
  });

  it('changes nothing when the user declines, and stays open', async () => {
    const onInstall = vi.fn();
    const onClose = vi.fn();
    render(ModDetailModal, { props: props({ onInstall, onClose }) });
    await openVersionsShowingAll();
    await fireEvent.click(rowButtons()[1]);

    await fireEvent.click(await screen.findByRole('button', { name: /skip these/i }));

    expect(onInstall).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog', { name: 'Mod compatibility warning' })).toBeNull();
    expect(screen.getByRole('dialog', { name: 'FerriteCore' })).toBeTruthy();
  });

  it('installs a compatible build with no question asked', async () => {
    // (pin)
    const onInstall = vi.fn();
    render(ModDetailModal, { props: props({ onInstall }) });
    await openVersionsShowingAll();

    await fireEvent.click(rowButtons()[0]);

    expect(onInstall).toHaveBeenCalledWith(expect.objectContaining({ version_id: '7.0.3' }));
    expect(screen.queryByRole('dialog', { name: 'Mod compatibility warning' })).toBeNull();
  });

  it('recognises the installed build by its bytes when the registry has no version id', async () => {
    render(ModDetailModal, {
      props: props({ installedVersionId: null, installedSha1: 'SHA-7.0.3' }),
    });
    await fireEvent.click(await screen.findByRole('tab', { name: 'Versions' }));

    expect(await screen.findByText('7.0.3 · installed')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Installed' }).hasAttribute('disabled')).toBe(true);
  });
});
