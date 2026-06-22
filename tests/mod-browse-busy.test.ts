import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
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
  // A required dependency version used by the dialog-path test. Distinct
  // project so it surfaces in the plan's `required` list rather than being
  // pruned as the primary itself.
  const depVersion = {
    source: 'modrinth',
    project_id: 'dep',
    version_id: 'dv1',
    name: 'Fabric API 0.1',
    version_number: '0.1',
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
      gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
    },
    // Exported for the dialog-path test to drive a plan with a required dep.
    __depVersion: depVersion,
  };
});

import * as bindings from '$lib/ipc/bindings';
import ModBrowseView from '$lib/mods/ModBrowseView.svelte';

// Pull the mocked command surface + the dep fixture the factory exported.
const commands = (bindings as unknown as { commands: Record<string, ReturnType<typeof vi.fn>> })
  .commands;
const depVersion = (bindings as unknown as { __depVersion: unknown }).__depVersion;

const baseProps = {
  source: 'modrinth',
  instanceId: 'i',
  instanceName: 'Test',
  mcVersion: '1.20.1',
  loader: 'fabric',
  kind: 'mod',
};

afterEach(() => {
  vi.clearAllMocks();
  delete globalThis.__resolveInstall;
});

describe('ModBrowseView install busy state', () => {
  it('disables the card Install + shows a spinner until install resolves', async () => {
    render(ModBrowseView, { props: baseProps as never });

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

  it('tracks concurrent installs independently — starting B does not clear A', async () => {
    // Regression guard for the scalar→Set change: the UI lets you click Install
    // on card B while card A is still installing. With a single scalar id, B's
    // start would overwrite A's and prematurely clear A's spinner. A Set keeps
    // both busy until each one's own flow settles.
    const mkHit = (id: string, name: string) => ({
      source: 'modrinth',
      project_id: id,
      slug: name.toLowerCase(),
      name,
      summary: '',
      icon_url: null,
      downloads: 0,
      author: 'a',
      updated_at: null,
    });
    commands.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'set' });
    commands.modsSearch.mockResolvedValue({
      status: 'ok',
      data: {
        hits: [mkHit('abc', 'Sodium'), mkHit('xyz', 'Lithium')],
        total: 2,
        offset: 0,
        page_size: 20,
      },
    });
    commands.modsListInstalled.mockResolvedValue({ status: 'ok', data: [] });
    commands.assetsList.mockResolvedValue({ status: 'ok', data: [] });
    commands.modsProject.mockImplementation((_src: string, id: string) =>
      Promise.resolve({ status: 'ok', data: { summary: mkHit(id, id) } }),
    );
    // Echo the requested project_id so each card resolves to its own version.
    commands.modsVersions.mockImplementation((_src: string, id: string) =>
      Promise.resolve({
        status: 'ok',
        data: [
          {
            source: 'modrinth',
            project_id: id,
            version_id: `${id}-v1`,
            name: `${id} 1.0`,
            version_number: '1.0',
            loaders: ['fabric'],
            primary_file: { distribution_allowed: true },
          },
        ],
      }),
    );
    commands.modsResolveInstallPlan.mockResolvedValue({
      status: 'ok',
      data: {
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [],
        loader_requirements: [],
      },
    });
    // Both installs hang (distinct never-resolving promises) so both stay busy.
    commands.modsInstallWithDeps.mockReturnValue(new Promise(() => {}));

    render(ModBrowseView, { props: baseProps as never });

    const installButtons = await screen.findAllByRole('button', { name: /install/i });
    expect(installButtons.length).toBe(2);
    const [aBtn, bBtn] = installButtons;

    // Start install on card A → A becomes busy.
    await fireEvent.click(aBtn!);
    await waitFor(() => expect(aBtn!.querySelector('[role="status"]')).not.toBeNull());

    // Start install on card B while A is still in flight.
    await fireEvent.click(bBtn!);
    await waitFor(() => expect(bBtn!.querySelector('[role="status"]')).not.toBeNull());

    // The key assertion: A is STILL busy — B's start did not clobber it.
    expect(aBtn!.querySelector('[role="status"]')).not.toBeNull();
    expect(aBtn!.hasAttribute('disabled')).toBe(true);
  });

  it('keeps the card Install disabled while its dependency dialog is open', async () => {
    // Plan with a required dependency → startInstall opens the dependency
    // dialog instead of installing directly. Its `finally` clears
    // installingProjectId at hand-off, so without the depPrompt term in
    // isCardBusy the card would briefly re-enable under the open dialog.
    // clearAllMocks (afterEach) wiped the implementation, so restore the
    // happy-path responses this render needs, then set the dep plan.
    commands.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'set' });
    commands.modsSearch.mockResolvedValue({
      status: 'ok',
      data: {
        hits: [
          {
            source: 'modrinth',
            project_id: 'abc',
            slug: 'sodium',
            name: 'Sodium',
            summary: 'Fast rendering',
            icon_url: null,
            downloads: 0,
            author: 'caffeine',
            updated_at: null,
          },
        ],
        total: 1,
        offset: 0,
        page_size: 20,
      },
    });
    commands.modsListInstalled.mockResolvedValue({ status: 'ok', data: [] });
    commands.assetsList.mockResolvedValue({ status: 'ok', data: [] });
    commands.modsProject.mockResolvedValue({
      status: 'ok',
      data: { summary: { name: 'Fabric API' } },
    });
    commands.modsVersions.mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'abc',
          version_id: 'v1',
          name: 'Sodium 0.5',
          version_number: '0.5',
          loaders: ['fabric'],
          primary_file: { distribution_allowed: true },
        },
      ],
    });
    commands.modsResolveInstallPlan.mockResolvedValue({
      status: 'ok',
      data: {
        required: [{ version: depVersion, selection_reason: 'newest_no_pin' }],
        optional: [],
        incompatible: [],
        unresolvable: [],
        loader_requirements: [],
      },
    });

    render(ModBrowseView, { props: baseProps as never });

    const installBtn = await screen.findByRole('button', { name: /install/i });
    await fireEvent.click(installBtn);

    // The dependency dialog opens. Once it is in the DOM, the originating
    // Install button must stay disabled/busy (depPrompt drives isCardBusy)
    // until the user confirms or cancels.
    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeTruthy();
    });
    expect(installBtn.hasAttribute('disabled')).toBe(true);
    expect(installBtn.querySelector('[role="status"]')).not.toBeNull();
  });

  it('closes the detail drawer when an install started from it settles', async () => {
    // Open the detail drawer (ModDetailModal) by clicking the card's title
    // button, install the recommended version from it (fast path → empty
    // plan), then resolve the install. The drawer's onInstall finally now
    // clears installingVersionId AND closes the drawer (drawerProject = null)
    // so the result — and any install-error banner — is visible in the browse
    // list underneath instead of hidden behind the open modal.
    //
    // clearAllMocks (afterEach) wipes call data but NOT the implementations a
    // prior test installed (e.g. the dep-plan override). Restore the fast-path
    // (empty plan) responses this render needs.
    commands.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'set' });
    commands.modsSearch.mockResolvedValue({
      status: 'ok',
      data: {
        hits: [
          {
            source: 'modrinth',
            project_id: 'abc',
            slug: 'sodium',
            name: 'Sodium',
            summary: 'Fast rendering',
            icon_url: null,
            downloads: 0,
            author: 'caffeine',
            updated_at: null,
          },
        ],
        total: 1,
        offset: 0,
        page_size: 20,
      },
    });
    commands.modsListInstalled.mockResolvedValue({ status: 'ok', data: [] });
    commands.assetsList.mockResolvedValue({ status: 'ok', data: [] });
    commands.modsProject.mockResolvedValue({
      status: 'ok',
      data: { summary: { name: 'Sodium', author: 'caffeine', source: 'modrinth', downloads: 0 } },
    });
    commands.modsVersions.mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'abc',
          version_id: 'v1',
          name: 'Sodium 0.5',
          version_number: '0.5',
          loaders: ['fabric'],
          mc_versions: ['1.20.1'],
          primary_file: { distribution_allowed: true },
        },
      ],
    });
    commands.modsResolveInstallPlan.mockResolvedValue({
      status: 'ok',
      data: {
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [],
        loader_requirements: [],
      },
    });
    commands.modsInstallWithDeps.mockReturnValue(
      new Promise((res) => {
        globalThis.__resolveInstall = res as (v: unknown) => void;
      }),
    );

    render(ModBrowseView, { props: baseProps as never });

    // The card title button opens the detail modal. It carries the mod name
    // ("Sodium") plus the author/downloads sub-line.
    const titleBtn = await screen.findByRole('button', { name: /Sodium/i });
    await fireEvent.click(titleBtn);

    // Wait for the modal to load and its recommended-install CTA to render.
    const dialog = await screen.findByRole('dialog');
    const installCta = await waitFor(() =>
      within(dialog).getByRole('button', { name: /install/i }),
    );

    await fireEvent.click(installCta);

    // The drawer stays open while the install runs so its CTA can spin.
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeNull();
    });

    // The originating CTA is busy mid-flight: disabled + spinner present.
    await waitFor(() => expect(installCta.hasAttribute('disabled')).toBe(true));
    expect(installCta.querySelector('[role="status"]')).not.toBeNull();

    // Settle the install — the finally must close the drawer.
    globalThis.__resolveInstall?.({ status: 'ok', data: null });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
  });
});
