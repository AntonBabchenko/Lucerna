import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// Capture each listener callback at module-eval time via vi.hoisted so
// the vi.mock factory (which is itself hoisted) can write through to
// this registry. Tests can then poke the listener to simulate a live
// event from the backend. The real bindings expose a single `modToggle`
// event for both enable and disable transitions — not two separate ones.
const listeners = vi.hoisted(() => ({
  modInstalled: null as null | (() => void),
  modUninstalled: null as null | (() => void),
  modToggle: null as null | (() => void),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // Both rows return the typedError ok-shape. The first is a normal
    // Modrinth install (source set, version + project ids present); the
    // second is a "manual" mod — a JAR the user dropped into the
    // instance's mods folder by hand (source: null).
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'jei.jar',
          sha1: 'abc',
          source: 'modrinth',
          project_id: 'p',
          version_id: 'v',
          name: 'Just Enough Items',
          version_number: '15.0',
          installed_at: '2026-05-18T00:00:00Z',
          enabled: true,
        },
        {
          filename: 'mystery.jar',
          sha1: 'def',
          source: null,
          project_id: null,
          version_id: null,
          name: 'mystery.jar',
          version_number: null,
          installed_at: '2026-05-18T00:00:00Z',
          enabled: false,
        },
      ],
    }),
    modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUninstall: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { token: 'tok', items: [{ sha1: 'abc', name: 'Just Enough Items' }] },
    }),
    // Nothing here depends on anything: the safe flip order names just the targets.
    modsRemovalImpact: vi.fn(async (_i: string, sha1s: string[]) => ({
      status: 'ok',
      data: { dependents: [], order: sha1s },
    })),
    modsEnableImpact: vi.fn(async (_i: string, sha1s: string[]) => ({
      status: 'ok',
      data: { requirements: [], order: sha1s },
    })),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    openModsFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // The row menu: the jar on disk and the hold.
    modsRevealFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsSetHold: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsLastUpdateCheck: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsListHolds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // The view now fetches ModProject for each platform-installed mod
    // so it can render the project's display name via the shared
    // ModCard component.
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'p',
          slug: 'jei',
          name: 'Just Enough Items',
          summary: 'View items and recipes',
          icon_url: null,
          downloads: 1234,
          author: 'mezz',
          updated_at: null,
        },
        description: '',
        website_url: null,
      },
    }),
    modsProjects: vi.fn((_s: unknown, ids: string[]) =>
      Promise.resolve({
        status: 'ok',
        data: ids.map((id) => ({
          source: 'modrinth',
          project_id: id,
          slug: 'jei',
          name: 'Just Enough Items',
          summary: 'View items and recipes',
          icon_url: null,
          downloads: 1234,
          author: 'mezz',
          updated_at: null,
        })),
      }),
    ),
  },
  events: {
    modInstalled: {
      listen: (cb: () => void) => {
        listeners.modInstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modUninstalled: {
      listen: (cb: () => void) => {
        listeners.modUninstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modToggle: {
      listen: (cb: () => void) => {
        listeners.modToggle = cb;
        return Promise.resolve(() => {});
      },
    },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

// The row menu's «Open mod page» leaves through the https-only opener.
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';
import { __resetUpdateCheckStoreForTests } from '$lib/mods/update-check-store.svelte';
import { toastList } from '$lib/toasts/toasts.svelte';

// The persisted update check is held once per profile for the whole app; a check one case runs
// must not seed the next case's rows.
beforeEach(() => __resetUpdateCheckStoreForTests());

describe('InstalledModsView', () => {
  it('renders rows with Disable button when enabled and Enable when disabled', async () => {
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // Yield once so the mount-time refresh() promise resolves before we
    // assert on the rendered rows.
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText('Just Enough Items')).toBeTruthy();
    // Enabled row → Disable button; Disabled row → Enable button.
    expect(screen.getByRole('button', { name: 'Disable' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Enable' })).toBeTruthy();
  });

  it('asks what depends on a mod before removing it, then offers Undo', async () => {
    const mod = await import('$lib/ipc/bindings');
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    const first = screen.getAllByRole('button', { name: 'Remove' })[0];
    if (!first) throw new Error('expected at least one Remove button');
    await fireEvent.click(first);
    await waitFor(() => expect(mod.commands.modsUninstall).toHaveBeenCalledWith('i', 'abc'));
    expect(mod.commands.modsRemovalImpact).toHaveBeenCalledWith('i', ['abc']);
    await waitFor(() =>
      expect(
        toastList().some(
          (x) => x.title === 'Removed Just Enough Items' && x.action?.label === 'Undo',
        ),
      ).toBe(true),
    );
  });

  // Plan §5b V2: the focused Remove button left with its row and focus fell to <body>. It goes to
  // the row now in that place — the next one — else the one before it, else the empty list.
  describe('focus after a removal', () => {
    const jei = {
      filename: 'jei.jar',
      sha1: 'abc',
      source: 'modrinth',
      project_id: 'p',
      version_id: 'v',
      name: 'Just Enough Items',
      version_number: '15.0',
      installed_at: '2026-05-18T00:00:00Z',
      enabled: true,
    };
    const mystery = {
      filename: 'mystery.jar',
      sha1: 'def',
      source: null,
      project_id: null,
      version_id: null,
      name: 'mystery.jar',
      version_number: null,
      installed_at: '2026-05-18T00:00:00Z',
      enabled: false,
    };
    /** Lists `before`, then — the re-read after the removal — `after`; removes `index`. */
    async function removeAt(before: unknown[], after: unknown[], index: number) {
      const mod = await import('$lib/ipc/bindings');
      vi.mocked(mod.commands.modsListInstalled)
        .mockResolvedValueOnce({ status: 'ok', data: before } as never)
        .mockResolvedValueOnce({ status: 'ok', data: after } as never);
      render(InstalledModsView, {
        props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
      });
      await waitFor(() => expect(screen.getAllByRole('group')).toHaveLength(before.length));
      const remove = screen.getAllByRole('button', { name: 'Remove' })[index] as HTMLElement;
      remove.focus();
      await fireEvent.click(remove);
      await waitFor(() => expect(screen.queryAllByRole('group')).toHaveLength(after.length));
    }

    it('goes to the next row', async () => {
      await removeAt([jei, mystery], [mystery], 0);
      const row = screen.getByRole('group', { name: 'mystery.jar' });
      await waitFor(() => expect(row.contains(document.activeElement)).toBe(true));
    });

    it('goes to the row before when the last one went', async () => {
      await removeAt([jei, mystery], [jei], 1);
      const row = screen.getByRole('group', { name: 'Just Enough Items' });
      await waitFor(() => expect(row.contains(document.activeElement)).toBe(true));
    });

    it('goes to the empty list when no row is left', async () => {
      await removeAt([jei], [], 0);
      await waitFor(() => expect(document.activeElement).toBe(screen.getByTestId('list-empty')));
    });
  });

  it('asks what depends on a mod before disabling it', async () => {
    const mod = await import('$lib/ipc/bindings');
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole('button', { name: 'Disable' }));
    await waitFor(() => expect(mod.commands.modsDisable).toHaveBeenCalledWith('i', 'abc'));
    expect(mod.commands.modsRemovalImpact).toHaveBeenCalledWith('i', ['abc']);
  });

  it('the ⋯ menu opens the mods folder of this profile', async () => {
    const mod = await import('$lib/ipc/bindings');
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await fireEvent.click(screen.getByRole('button', { name: /more actions/i }));
    await fireEvent.click(screen.getByRole('menuitem', { name: /open mods folder/i }));
    await waitFor(() => expect(mod.commands.openModsFolder).toHaveBeenCalledWith('i'));
  });

  // The re-check is a thing the user asked for: when its compatibility half fails, the spinner
  // going away must not read as "all clear".
  it('a re-check whose compatibility check fails says so', async () => {
    const mod = await import('$lib/ipc/bindings');
    vi.mocked(mod.commands.checkInstanceModCompat).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'network', url: 'https://api.modrinth.com', details: 'offline' },
    } as never);
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await screen.findByText('Just Enough Items');
    await fireEvent.click(screen.getByRole('button', { name: /more actions/i }));
    await fireEvent.click(screen.getByRole('menuitem', { name: /re-check/i }));
    await waitFor(() =>
      expect(toastList().some((t) => t.title === "Couldn't re-check compatibility")).toBe(true),
    );
  });

  it('shows empty state when no instance is selected', () => {
    render(InstalledModsView, {
      props: { instanceId: null, mcVersion: null, loader: null },
    });
    expect(screen.getByText(/Pick an instance first/)).toBeTruthy();
  });

  it('checks for updates and renders an update badge', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsCheckUpdates as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          sha1: 'abc',
          name: 'Just Enough Items',
          source: 'modrinth',
          project_id: 'p',
          current_version_id: 'v',
          current_version_number: '15.0',
          state: {
            kind: 'update_available',
            target: {
              source: 'modrinth',
              project_id: 'p',
              version_id: 'v2',
              name: 'Just Enough Items',
              version_number: '16.0',
              mc_versions: ['1.20.1'],
              loaders: ['fabric'],
              primary_file: {
                filename: 'jei-16.jar',
                url: 'https://example/jei-16.jar',
                sha1: 'ffff',
                size: 1,
                distribution_allowed: true,
              },
              deps: [],
              published_at: null,
            },
          },
        },
      ],
    });
    const { container } = render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole('button', { name: /Check for updates/ }));
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.modsCheckUpdates).toHaveBeenCalledWith('i');
    // The update chip shows the version transition as "vOld <arrow-right icon> vNew".
    const updateArrow = container.querySelector('.lucide-arrow-right');
    expect(updateArrow).toBeTruthy();
    expect(updateArrow?.parentElement?.textContent).toContain('v15.0');
    expect(updateArrow?.parentElement?.textContent).toContain('v16.0');
    expect(screen.getByRole('button', { name: 'Update' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Update all (1)' })).toBeTruthy();
  });

  it('Update all opens a review, and only the ticked mods update', async () => {
    const mod = await import('$lib/ipc/bindings');
    const target = {
      source: 'modrinth',
      project_id: 'p',
      version_id: 'v2',
      name: 'Just Enough Items',
      version_number: '16.0',
      mc_versions: ['1.20.1'],
      loaders: ['fabric'],
      primary_file: {
        filename: 'jei-16.jar',
        url: 'https://example/jei-16.jar',
        sha1: 'ffff',
        size: 1,
        distribution_allowed: true,
      },
      deps: [],
      published_at: null,
    };
    const result = {
      sha1: 'abc',
      name: 'Just Enough Items',
      source: 'modrinth',
      project_id: 'p',
      current_version_id: 'v',
      current_version_number: '15.0',
      state: { kind: 'update_available', target },
    };
    (mod.commands.modsLastUpdateCheck as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { checked_at_secs: 1, results: [result] },
    });
    (mod.commands.modsUpdateOne as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { primary_name: 'Just Enough Items', installed_dependencies: [], details: [] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Update all (1)' }));
    expect(
      (screen.getByRole('checkbox', { name: 'Just Enough Items' }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(mod.commands.modsUpdateOne).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'Update 1' }));
    await waitFor(() =>
      expect(mod.commands.modsUpdateOne).toHaveBeenCalledWith('i', 'abc', target, false),
    );
    // The run closes the review.
    await waitFor(() => expect(screen.queryByTestId('update-review-list')).toBeNull());
  });

  it('marks a modpack-origin mod with a pack chip', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Cool Pack', mod_shas: ['abc'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    const coolChip = screen.getByText('Cool Pack');
    expect(coolChip.querySelector('.lucide-package')).toBeTruthy();
  });

  it('labels an unresolved pack-origin mod "from modpack" with a chip', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          filename: 'bundled.jar',
          sha1: 'pack1',
          source: null,
          project_id: null,
          version_id: null,
          name: 'bundled.jar',
          version_number: null,
          installed_at: '2026-05-18T00:00:00Z',
          enabled: true,
          // already attempted — suppresses the backfill for this test
          enrich_attempted: true,
        },
      ],
    });
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Parasites Reloaded', mod_shas: ['pack1'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/from modpack/)).toBeTruthy();
    // The chip, not its label: a long pack name is cut inside its own span.
    const packChip = screen.getByTestId('mod-pack-chip');
    expect(packChip.textContent).toContain('Parasites Reloaded');
    expect(packChip.querySelector('.lucide-package')).toBeTruthy();
  });

  it('keeps "manual mod" for a hand-dropped jar not in the pack', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          filename: 'handdrop.jar',
          sha1: 'hand1',
          source: null,
          project_id: null,
          version_id: null,
          name: 'handdrop.jar',
          version_number: null,
          installed_at: '2026-05-18T00:00:00Z',
          enabled: true,
          enrich_attempted: true,
        },
      ],
    });
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Some Pack', mod_shas: ['other-sha'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/manual mod/)).toBeTruthy();
  });

  it('runs an enrichment backfill when a pack mod is unenriched', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockClear();
    const unresolved = {
      filename: 'bundled.jar',
      sha1: 'pack1',
      source: null,
      project_id: null,
      version_id: null,
      name: 'bundled.jar',
      version_number: null,
      installed_at: '2026-05-18T00:00:00Z',
      enabled: true,
      enrich_attempted: false,
    };
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce({ status: 'ok', data: [unresolved] })
      .mockResolvedValueOnce({
        status: 'ok',
        // Distinguishable filename so the assertion proves the re-fetched
        // list (not the pre-backfill list) is what gets rendered.
        data: [
          {
            ...unresolved,
            filename: 'bundled-after-backfill.jar',
            name: 'bundled-after-backfill.jar',
            enrich_attempted: true,
          },
        ],
      });
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Parasites Reloaded', mod_shas: ['pack1'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.modsEnrichPackMods).toHaveBeenCalledWith('i');
    expect(mod.commands.modsListInstalled).toHaveBeenCalledTimes(2);
    // The re-fetched list is what renders — proves `r = r2` was consumed.
    expect(screen.getByText('bundled-after-backfill.jar')).toBeTruthy();
    expect(screen.queryByText('bundled.jar')).toBeNull();
  });

  // The row menu (spec D13, §6.8) — «Show in folder» is also the keyboard path to the file name
  // the version's tooltip shows on hover.
  describe('row menu', () => {
    const openMenu = async (name: string, testid = 'card-list-row') => {
      const row = await screen.findByRole('group', { name });
      await fireEvent.contextMenu(row.querySelector(`[data-testid="${testid}"]`) as HTMLElement);
    };
    const items = () => screen.getAllByRole('menuitem').map((m) => m.textContent?.trim());

    it('Show in folder reveals the jar on disk', async () => {
      const mod = await import('$lib/ipc/bindings');
      render(InstalledModsView, {
        props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
      });
      await openMenu('Just Enough Items');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Show in folder' }));
      await waitFor(() => expect(mod.commands.modsRevealFile).toHaveBeenCalledWith('i', 'abc'));
    });

    it('Open mod page opens the project’s page; a hand-dropped jar has neither page nor hold', async () => {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      render(InstalledModsView, {
        props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
      });
      await openMenu('Just Enough Items');
      await fireEvent.click(screen.getByRole('menuitem', { name: 'Open mod page' }));
      await waitFor(() => expect(openUrl).toHaveBeenCalledWith('https://modrinth.com/mod/jei'));
      await openMenu('mystery.jar', 'manual-mod-row');
      expect(items()).toContain('Show in folder');
      expect(items()).not.toContain('Open mod page');
      expect(items()).not.toContain("Don't update");
    });

    it('Don’t update holds the project, and the menu then offers to allow updates', async () => {
      const mod = await import('$lib/ipc/bindings');
      render(InstalledModsView, {
        props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
      });
      await openMenu('Just Enough Items');
      vi.mocked(mod.commands.modsListHolds).mockResolvedValueOnce({
        status: 'ok',
        data: [{ source: 'modrinth', project_id: 'p' }],
      });
      await fireEvent.click(screen.getByRole('menuitem', { name: "Don't update" }));
      await waitFor(() =>
        expect(mod.commands.modsSetHold).toHaveBeenCalledWith('i', 'modrinth', 'p', true),
      );
      await waitFor(() => expect(screen.getByTestId('mod-held-pin')).toBeTruthy());
      await openMenu('Just Enough Items');
      expect(items()).toContain('Allow updates');
      expect(items()).not.toContain("Don't update");
    });

    // A modpack's own mods are never offered updates — the pack owns their versions — so a hold
    // there would promise nothing.
    it('a modpack’s own mod offers no «Don’t update»', async () => {
      const mod = await import('$lib/ipc/bindings');
      vi.mocked(mod.commands.modsPackOriginSummary).mockResolvedValueOnce({
        status: 'ok',
        data: { project_name: 'Cool Pack', mod_shas: ['abc'] },
      });
      render(InstalledModsView, {
        props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
      });
      await screen.findByText('Cool Pack');
      await openMenu('Just Enough Items');
      expect(items()).toContain('Show in folder');
      expect(items()).not.toContain("Don't update");
    });
  });
});
