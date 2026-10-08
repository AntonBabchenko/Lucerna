import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// Mirrors the vi.hoisted listener registry pattern from
// installed-mods-view.test.ts so the (hoisted) vi.mock factory can register
// the event callbacks the view subscribes to on mount.
const listeners = vi.hoisted(() => ({
  modInstalled: null as null | (() => void),
  modUninstalled: null as null | (() => void),
  modToggle: null as null | (() => void),
}));

// Two installed platform mods: A (sha1 'a', project PA) requires B; B (sha1 'b',
// project PB) stands alone. The dependency graph reports A as a root whose
// single required child is the installed node B, so B is required by A. The
// factory helpers live in vi.hoisted so they exist before the (also hoisted)
// vi.mock factory runs.
const { mod, proj } = vi.hoisted(() => ({
  mod: (sha1: string, projectId: string, name: string, requires: string[] = []) => ({
    filename: `${sha1}.jar`,
    sha1,
    source: 'modrinth',
    project_id: projectId,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires,
  }),
  proj: (projectId: string, name: string) => ({
    status: 'ok',
    data: {
      summary: {
        source: 'modrinth',
        project_id: projectId,
        slug: name.toLowerCase(),
        name,
        summary: '',
        icon_url: null,
        downloads: 1,
        author: 'x',
        updated_at: null,
      },
      description: '',
      website_url: null,
    },
  }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [mod('a', 'PA', 'Alpha', ['PB']), mod('b', 'PB', 'Bravo')],
    }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsProject: vi
      .fn()
      .mockImplementation((_src: string, id: string) =>
        Promise.resolve(id === 'PA' ? proj('PA', 'Alpha') : proj('PB', 'Bravo')),
      ),
    modsProjects: vi.fn((_s: string, ids: string[]) =>
      Promise.resolve({
        status: 'ok',
        data: ids.map((id) => ({
          source: 'modrinth',
          project_id: id,
          slug: id,
          name: id === 'PA' ? 'Alpha' : 'Bravo',
          summary: '',
          icon_url: null,
          downloads: 0,
          author: 'x',
          updated_at: null,
        })),
      }),
    ),
    modsDependencyGraph: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        roots: [
          {
            sha1: 'a',
            source: 'modrinth',
            project_id: 'PA',
            name: 'Alpha',
            required: [
              {
                source: 'modrinth',
                project_id: 'PB',
                name: 'Bravo',
                installed: true,
                declared: 'required',
                cycle: false,
                children: [],
              },
            ],
            optional: [],
          },
          {
            sha1: 'b',
            source: 'modrinth',
            project_id: 'PB',
            name: 'Bravo',
            required: [],
            optional: [],
          },
        ],
      },
    }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
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

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

// Pointing at a mod marks only what is under the pointer, through its own CSS :hover. Nothing
// lights up its row, its dependency-tree nodes and its «Required by» entries together — 0.25.0's
// amber wash did, and then a blue ring. The way from a dependency or a dependent to its row is its «show in the list».
// A class drew both, so the guard is on every element's class list, whatever a future highlight
// would be called.
const classSnapshot = () =>
  new Map([...document.body.querySelectorAll('*')].map((el) => [el, el.getAttribute('class')]));
const classChanges = (before: Map<Element, string | null>): string[] =>
  [...before]
    .filter(([el, cls]) => el.getAttribute('class') !== cls)
    .map(([el, cls]) => `<${el.tagName.toLowerCase()}> "${cls}" → "${el.getAttribute('class')}"`);

// A pointer that comes to rest on `el` has entered it and every element around it, and its
// mouseover bubbles up from it.
async function pointAt(el: Element): Promise<void> {
  const around: Element[] = [];
  for (let n: Element | null = el; n && n !== document.body; n = n.parentElement) around.unshift(n);
  for (const n of around) await fireEvent.mouseEnter(n);
  await fireEvent.mouseOver(el);
}

const relationPill = (rowKey: string) =>
  waitFor(() => {
    const pill = document.querySelector(`[data-mod-row="${rowKey}"] [data-testid="relation-pill"]`);
    if (!pill) throw new Error(`relation pill of ${rowKey} not rendered yet`);
    return pill as HTMLButtonElement;
  });

describe('pointing at a mod marks nothing else', () => {
  it('a row, a dependency-tree node and a «Required by» entry change no class anywhere', async () => {
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });

    // Open both dependency sections (the pills appear once the graph has loaded): A's tree names
    // B, and B's «Required by» names A — every place one mod shows up in the other's row.
    await fireEvent.click(await relationPill('modrinth:PA'));
    await fireEvent.click(await relationPill('modrinth:PB'));
    const treeItem = await screen.findByRole('treeitem', { name: 'Bravo' });
    const requiredBy = within(screen.getByRole('group', { name: 'Bravo' })).getByRole('button', {
      name: 'Alpha',
    });
    const aCard = document.querySelector(
      '[data-mod-row="modrinth:PA"] [data-testid="card-list-row"]',
    );
    expect(aCard).not.toBeNull();

    const treeName = treeItem.querySelector('[data-tree-name]');
    expect(treeName).not.toBeNull();

    // What each gesture changed, collected so a failure names every one of them. The keyboard
    // marks nothing either: a tree item taking focus moves the tree's tab stop, and the focus
    // ring on its own row is CSS.
    const gestures: [string, () => Promise<void>][] = [
      ["pointing at A's row", () => pointAt(aCard as Element)],
      ["pointing at B's node in A's dependency tree", () => pointAt(treeName as Element)],
      ["pointing at A's entry in B's «Required by»", () => pointAt(requiredBy)],
      [
        "focusing B's node in A's dependency tree",
        async () => {
          await fireEvent.focusIn(treeItem);
        },
      ],
    ];
    const changed: Record<string, string[]> = {};
    for (const [what, gesture] of gestures) {
      const before = classSnapshot();
      await gesture();
      changed[what] = classChanges(before);
    }
    expect(changed).toEqual(Object.fromEntries(gestures.map(([what]) => [what, []])));
  });
});
