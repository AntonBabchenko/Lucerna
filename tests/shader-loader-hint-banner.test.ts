// ShaderLoaderHint (the Shaders tab's shader-loader banner) against mocked IPC:
// every banner state the user can meet, the re-check, a profile switch while a
// check is in flight, and the pending delay.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const ipc = vi.hoisted(() => ({
  modsListInstalled: vi.fn(),
  modsVersions: vi.fn(),
  listFabricLoaders: vi.fn(),
  listNeoforgeLoaders: vi.fn(),
  listForgeLoaders: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: ipc,
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
  },
}));

import ShaderLoaderHint from '$lib/mods/ShaderLoaderHint.svelte';
import { __resetShaderLoaderAvailabilityCache } from '$lib/mods/shader-loader-availability';

const IRIS = 'YL57xq9U';
const OCULUS = 'GchcoXML';
const ok = <T>(data: T) => ({ status: 'ok' as const, data });
const err = (kind: string) => ({ status: 'error' as const, error: { kind } });
const VERSION = { version_id: 'v' };
const LOADERS = ok([{ version: '1', stable: true, build: 1 }]);

/** `modsVersions` answering "has a version" for the listed `project:loader` pairs. */
function versionsFor(...available: string[]) {
  return async (_source: string, id: string, _mc: string, loader: string) =>
    ok(available.includes(`${id}:${loader}`) ? [VERSION] : []);
}

const handlers = () => ({
  onOpenIris: vi.fn(),
  onOpenOculus: vi.fn(),
  onOpenOptifine: vi.fn(),
  onManageLoader: vi.fn(),
});

function renderHint(
  loader: string | null,
  mcVersion: string | null,
  instanceId: string | null = 'i',
) {
  const h = handlers();
  const view = render(ShaderLoaderHint, {
    props: { instanceId, loader, mcVersion, ...h } as never,
  });
  return { ...view, h };
}

beforeEach(() => {
  ipc.modsListInstalled.mockResolvedValue(ok([]));
  ipc.modsVersions.mockResolvedValue(ok([]));
  ipc.listFabricLoaders.mockResolvedValue(LOADERS);
  ipc.listNeoforgeLoaders.mockResolvedValue(LOADERS);
  ipc.listForgeLoaders.mockResolvedValue(LOADERS);
});

afterEach(() => {
  __resetShaderLoaderAvailabilityCache();
  vi.clearAllMocks();
});

describe('ShaderLoaderHint — modded profiles', () => {
  it('offers only Iris on NeoForge 1.21.1, where Oculus has no version', async () => {
    ipc.modsVersions.mockImplementation(versionsFor(`${IRIS}:neoforge`));
    const { h } = renderHint('neoforge', '1.21.1');
    const iris = await screen.findByRole('button', { name: 'Iris' });
    expect(screen.queryByRole('button', { name: 'Oculus' })).toBeNull();
    expect(screen.getByRole('note')).toBeTruthy();
    expect(screen.getByText('Shaders need a shader loader to run — install:')).toBeTruthy();
    expect(iris.classList.contains('btn-secondary')).toBe(true);
    await fireEvent.click(iris);
    expect(h.onOpenIris).toHaveBeenCalledOnce();
  });

  it('offers Oculus and OptiFine on Forge 1.20.1, OptiFine opening its site', async () => {
    ipc.modsVersions.mockImplementation(versionsFor(`${OCULUS}:forge`));
    const { h } = renderHint('forge', '1.20.1');
    expect(await screen.findByRole('button', { name: 'Oculus' })).toBeTruthy();
    const optifine = screen.getByRole('button', { name: /OptiFine/ });
    expect(optifine.querySelector('svg')).toBeTruthy(); // the external-link icon
    expect(screen.getByText(/download a file that lists a Forge version/)).toBeTruthy();
    await fireEvent.click(optifine);
    expect(h.onOpenOptifine).toHaveBeenCalledOnce();
  });

  it('points to the preview of OptiFine on Forge 1.21.10', async () => {
    renderHint('forge', '1.21.10');
    expect(await screen.findByRole('button', { name: /OptiFine/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Oculus' })).toBeNull();
    expect(screen.getByText(/only as a preview/)).toBeTruthy();
  });

  it('says nothing fits on NeoForge 1.21.9, with no buttons', async () => {
    renderHint('neoforge', '1.21.9');
    expect(
      await screen.findByText(
        'Lucerna found no shader loader for NeoForge 1.21.9: neither Iris nor Oculus has a version for it on Modrinth.',
      ),
    ).toBeTruthy();
    expect(screen.getByText(/they won't work without a shader loader/)).toBeTruthy();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('tells a Fabric-only Iris apart from none on Quilt 26.3', async () => {
    ipc.modsVersions.mockImplementation(versionsFor(`${IRIS}:fabric`));
    renderHint('quilt', '26.3');
    expect(await screen.findByText(/hasn't marked it for Quilt/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Iris' })).toBeNull();
  });

  it('names what it could not check, and a re-check keeps the banner and asks again', async () => {
    ipc.modsVersions.mockResolvedValue(err('network'));
    renderHint('fabric', '1.21.1');
    expect(await screen.findByText("Couldn't check: Iris.")).toBeTruthy();
    expect(ipc.modsVersions).toHaveBeenCalledTimes(1);

    let answer: (v: unknown) => void = () => {};
    ipc.modsVersions.mockImplementation(() => new Promise((r) => (answer = r)));
    await fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    await waitFor(() => expect(ipc.modsVersions).toHaveBeenCalledTimes(2));
    // While it runs the banner stays and the button is busy (disabled; its
    // spinner adds to the accessible name, hence the pattern).
    expect(screen.getByText("Couldn't check: Iris.")).toBeTruthy();
    const busy = screen.getByRole('button', { name: /Check again/ }) as HTMLButtonElement;
    expect(busy.disabled).toBe(true);
    expect(busy.getAttribute('aria-busy')).toBe('true');

    answer(ok([VERSION]));
    expect(await screen.findByRole('button', { name: 'Iris' })).toBeTruthy();
    expect(screen.queryByText("Couldn't check: Iris.")).toBeNull();
  });

  it('a thrown check is "could not check", not a hidden banner', async () => {
    ipc.modsVersions.mockRejectedValue(new Error('boom'));
    renderHint('fabric', '1.21.1');
    expect(await screen.findByText("Couldn't check: Iris.")).toBeTruthy();
  });

  it('stays hidden when an applicable shader loader is installed', async () => {
    ipc.modsListInstalled.mockResolvedValue(
      ok([
        {
          filename: 'iris.jar',
          sha1: 'x',
          source: 'modrinth',
          project_id: IRIS,
          version_id: null,
          name: 'Iris',
          version_number: null,
          installed_at: '2026-10-12T00:00:00Z',
          enabled: true,
        },
      ]),
    );
    renderHint('fabric', '1.14.4');
    await waitFor(() => expect(ipc.modsVersions).toHaveBeenCalled());
    await waitFor(() => expect(ipc.modsListInstalled).toHaveBeenCalled());
    for (let i = 0; i < 5; i++) await tick();
    await new Promise((r) => setTimeout(r, 350)); // past the pending delay
    expect(screen.queryByRole('note')).toBeNull();
  });
});

describe('ShaderLoaderHint — vanilla profiles', () => {
  it('names the mod loaders that would work and opens Manage on the loader field', async () => {
    ipc.modsVersions.mockImplementation(versionsFor(`${IRIS}:fabric`, `${IRIS}:neoforge`));
    const { h } = renderHint('vanilla', '1.21.1');
    expect(await screen.findByText('Fabric: Iris')).toBeTruthy();
    expect(screen.getByText('NeoForge: Iris')).toBeTruthy();
    expect(screen.getByText('Forge: OptiFine')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /OptiFine/ })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Choose a mod loader' }));
    expect(h.onManageLoader).toHaveBeenCalledOnce();
  });

  it('leaves out a mod loader Lucerna cannot install for that Minecraft', async () => {
    ipc.modsVersions.mockImplementation(versionsFor(`${IRIS}:fabric`, `${IRIS}:neoforge`));
    ipc.listNeoforgeLoaders.mockResolvedValue(err('loader_unavailable'));
    renderHint('vanilla', '1.21.1');
    expect(await screen.findByText('Fabric: Iris')).toBeTruthy();
    expect(screen.queryByText('NeoForge: Iris')).toBeNull();
  });

  it('says nothing fits on vanilla 1.13.2', async () => {
    renderHint('vanilla', '1.13.2');
    expect(
      await screen.findByText(
        'This profile has no mod loader. Lucerna found no mod loader for Minecraft 1.13.2 that shaders would work with.',
      ),
    ).toBeTruthy();
    expect(screen.queryByRole('button')).toBeNull();
  });
});

describe('ShaderLoaderHint — no profile, switching, pending', () => {
  it('explains without buttons or IPC when no profile is selected', async () => {
    renderHint(null, null, null);
    expect(await screen.findByText(/Once you select a profile/)).toBeTruthy();
    expect(screen.queryByRole('button')).toBeNull();
    expect(ipc.modsVersions).not.toHaveBeenCalled();
  });

  it('never shows an answer for a profile it has since left', async () => {
    let first: (v: unknown) => void = () => {};
    ipc.modsVersions.mockImplementation((_s: string, _id: string, mc: string) =>
      mc === '1.21.1' ? new Promise((r) => (first = r)) : Promise.resolve(ok([])),
    );
    const { rerender, h } = renderHint('fabric', '1.21.1');
    await waitFor(() => expect(ipc.modsVersions).toHaveBeenCalledTimes(1));
    await rerender({ instanceId: 'i', loader: 'fabric', mcVersion: '1.14.4', ...h } as never);
    expect(await screen.findByText(/Iris has no version for it on Modrinth/)).toBeTruthy();
    first(ok([VERSION]));
    for (let i = 0; i < 5; i++) await tick();
    expect(screen.queryByRole('button', { name: 'Iris' })).toBeNull();
    expect(screen.getByText(/Iris has no version for it on Modrinth/)).toBeTruthy();
  });

  it('shows nothing for a moment, then "Checking" while the answer is slow', async () => {
    ipc.modsVersions.mockImplementation(() => new Promise(() => {}));
    renderHint('fabric', '1.21.1');
    await waitFor(() => expect(ipc.modsVersions).toHaveBeenCalled());
    expect(screen.queryByRole('note')).toBeNull();
    expect(await screen.findByText('Checking which one fits…')).toBeTruthy();
  });
});
