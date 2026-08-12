// The "What are data packs?" concept explainer, exercised on the instance
// library surface (InstalledDatapacksView).
//
// tests/help-popover-paragraphs.test.ts already covers HelpPopover's own
// multi-paragraph rendering with synthetic strings. This file covers the
// WIRING instead: that DatapackConceptHelp passes the four
// `onboarding.datapackConcept.pN` keys, in order, through explainKey — so a
// wrong key, a dropped paragraph, or a missing explanation-level swap fails
// here rather than only in front of a user. The expected fragments below are
// verbatim substrings of the real EN values in src/lib/i18n/locales/en.json.
//
// WorldDatapacks and ServerDatapacksInstalled render the very same
// DatapackConceptHelp component, so the copy assertions are not repeated per
// surface. The left-slot test at the bottom is the exception: that layout is
// specific to this toolbar.

import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Backend seam: the library view fetches its rows and the running-instance
// state on mount, and subscribes to the process events. An empty library is
// enough — the toolbar (and therefore the explainer) renders regardless.
const { datapacksListLibrary, runningInstances, spawnListen, exitListen } = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  spawnListen: vi.fn(),
  exitListen: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { datapacksListLibrary, runningInstances },
  events: {
    processSpawned: { listen: spawnListen },
    processExited: { listen: exitListen },
  },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';
import { explanationState } from '$lib/onboarding/explanation-level.svelte';

const TRIGGER = /what are data packs\?/i;

beforeEach(() => {
  datapacksListLibrary.mockResolvedValue({ status: 'ok', data: { entries: [] } });
  runningInstances.mockResolvedValue([]);
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
  render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
  const trigger = await screen.findByRole('button', { name: TRIGGER });
  await fireEvent.click(trigger);
  return [...popoverEl().querySelectorAll('p')];
}

describe('datapack concept help — instance library surface', () => {
  it('is closed until the trigger is clicked', async () => {
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    const trigger = await screen.findByRole('button', { name: TRIGGER });
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    // No concept copy on screen before the click — the explainer must not
    // occupy the toolbar row it sits in.
    expect(screen.queryByText(/data packs change/i)).toBeNull();
  });

  it('surfaces all four concept paragraphs, in order, at the Advanced level', async () => {
    explanationState.level = 'advanced';
    const paragraphs = await openExplainer();
    expect(paragraphs).toHaveLength(4);
    // p1 — the definition. "loot tables" is the Advanced-only wording.
    expect(paragraphs[0].textContent).toContain('recipes, loot tables, advancements');
    // p2 — not a mod, lives in a world.
    expect(paragraphs[1].textContent).toContain('a data pack is not code');
    // p3 — the library/worlds workflow.
    expect(paragraphs[2].textContent).toContain("instance's library");
    // p4 — the Beta caveat.
    expect(paragraphs[3].textContent).toContain('Beta');
  });

  it('swaps in the Basic wording at the Basic explanation level', async () => {
    explanationState.level = 'basic';
    const paragraphs = await openExplainer();
    expect(paragraphs).toHaveLength(4);
    expect(paragraphs[0].textContent).toContain('parts of the game itself');
    // The Advanced phrasing must be gone, not merely accompanied — otherwise a
    // broken explainKey mapping would still pass the assertion above.
    expect(paragraphs[0].textContent).not.toContain('loot tables');
  });
});

// The toolbar row is `justify-end`, so exactly one child may carry `mr-auto` —
// it is what pins the left slot. The explainer and the gate note share one
// always-rendered wrapper that owns it: were `mr-auto` moved onto the (?)
// alone, the note would lose the left slot and slide across to the buttons.
describe('datapack concept help — toolbar left slot', () => {
  it('pins the explainer and the gate note together in the single left slot', async () => {
    runningInstances.mockResolvedValue([{ instance_id: 'inst-1' }]);
    render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
    const trigger = await screen.findByRole('button', { name: TRIGGER });
    const row = screen.getByTestId('installed-datapacks').firstElementChild as HTMLElement;
    const leftSlot = trigger.closest('div.mr-auto');
    expect(leftSlot).not.toBeNull();
    expect(leftSlot?.parentElement).toBe(row);
    // The running instance gates every mutation, so the note is on screen —
    // inside that same wrapper, not stranded next to the buttons.
    const note = await screen.findByTestId('datapacks-gate-note');
    expect(note.parentElement).toBe(leftSlot);
    expect(note.className).not.toContain('mr-auto');
    expect(row.querySelectorAll('.mr-auto')).toHaveLength(1);
  });
});
