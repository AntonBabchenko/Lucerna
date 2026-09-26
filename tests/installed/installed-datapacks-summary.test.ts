// The collapsed library row's world summary. A world Lucerna could not check
// (its data pack folder or level file unreadable: placement state `null`) was
// counted in "of N" as if the pack were off there — "Enabled in 0 of 1 world"
// for a pack whose only world could not be checked, while the sub-row said
// "State unknown". Such a world is now shown apart and counted in neither
// number.
import { render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const cmd = vi.hoisted(() => ({
  datapacksListLibrary: vi.fn(),
  runningInstances: vi.fn(),
  modsProjects: vi.fn(),
}));
const ev = vi.hoisted(() => ({ spawn: vi.fn(), exit: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: cmd,
  events: { processSpawned: { listen: ev.spawn }, processExited: { listen: ev.exit } },
}));

import InstalledDatapacksView from '$lib/mods/InstalledDatapacksView.svelte';

function placement(world: string, state: string | null) {
  return { world, state, ignored_reason: null, level_dat: 'present' };
}
function library(placements: ReturnType<typeof placement>[]) {
  cmd.datapacksListLibrary.mockResolvedValue({
    status: 'ok',
    data: {
      entries: [
        {
          pack: {
            filename: 'vm.zip',
            sha1: 'a'.repeat(40),
            size_bytes: 1024,
            name: 'VeinMiner',
            source: null,
            project_id: null,
            version_id: null,
            version_number: null,
            installed_at: '2026-09-24T00:00:00Z',
          },
          in_library: true,
          compat: { kind: 'unknown' },
          placements,
        },
      ],
      worlds: placements.map((p) => ({ world: p.world, level_dat: p.level_dat })),
    },
  });
}

beforeEach(() => {
  cmd.runningInstances.mockResolvedValue([]);
  ev.spawn.mockResolvedValue(() => {});
  ev.exit.mockResolvedValue(() => {});
});
afterEach(() => {
  vi.clearAllMocks();
  locale.set('en');
});

/** The collapsed row: the expander's row, which holds the summary badges. */
async function row(): Promise<HTMLElement> {
  render(InstalledDatapacksView, { props: { instanceId: 'inst-1' } });
  const expand = await screen.findByTestId('datapack-row-expand');
  return expand.parentElement as HTMLElement;
}

describe('InstalledDatapacksView — the collapsed row summary', () => {
  it('does not count a world it could not check as one the pack is off in', async () => {
    library([placement('Alpha', null)]);
    const r = await row();
    // No "0 of 1" and no "In no world": both would be guesses about Alpha.
    expect(r.textContent).not.toMatch(/Enabled in 0 of 1/);
    expect(r.textContent).not.toMatch(/In no world/);
    expect(within(r).queryByTestId('datapack-world-summary')).toBeNull();
    expect(within(r).getByTestId('datapack-world-summary-unchecked').textContent).toMatch(
      /Couldn't check 1 world/,
    );
  });

  it('counts the checked worlds and names the unchecked one apart', async () => {
    library([placement('Beta', 'enabled'), placement('Alpha', null)]);
    const r = await row();
    expect(within(r).getByTestId('datapack-world-summary').textContent).toMatch(
      /Enabled in 1 of 1 world/,
    );
    expect(within(r).getByTestId('datapack-world-summary-unchecked').textContent).toMatch(
      /Couldn't check 1 world/,
    );
  });

  it('reads naturally in Russian', async () => {
    locale.set('ru');
    library([placement('Beta', 'enabled'), placement('Alpha', null), placement('Gamma', null)]);
    const r = await row();
    expect(within(r).getByTestId('datapack-world-summary').textContent).toMatch(
      /Включён в 1 из 1 мира/,
    );
    expect(within(r).getByTestId('datapack-world-summary-unchecked').textContent).toMatch(
      /Не удалось проверить 2 мира/,
    );
  });
});
