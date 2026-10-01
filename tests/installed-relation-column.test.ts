/**
 * The relation column of the Installed list (spec 2026-09-30, DESIGN.md §9): an installed row's
 * «⛓N ↑M» sits in a fixed-width cell after the checkbox and before the icon — the row's
 * disclosure — its figures in two slots that line up from row to row, the column as wide as the
 * widest figure over the whole profile.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstalledModRow from '$lib/mods/installed/InstalledModRow.svelte';
import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';
import {
  depSectionId,
  hasFigures,
  relationFigures,
  relationInput,
  relationSlotDigits,
} from '$lib/mods/installed/relation-cell';
import ModCard from '$lib/mods/ModCard.svelte';

// ---------------------------------------------------------------------------------------------
// The IPC the list view reads (the row cases below never call it).
// ---------------------------------------------------------------------------------------------
const v = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string) => ({
    filename: `${name.toLowerCase()}-1.0.jar`,
    sha1,
    source: 'modrinth',
    project_id: projectId,
    version_id: 'v',
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  });
  const absent = (i: number) => ({
    source: 'modrinth',
    project_id: `gone-${i}`,
    name: `Gone ${i}`,
    installed: false,
    declared: 'required',
    cycle: false,
    children: [],
  });
  return {
    rows: [mod('a', 'PA', 'Alpha'), mod('l', 'PL', 'Lib')],
    graph: {
      roots: [
        {
          sha1: 'a',
          source: 'modrinth',
          project_id: 'PA',
          name: 'Alpha',
          // Twelve projects: the widest ⛓ figure of the profile, on the row a filter hides.
          required: [
            {
              source: 'modrinth',
              project_id: 'PL',
              name: 'Lib',
              installed: true,
              declared: 'required',
              cycle: false,
              children: [],
            },
            ...Array.from({ length: 11 }, (_, i) => absent(i)),
          ],
          optional: [],
        },
        {
          sha1: 'l',
          source: 'modrinth',
          project_id: 'PL',
          name: 'Lib',
          required: [],
          optional: [],
        },
      ],
    },
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: v.rows }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: v.graph }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

// ---------------------------------------------------------------------------------------------
// The rules (relation-cell.ts)
// ---------------------------------------------------------------------------------------------
const none = { depsUnknown: false, depTotal: 0, optionalTotal: 0, requiredByCount: 0 };

describe('relationFigures — what each slot shows', () => {
  it('counts required dependencies and the mods that require it', () => {
    expect(relationFigures({ ...none, depTotal: 3, requiredByCount: 13 })).toEqual({
      dep: '3',
      by: '13',
    });
  });

  it('shows unknown dependencies as «?», never as nothing', () => {
    expect(relationFigures({ ...none, depsUnknown: true, requiredByCount: 1 })).toEqual({
      dep: '?',
      by: '1',
    });
  });

  it('falls back to the optional count only when that is all the section would show', () => {
    expect(relationFigures({ ...none, optionalTotal: 2 })).toEqual({ dep: '2', by: null });
    expect(relationFigures({ ...none, optionalTotal: 2, requiredByCount: 1 })).toEqual({
      dep: null,
      by: '1',
    });
  });

  it('leaves both slots empty when there is nothing to open', () => {
    expect(relationFigures(none)).toEqual({ dep: null, by: null });
    expect(hasFigures(relationFigures(none))).toBe(false);
    expect(hasFigures({ dep: null, by: '1' })).toBe(true);
  });

  it('reads its input off the graph root the way every row does', () => {
    const root = { optional: [{}, {}], deps_unknown: 'unreachable' } as never;
    expect(relationInput(root, 4, 1)).toEqual({
      depsUnknown: true,
      depTotal: 4,
      optionalTotal: 2,
      requiredByCount: 1,
    });
    expect(relationInput(undefined, 0, 0)).toEqual(none);
  });
});

describe('relationSlotDigits — the column is as wide as its widest figure', () => {
  it('takes the longest figure of each slot over every row, one at least', () => {
    expect(relationSlotDigits([])).toEqual({ dep: 1, by: 1 });
    expect(
      relationSlotDigits([
        { dep: '3', by: '13' },
        { dep: '?', by: null },
        { dep: null, by: '120' },
      ]),
    ).toEqual({ dep: 1, by: 3 });
    expect(relationSlotDigits([{ dep: '12', by: null }])).toEqual({ dep: 2, by: 1 });
  });

  it('names the section a cell controls by the row’s jar', () => {
    expect(depSectionId('abc')).toBe('dep-section-abc');
  });
});

// ---------------------------------------------------------------------------------------------
// The cell in a row
// ---------------------------------------------------------------------------------------------
const summary = {
  source: 'modrinth' as const,
  project_id: 'p',
  slug: 's',
  name: 'Alpha',
  summary: '',
  icon_url: null,
  downloads: 1,
  author: 'x',
  updated_at: null,
};
const installed = (over: Record<string, unknown> = {}) => ({
  filename: 'a.jar',
  sha1: 'a',
  source: 'modrinth' as const,
  project_id: 'p',
  version_id: 'v',
  name: 'Alpha',
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
  ...over,
});
const root = {
  sha1: 'a',
  source: 'modrinth',
  project_id: 'p',
  name: 'Alpha',
  required: [
    {
      source: 'modrinth',
      project_id: 'lib',
      name: 'Lib',
      installed: true,
      declared: 'required',
      cycle: false,
      children: [],
    },
  ],
  optional: [],
} as never;
const requiredBy = [{ name: 'Beta', source: 'modrinth' as const, projectId: 'pb', sha1: 'b' }];
const rowProps = () => ({
  summary,
  installed: installed(),
  rowKey: 'modrinth:p',
  root: undefined,
  requiredBy: [],
  depTotal: 0,
  problem: null,
  expanded: false,
  graphLoading: false,
  updateState: null,
  checking: false,
  packChip: null,
  selected: false,
  onToggleExpand() {},
  onOpenDetail() {},
  onOpenDetailMod() {},
  onToggle() {},
  onUninstall() {},
  onUpdate() {},
  onShowChangelog() {},
  onSelectChange() {},
  onInstallDep() {},
  onJump() {},
});
/** `a` precedes `b` in document order. */
const precedes = (a: Element, b: Element) =>
  (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;

describe('the relation cell in an installed row', () => {
  // The row's disclosure goes where every outline puts one: before the item's icon and name. The
  // icon keeps to its name on every row, with or without figures.
  it('sits after the checkbox and before the icon and the name', () => {
    render(InstalledModRow, { props: { ...rowProps(), depTotal: 1, requiredBy } });
    const card = screen.getByTestId('card-list-row');
    const box = within(card).getByRole('checkbox');
    const cell = within(card).getByTestId('relation-col');
    const icon = card.querySelector('[data-card-media]');
    expect(icon).not.toBeNull();
    expect(precedes(box, cell)).toBe(true);
    expect(cell.nextElementSibling).toBe(icon);
    expect(precedes(icon as Element, within(card).getByText('Alpha'))).toBe(true);
    expect(cell.contains(screen.getByTestId('relation-pill'))).toBe(true);
    expect(cell.className.split(/\s+/)).toContain('relation-col');
  });

  // An empty cell keeps its width, so the icon and the name after it line up with every other row.
  it('is there, empty, on a row with nothing to open', () => {
    render(InstalledModRow, { props: rowProps() });
    const cell = screen.getByTestId('relation-col');
    expect(cell.childElementCount).toBe(0);
    expect(screen.queryByTestId('relation-pill')).toBeNull();
  });

  it('is at the same place on a jar from no platform', () => {
    const manual = installed({ source: null, project_id: null, version_id: null });
    render(InstalledModRow, {
      props: { ...rowProps(), summary: null, installed: manual as never },
    });
    const card = screen.getByTestId('manual-mod-row');
    const cell = within(card).getByTestId('relation-col');
    expect(precedes(within(card).getByRole('checkbox'), cell)).toBe(true);
    expect(cell.nextElementSibling).toBe(card.querySelector('[data-card-media]'));
  });

  // Each figure keeps to its slot: a mod that others require but that needs nothing keeps an
  // empty ⛓ slot, so its ↑ stands where every other row's ↑ stands.
  it('keeps the ⛓ slot, empty, when only ↑ shows', () => {
    render(InstalledModRow, { props: { ...rowProps(), requiredBy } });
    const pill = screen.getByTestId('relation-pill');
    const dep = within(pill).getByTestId('relation-dep');
    const by = within(pill).getByTestId('relation-by');
    expect(dep.textContent?.trim()).toBe('');
    expect(dep.className.split(/\s+/)).toContain('relation-slot-dep');
    expect(by.textContent?.trim()).toBe('1');
    expect(by.className.split(/\s+/)).toContain('relation-slot-by');
    expect(precedes(dep, by)).toBe(true);
  });

  // The disclosure pattern: the cell names the section it opens — only while it is in the DOM
  // (a reference to an id that is not there is the same fault as a stale describedby).
  it('names the section it controls while that section is rendered', async () => {
    const props = { ...rowProps(), root, depTotal: 1 };
    const { rerender } = render(InstalledModRow, { props });
    expect(screen.getByTestId('relation-pill').hasAttribute('aria-controls')).toBe(false);
    await rerender({ ...props, expanded: true });
    const id = screen.getByTestId('relation-pill').getAttribute('aria-controls');
    expect(id).toBe('dep-section-a');
    expect(document.getElementById(id as string)?.contains(screen.getByRole('tree'))).toBe(true);
  });

  it('still toggles the section', async () => {
    const onToggleExpand = vi.fn();
    render(InstalledModRow, { props: { ...rowProps(), depTotal: 1, onToggleExpand } });
    await fireEvent.click(screen.getByTestId('relation-pill'));
    expect(onToggleExpand).toHaveBeenCalledOnce();
  });
});

describe('a catalog row', () => {
  it('has no relation cell', () => {
    render(ModCard, {
      props: {
        summary,
        installed: null,
        layout: 'list',
        onInstall() {},
        onOpenDetail() {},
        onToggle() {},
        onUninstall() {},
      },
    });
    expect(screen.queryByTestId('relation-col')).toBeNull();
  });
});

// ---------------------------------------------------------------------------------------------
// The column across the list
// ---------------------------------------------------------------------------------------------
describe('the relation column across the list', () => {
  // One width on every row, page and filter: the widest figure over the whole profile — here the
  // twelve dependencies of a mod the search then hides.
  it('is as wide as the widest figure of the profile, whatever the filter shows', async () => {
    render(InstalledModsView, {
      props: { instanceId: 'relcol-width', mcVersion: '1.21.1', loader: 'fabric' as const },
    });
    const list = await waitFor(() => screen.getByTestId('installed-list'));
    await waitFor(() => expect(list.style.getPropertyValue('--rel-dep-ch')).toBe('2'));
    expect(list.style.getPropertyValue('--rel-by-ch')).toBe('1');

    const search = screen.getByRole('searchbox', { name: 'Filter installed mods' });
    await fireEvent.input(search, { target: { value: 'Lib' } });
    await waitFor(() => expect(document.querySelectorAll('[data-mod-row]')).toHaveLength(1));
    expect(screen.getByTestId('installed-list').style.getPropertyValue('--rel-dep-ch')).toBe('2');
  });
});
