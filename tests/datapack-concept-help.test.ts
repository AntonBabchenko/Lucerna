// The "What are data packs?" concept explainer.
//
// tests/help-popover-paragraphs.test.ts already covers HelpPopover's own
// multi-paragraph rendering with synthetic strings. This file covers the
// WIRING: that DatapackConceptHelp passes its five `onboarding.datapackConcept`
// leaves (p1, p2, choose, p3, p4), in order, through explainKey — so a wrong
// key, a dropped paragraph, or a missing explanation-level swap fails here
// rather than only in front of a user. The expected fragments are verbatim
// substrings of the real EN values in src/lib/i18n/locales/en.json.
//
// Copy is asserted once, on the component. Surfaces pin PRESENCE only: the
// Add-ons kind row (tests/addons-tab.test.ts), a world's Datapacks tab
// (tests/world-datapacks.test.ts) and the server pane (below). The instance
// library toolbar is the one surface that must NOT carry it any more — the
// kind row above it already does, and two (?) on one screen was the defect.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Backend seam for the two panes rendered below: the library view fetches its
// rows and the running-instance state on mount, and subscribes to the process
// events. An empty library is enough — the toolbar renders regardless.
const { datapacksListLibrary, runningInstances, serverListDatapacks, spawnListen, exitListen } =
  vi.hoisted(() => ({
    datapacksListLibrary: vi.fn(),
    runningInstances: vi.fn(),
    serverListDatapacks: vi.fn(),
    spawnListen: vi.fn(),
    exitListen: vi.fn(),
  }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { datapacksListLibrary, runningInstances, serverListDatapacks },
  events: {
    processSpawned: { listen: spawnListen },
    processExited: { listen: exitListen },
  },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';
import DatapackConceptHelp from '$lib/onboarding/DatapackConceptHelp.svelte';
import { explanationState } from '$lib/onboarding/explanation-level.svelte';
import ServerDatapacksInstalled from '$lib/servers/datapacks/ServerDatapacksInstalled.svelte';

const TRIGGER = /what are data packs\?/i;

beforeEach(() => {
  datapacksListLibrary.mockResolvedValue({
    status: 'ok',
    data: { entries: [], worlds: [] },
  });
  runningInstances.mockResolvedValue([]);
  serverListDatapacks.mockResolvedValue({
    status: 'ok',
    data: { level_dat: 'present', entries: [] },
  });
  spawnListen.mockResolvedValue(() => {});
  exitListen.mockResolvedValue(() => {});
});

// explanationState is a module singleton; leaving it flipped would leak the
// level into any test that renders an adaptive surface afterwards.
afterEach(() => {
  explanationState.level = 'basic';
  vi.clearAllMocks();
});

/** The popover element, located via the trigger's aria-controls (the id is
 *  generated per HelpPopover instance, so it cannot be hardcoded). */
function popoverEl(): HTMLElement {
  const trigger = screen.getByRole('button', { name: TRIGGER });
  const id = trigger.getAttribute('aria-controls') as string;
  const el = document.getElementById(id);
  if (!el) throw new Error('concept popover is not open');
  return el;
}

async function openExplainer(): Promise<HTMLParagraphElement[]> {
  render(DatapackConceptHelp);
  const trigger = await screen.findByRole('button', { name: TRIGGER });
  await fireEvent.click(trigger);
  return [...popoverEl().querySelectorAll('p')];
}

describe('datapack concept help — copy', () => {
  it('is closed until the trigger is clicked', async () => {
    render(DatapackConceptHelp);
    const trigger = await screen.findByRole('button', { name: TRIGGER });
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText(/data packs change/i)).toBeNull();
  });

  it('surfaces all five concept paragraphs, in order, at the Advanced level', async () => {
    explanationState.level = 'advanced';
    const paragraphs = await openExplainer();
    expect(paragraphs).toHaveLength(5);
    // p1 — the definition. "loot tables" is the Advanced-only wording.
    expect(paragraphs[0].textContent).toContain('recipes, loot tables, advancements');
    // p2 — not a mod, lives in a world.
    expect(paragraphs[1].textContent).toContain('a data pack is not code');
    // choose — when a data pack is the right pick over a mod.
    expect(paragraphs[2].textContent).toContain('Pick a data pack when');
    // p3 — the library/worlds workflow.
    expect(paragraphs[3].textContent).toContain("instance's library");
    // p4 — the Beta caveat.
    expect(paragraphs[4].textContent).toContain('Beta');
  });

  it('swaps in the Basic wording at the Basic explanation level', async () => {
    explanationState.level = 'basic';
    const paragraphs = await openExplainer();
    expect(paragraphs).toHaveLength(5);
    expect(paragraphs[0].textContent).toContain('parts of the game itself');
    // The Advanced phrasing must be gone, not merely accompanied — otherwise a
    // broken explainKey mapping would still pass the assertion above.
    expect(paragraphs[0].textContent).not.toContain('loot tables');
    expect(paragraphs[2].textContent).toContain("the way to change the game's rules");
  });
});

// PRESENCE on the server surface — ServerAddonsTab is the only thing that
// renders ServerDatapacksInstalled, and tests/server-addons-tab.test.ts stubs
// it out with a no-op component. So the pane is mounted directly here.
describe('datapack concept help — server surface', () => {
  it('renders the help trigger in the server datapack toolbar', async () => {
    render(ServerDatapacksInstalled, { props: { serverId: 'srv-1', mcVersion: '1.20.4' } });
    expect(await screen.findByRole('button', { name: TRIGGER })).toBeTruthy();
  });
});

// ABSENCE on the instance library: the Add-ons kind row carries the explainer
// for both Browse and Installed, so the toolbar no longer repeats it. The row
// is `justify-end`; exactly one child carries `mr-auto` and pins the left
// slot, which now holds only the gate note.
describe('instance library toolbar', () => {
  it('carries no data pack explainer', async () => {
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    await screen.findByTestId('installed-datapacks');
    await waitFor(() => expect(datapacksListLibrary).toHaveBeenCalled());
    expect(screen.queryByRole('button', { name: TRIGGER })).toBeNull();
  });

  it('keeps the gate note alone in the single left slot', async () => {
    runningInstances.mockResolvedValue([{ instance_id: 'inst-1' }]);
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    const note = await screen.findByTestId('datapacks-gate-note');
    const row = screen.getByTestId('installed-datapacks').firstElementChild as HTMLElement;
    const leftSlot = note.parentElement as HTMLElement;
    expect(leftSlot.classList.contains('mr-auto')).toBe(true);
    expect(leftSlot.parentElement).toBe(row);
    expect(note.className).not.toContain('mr-auto');
    expect(row.querySelectorAll('.mr-auto')).toHaveLength(1);
  });
});
