import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import { formatError } from '$lib/ipc/format-error';

const h = vi.hoisted(() => ({
  modsRemovalImpact: vi.fn(),
  modsEnableImpact: vi.fn(),
  modsDisable: vi.fn(),
  modsEnable: vi.fn(),
  modsUninstall: vi.fn(),
  modsUninstallMany: vi.fn(),
  modsRestoreUninstalled: vi.fn(),
  modsFindOrphans: vi.fn(),
  pushActionToast: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsRemovalImpact: h.modsRemovalImpact,
    modsEnableImpact: h.modsEnableImpact,
    modsDisable: h.modsDisable,
    modsEnable: h.modsEnable,
    modsUninstall: h.modsUninstall,
    modsUninstallMany: h.modsUninstallMany,
    modsRestoreUninstalled: h.modsRestoreUninstalled,
    modsFindOrphans: h.modsFindOrphans,
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushActionToast: h.pushActionToast,
  pushSuccess: h.pushSuccess,
  pushWarning: h.pushWarning,
}));

import {
  __resetModOpsForTests,
  disableMods,
  enableMods,
  enableModsUnguarded,
  UNDO_TTL_MS,
  uninstallMods,
} from '$lib/mods/mod-ops.svelte';
import ModOpsHost from '$lib/mods/ops/ModOpsHost.svelte';
import ModOpsHostToggle from './fixtures/ModOpsHostToggle.svelte';

const ok = <T>(data: T) => ({ status: 'ok' as const, data });
// A mod write takes the SHARED claim: it is refused only while a long operation holds the
// profile, never because the game runs (plan A9) — so the reason must not say the game runs.
const BUSY =
  'Another operation — such as a modpack update, a migration or a clone — is using this profile. Try again once it finishes.';
const busyErr = { status: 'error' as const, error: { kind: 'instance_busy' as const } };
const ioErr = {
  status: 'error' as const,
  error: { kind: 'io' as const, path: 'mods', details: 'denied' },
};
const scope = { instanceId: 'inst', profileName: 'Alpha Pack' };
const sodium = { sha1: 's', name: 'Sodium' };
const indium = { sha1: 'i', name: 'Indium', needs: ['Sodium'] };
// What mods_removal_impact answers for Sodium, which Indium needs: `order` is the one safe
// disable order over the targets and the dependents — Indium before Sodium.
const sodiumImpact = ok({ dependents: [indium], order: ['i', 's'] });
type ToastCall = [
  string,
  string,
  { label: string; run: () => void },
  string[],
  { ttlMs?: number }?,
];
const toast = (n: number) => h.pushActionToast.mock.calls[n] as ToastCall;
const host = (activeInstanceId = 'inst') => render(ModOpsHost, { props: { activeInstanceId } });
/** The sha1s a flip command was called with, in call order. */
const flipped = (fn: typeof h.modsDisable) => fn.mock.calls.map((c) => c[1] as string);
/** A promise the test settles by hand — a mutation still running. */
function deferred<T>() {
  let resolve: (v: T) => void = () => {};
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
/** One macrotask: every microtask-only continuation (a flow after its IPC answer) has run. */
const flush = () => new Promise<void>((r) => setTimeout(r, 0));
/** What `p` settled with, or 'still waiting' when it has not settled by the next macrotask. */
const settledSoon = <T>(p: Promise<T>) => Promise.race([p, flush().then(() => 'still waiting')]);

beforeAll(() => locale.set('en'));
beforeEach(() => {
  vi.resetAllMocks();
  // Nothing to carry along: `order` names just the targets (none of these needs another).
  h.modsRemovalImpact.mockImplementation(async (_id: string, sha1s: string[]) =>
    ok({ dependents: [], order: sha1s }),
  );
  h.modsEnableImpact.mockImplementation(async (_id: string, sha1s: string[]) =>
    ok({ requirements: [], order: sha1s }),
  );
  h.modsDisable.mockResolvedValue(ok(null));
  h.modsEnable.mockResolvedValue(ok(null));
  h.modsUninstall.mockResolvedValue(
    ok({ token: 'tok', items: [{ sha1: 's', name: 'sodium-jar' }] }),
  );
  // `restored` holds display names (plan A6).
  h.modsRestoreUninstalled.mockResolvedValue(
    ok({ restored: ['Sodium'], skipped: [], expired: false }),
  );
  h.modsFindOrphans.mockResolvedValue(ok([]));
  __resetModOpsForTests();
});

describe('guarded disable', () => {
  it('disables at once when nothing depends on it', async () => {
    host();
    await expect(disableMods(scope, [sodium])).resolves.toBe('applied');
    expect(h.modsRemovalImpact).toHaveBeenCalledWith('inst', ['s']);
    expect(h.modsDisable).toHaveBeenCalledWith('inst', 's');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('names the mods that need it; «Disable all N» disables them too', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    host();
    const done = disableMods(scope, [sodium]);
    const dialog = await screen.findByRole('dialog', { name: 'Disable Sodium?' });
    expect(within(dialog).getByText('Indium')).toBeTruthy();
    const all = within(dialog).getByRole('button', { name: 'Disable all 2' });
    expect(all).toHaveBtnVariant('primary'); // disabling is reversible: never .btn-danger
    expect(within(dialog).getByRole('button', { name: 'Only this one' })).toHaveBtnVariant(
      'secondary',
    );
    await fireEvent.click(all);
    await expect(done).resolves.toBe('applied');
    // Dependents first: no step leaves an enabled mod without what it needs.
    expect(h.modsDisable.mock.calls).toEqual([
      ['inst', 'i'],
      ['inst', 's'],
    ]);
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('a dependent that could not be disabled keeps the mod it needs enabled', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    h.modsDisable.mockResolvedValueOnce(ioErr);
    host();
    const done = disableMods(scope, [sodium]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Disable all 2' }));
    await expect(done).resolves.toBe('failed');
    expect(h.modsDisable.mock.calls).toEqual([['inst', 'i']]);
    expect(h.pushWarning).toHaveBeenCalledWith(expect.stringContaining('2 failed'), [
      expect.any(String),
    ]);
  });

  it('«Only this one» leaves the dependents enabled', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    host();
    const done = disableMods(scope, [sodium]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Only this one' }));
    await expect(done).resolves.toBe('applied');
    expect(h.modsDisable.mock.calls).toEqual([['inst', 's']]);
  });

  it('Cancel changes nothing', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    host();
    const done = disableMods(scope, [sodium]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    await expect(done).resolves.toBe('cancelled');
    expect(h.modsDisable).not.toHaveBeenCalled();
  });

  it('a check that could not run asks instead of assuming nothing depends on it', async () => {
    h.modsRemovalImpact.mockResolvedValue(ioErr);
    host();
    const done = disableMods(scope, [sodium]);
    const dialog = await screen.findByRole('dialog', {
      name: "Couldn't check whether other mods depend on it",
    });
    expect(h.modsDisable).not.toHaveBeenCalled();
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Disable anyway' }));
    await expect(done).resolves.toBe('applied');
    expect(h.modsDisable).toHaveBeenCalledWith('inst', 's');
  });

  it('a long operation took the profile mid-dialog: the refusal closes the dialog and says why', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    h.modsDisable.mockResolvedValue(busyErr);
    host();
    const done = disableMods(scope, [sodium]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Disable all 2' }));
    await expect(done).resolves.toBe('failed');
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(h.pushWarning).toHaveBeenCalledWith(expect.stringContaining('2 failed'), [BUSY]);
    expect(JSON.stringify(h.pushWarning.mock.calls)).not.toMatch(/game/i);
  });
});

describe('guarded enable', () => {
  it('asks to enable the disabled mods it needs together with it', async () => {
    h.modsEnableImpact.mockResolvedValue(
      ok({ requirements: [{ sha1: 's', name: 'Sodium' }], order: ['s', 'i'] }),
    );
    host();
    const done = enableMods(scope, [{ sha1: 'i', name: 'Indium' }]);
    const dialog = await screen.findByRole('dialog', {
      name: 'Enable Indium together with Sodium?',
    });
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Enable together' }));
    await expect(done).resolves.toBe('applied');
    // Requirements first: the mod never comes up without what it needs.
    expect(h.modsEnable.mock.calls).toEqual([
      ['inst', 's'],
      ['inst', 'i'],
    ]);
  });

  it('a bulk enable asks the impact ONCE for all targets (plan A5)', async () => {
    host();
    const targets = [
      { sha1: 'a', name: 'Alpha' },
      { sha1: 'b', name: 'Beta' },
    ];
    await expect(enableMods(scope, targets, { bulk: true })).resolves.toBe('applied');
    expect(h.modsEnableImpact.mock.calls).toEqual([['inst', ['a', 'b']]]);
    expect(h.pushSuccess).toHaveBeenCalledWith('Enabled 2 mods', []);
  });

  it('turns requirements on in the order the backend gives, so a chain never runs half-enabled', async () => {
    // Indium needs Sodium, Sodium needs Fabric API: mods_enable_impact orders them safely —
    // Fabric API before Sodium, which needs it, and both before Indium — and the flip keeps it.
    h.modsEnableImpact.mockResolvedValue(
      ok({
        requirements: [
          { sha1: 'f', name: 'Fabric API' },
          { sha1: 's', name: 'Sodium' },
        ],
        order: ['f', 's', 'i'],
      }),
    );
    host();
    const done = enableMods(scope, [{ sha1: 'i', name: 'Indium' }]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Enable together' }));
    await expect(done).resolves.toBe('applied');
    expect(h.modsEnable.mock.calls).toEqual([
      ['inst', 'f'],
      ['inst', 's'],
      ['inst', 'i'],
    ]);
  });
});

describe('unguarded enable — the repair, whose click is the consent (plan A13)', () => {
  it('switches the targets on with what they need, in the given order, without asking', async () => {
    h.modsEnableImpact.mockResolvedValue(
      ok({ requirements: [{ sha1: 'f', name: 'Fabric API' }], order: ['f', 's', 'i'] }),
    );
    host();
    await expect(enableModsUnguarded('inst', ['i', 's'])).resolves.toEqual({
      enabled: ['f', 's', 'i'],
      failed: [],
      reasons: [],
    });
    // One impact call for every target (plan A5).
    expect(h.modsEnableImpact.mock.calls).toEqual([['inst', ['i', 's']]]);
    expect(flipped(h.modsEnable)).toEqual(['f', 's', 'i']);
    expect(screen.queryByRole('dialog')).toBeNull();
    // Silent: the repair re-runs the pre-flight and reports what is left itself.
    expect(h.pushSuccess).not.toHaveBeenCalled();
    expect(h.pushWarning).not.toHaveBeenCalled();
  });

  it('stops at the first failure, so nothing comes on without what it needs', async () => {
    h.modsEnableImpact.mockResolvedValue(
      ok({ requirements: [{ sha1: 'f', name: 'Fabric API' }], order: ['f', 'i'] }),
    );
    h.modsEnable.mockResolvedValueOnce(ioErr);
    await expect(enableModsUnguarded('inst', ['i'])).resolves.toEqual({
      enabled: [],
      failed: ['f', 'i'],
      // The untried step failed for the same reason: said once.
      reasons: [formatError(ioErr.error)],
    });
    expect(flipped(h.modsEnable)).toEqual(['f']);
  });

  // The repair says why a step failed («Fixed 0 of N» alone cannot tell a held profile from
  // unrelated failures) — a refused flip in the busy-profile wording, never «the game is running».
  it('says why a flip failed — a busy profile as busy', async () => {
    h.modsEnableImpact.mockResolvedValue(ok({ requirements: [], order: ['i'] }));
    h.modsEnable.mockResolvedValueOnce(busyErr);
    await expect(enableModsUnguarded('inst', ['i'])).resolves.toEqual({
      enabled: [],
      failed: ['i'],
      reasons: [BUSY],
    });
  });

  it('with no order to follow — the check could not run — every target gets its try', async () => {
    h.modsEnableImpact.mockResolvedValue(ioErr);
    h.modsEnable.mockResolvedValueOnce(ioErr);
    host();
    await expect(enableModsUnguarded('inst', ['a', 'b'])).resolves.toEqual({
      enabled: ['b'],
      failed: ['a'],
      reasons: [formatError(ioErr.error)],
    });
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});

describe("flip order: the backend's, targets included", () => {
  // mods_removal_impact and mods_enable_impact answer with ONE safe flip `order` over the targets
  // and the mods they carry along (a stable topological sort over what each mod needs). Targets
  // may need each other, and a target may need a mod it carries along, so the targets do not
  // simply go last. Re-sorting or reversing the order here would undo it.
  const listed = [
    { sha1: 'c', name: 'Gamma' },
    { sha1: 'a', name: 'Alpha' },
    { sha1: 'b', name: 'Beta' },
  ];
  const dependents = listed.map((m) => ({ ...m, needs: ['Sodium'] }));
  const offImpact = ok({ dependents, order: ['c', 'a', 'b', 's'] });

  it('flips exactly in the given order — never re-sorted, never reversed', async () => {
    h.modsRemovalImpact.mockResolvedValue(offImpact);
    h.modsEnableImpact.mockResolvedValue(ok({ requirements: listed, order: ['c', 'a', 'b', 't'] }));
    host();
    const off = disableMods(scope, [sodium]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Disable all 4' }));
    await expect(off).resolves.toBe('applied');
    expect(flipped(h.modsDisable)).toEqual(['c', 'a', 'b', 's']);
    const on = enableMods(scope, [{ sha1: 't', name: 'Target' }]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Enable together' }));
    await expect(on).resolves.toBe('applied');
    expect(flipped(h.modsEnable)).toEqual(['c', 'a', 'b', 't']);
  });

  it('a target goes off ahead of a dependent it needs', async () => {
    // Sodium and Tweaks leave together; Dynamic Lights needs Sodium, and Tweaks needs Dynamic
    // Lights — so Tweaks goes off first. The dependents, then the targets, would switch Dynamic
    // Lights off while Tweaks still needs it.
    const lights = { sha1: 'd', name: 'Dynamic Lights', needs: ['Sodium'] };
    h.modsRemovalImpact.mockResolvedValue(ok({ dependents: [lights], order: ['t', 'd', 's'] }));
    host();
    const done = disableMods(scope, [sodium, { sha1: 't', name: 'Tweaks' }]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Disable all 3' }));
    await expect(done).resolves.toBe('applied');
    expect(flipped(h.modsDisable)).toEqual(['t', 'd', 's']);
  });

  it('a target comes on ahead of a requirement that needs it', async () => {
    // Tweaks and Utils come on together; Tweaks needs Reach, which is off, and Reach needs Utils
    // — so Utils comes on first. The requirements, then the targets, would switch Reach on
    // without Utils.
    h.modsEnableImpact.mockResolvedValue(
      ok({ requirements: [{ sha1: 'r', name: 'Reach' }], order: ['u', 'r', 't'] }),
    );
    host();
    const done = enableMods(scope, [
      { sha1: 't', name: 'Tweaks' },
      { sha1: 'u', name: 'Utils' },
    ]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Enable together' }));
    await expect(done).resolves.toBe('applied');
    expect(flipped(h.modsEnable)).toEqual(['u', 'r', 't']);
  });

  it('«Only these» flips just the targets, still in the given order', async () => {
    // Indium needs Sodium, and Xray needs both: switching only the two off, Indium goes first.
    const xray = { sha1: 'x', name: 'Xray', needs: ['Sodium', 'Indium'] };
    h.modsRemovalImpact.mockResolvedValue(ok({ dependents: [xray], order: ['x', 'i', 's'] }));
    host();
    const done = disableMods(scope, [sodium, { sha1: 'i', name: 'Indium' }]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Only these' }));
    await expect(done).resolves.toBe('applied');
    expect(flipped(h.modsDisable)).toEqual(['i', 's']);
  });

  it('targets that need each other stop at the first failure, the rest counted as failed', async () => {
    // Nothing else needs them, but Indium needs Sodium and Fabric API: it goes off first, and
    // when it cannot, the other two stay on — Indium keeps what it needs.
    h.modsRemovalImpact.mockResolvedValue(ok({ dependents: [], order: ['i', 's', 'f'] }));
    h.modsDisable.mockResolvedValueOnce(ioErr);
    host();
    const targets = [sodium, { sha1: 'i', name: 'Indium' }, { sha1: 'f', name: 'Fabric API' }];
    await expect(disableMods(scope, targets, { bulk: true })).resolves.toBe('failed');
    expect(flipped(h.modsDisable)).toEqual(['i']);
    expect(h.pushWarning).toHaveBeenCalledWith('Disabled 0 mods, 3 failed', [expect.any(String)]);
  });

  it('with no order to follow — the check could not run — every target gets its try', async () => {
    h.modsRemovalImpact.mockResolvedValue(ioErr);
    h.modsDisable.mockResolvedValueOnce(ioErr);
    host();
    const done = disableMods(scope, [sodium, { sha1: 'i', name: 'Indium' }], { bulk: true });
    await fireEvent.click(await screen.findByRole('button', { name: 'Disable anyway' }));
    await expect(done).resolves.toBe('applied');
    expect(flipped(h.modsDisable)).toEqual(['s', 'i']);
    expect(h.pushWarning).toHaveBeenCalledWith('Disabled 1 mod, 1 failed', [expect.any(String)]);
  });

  it('a removal disables its dependents in the order given, which may differ from their list', async () => {
    // Sodium and Tweaks leave; Dynamic Lights and Zoom need Sodium, and Tweaks needs Dynamic
    // Lights, so the order puts Zoom first. Undo re-enables them in reverse.
    const lights = { sha1: 'd', name: 'Dynamic Lights', needs: ['Sodium'] };
    const zoom = { sha1: 'z', name: 'Zoom', needs: ['Sodium'] };
    h.modsRemovalImpact.mockResolvedValue(
      ok({ dependents: [lights, zoom], order: ['z', 't', 'd', 's'] }),
    );
    h.modsUninstallMany.mockResolvedValue(
      ok({
        token: 't5',
        items: [
          { sha1: 's', name: 's' },
          { sha1: 't', name: 't' },
        ],
      }),
    );
    host();
    const done = uninstallMods(scope, [sodium, { sha1: 't', name: 'Tweaks' }]);
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Remove and disable 2 dependents' }),
    );
    await expect(done).resolves.toBe('applied');
    expect(flipped(h.modsDisable)).toEqual(['z', 'd']);
    toast(0)[2].run();
    await waitFor(() => expect(h.pushSuccess).toHaveBeenCalledWith('Restored 1 mod', []));
    expect(flipped(h.modsEnable)).toEqual(['d', 'z']);
  });

  it('a removal disables its dependents in the listed order; Undo re-enables them in reverse, providers first', async () => {
    h.modsRemovalImpact.mockResolvedValue(offImpact);
    host();
    const done = uninstallMods(scope, [sodium]);
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Remove and disable 3 dependents' }),
    );
    await expect(done).resolves.toBe('applied');
    expect(flipped(h.modsDisable)).toEqual(['c', 'a', 'b']);
    toast(0)[2].run();
    await waitFor(() => expect(h.pushSuccess).toHaveBeenCalledWith('Restored 1 mod', []));
    expect(flipped(h.modsEnable)).toEqual(['b', 'a', 'c']);
  });

  it('after a removal, a dependent that will not switch off does not stop the others', async () => {
    h.modsRemovalImpact.mockResolvedValue(offImpact);
    h.modsDisable.mockResolvedValueOnce(ioErr);
    host();
    const done = uninstallMods(scope, [sodium]);
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Remove and disable 3 dependents' }),
    );
    await expect(done).resolves.toBe('applied');
    // The removal already happened: every dependent still gets its try.
    expect(flipped(h.modsDisable)).toEqual(['c', 'a', 'b']);
    expect(toast(0)[3]).toEqual([
      'Also disabled: Alpha, Beta',
      expect.stringMatching(/^Couldn't disable Gamma: /),
    ]);
  });

  it('Undo stops at the first dependent it cannot switch back on, so none comes on without what it needs', async () => {
    h.modsRemovalImpact.mockResolvedValue(offImpact);
    host();
    const done = uninstallMods(scope, [sodium]);
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Remove and disable 3 dependents' }),
    );
    await done;
    h.modsEnable.mockResolvedValueOnce(busyErr);
    toast(0)[2].run();
    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith('Restored 1 mod', [
        `Couldn't re-enable Beta: ${BUSY}`,
        `Couldn't re-enable Alpha: ${BUSY}`,
        `Couldn't re-enable Gamma: ${BUSY}`,
      ]),
    );
    expect(flipped(h.modsEnable)).toEqual(['b']);
  });
});

describe('one dialog at a time', () => {
  it('a question never replaces a dialog whose operation still runs: it shows once that one closes', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    h.modsEnableImpact.mockResolvedValue(
      ok({ requirements: [{ sha1: 'f', name: 'Fabric API' }], order: ['f', 'b'] }),
    );
    const running = deferred<ReturnType<typeof ok<null>>>();
    h.modsDisable.mockReturnValueOnce(running.promise);
    host();
    const first = disableMods(scope, [sodium]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Disable all 2' }));
    const second = enableMods(scope, [{ sha1: 'b', name: 'Beta' }]);
    await flush();
    expect(h.modsEnableImpact).toHaveBeenCalledTimes(1); // the second flow has asked by now
    // The first dialog stays, its button still spinning (the spinner's status label joins the
    // button's name); the new question waits.
    within(screen.getByRole('dialog', { name: 'Disable Sodium?' })).getByRole('button', {
      name: /Disable all 2/,
      busy: true,
    });
    expect(
      screen.queryByRole('dialog', { name: 'Enable Beta together with Fabric API?' }),
    ).toBeNull();
    running.resolve(ok(null));
    await expect(first).resolves.toBe('applied');
    const next = await screen.findByRole('dialog', {
      name: 'Enable Beta together with Fabric API?',
    });
    await fireEvent.click(within(next).getByRole('button', { name: 'Cancel' }));
    await expect(second).resolves.toBe('cancelled');
    expect(h.modsEnable).not.toHaveBeenCalled();
  });

  it('a removal that disables its dependents can still offer the libraries nothing else needs', async () => {
    // The flow's own follow-up question takes its own running dialog's place: waiting for that
    // dialog to close would wait for itself.
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    h.modsFindOrphans.mockResolvedValue(
      ok([{ sha1: 'c', name: 'Cloth Config', project_id: 'cc' }]),
    );
    h.modsUninstallMany.mockResolvedValue(
      ok({
        token: 't4',
        items: [
          { sha1: 's', name: 's' },
          { sha1: 'c', name: 'c' },
        ],
      }),
    );
    host();
    const done = uninstallMods(scope, [sodium], { offerOrphans: true });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Remove and disable 1 dependent' }),
    );
    const offer = await screen.findByRole('dialog', {
      name: 'Also remove libraries nothing else needs?',
    });
    await fireEvent.click(within(offer).getByRole('checkbox'));
    await fireEvent.click(within(offer).getByRole('button', { name: /uninstall 2 mods/i }));
    await expect(settledSoon(done)).resolves.toBe('applied');
    expect(h.modsUninstallMany).toHaveBeenCalledWith('inst', ['s', 'c']);
    expect(flipped(h.modsDisable)).toEqual(['i']);
  });
});

describe('when the host goes away', () => {
  it('a question still waiting for an answer settles as cancelled', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    const { rerender } = render(ModOpsHostToggle, { props: { shown: true } });
    const done = disableMods(scope, [sodium]);
    await screen.findByRole('dialog', { name: 'Disable Sodium?' });
    await rerender({ shown: false });
    expect(screen.queryByRole('dialog')).toBeNull();
    await expect(settledSoon(done)).resolves.toBe('cancelled');
    expect(h.modsDisable).not.toHaveBeenCalled();
  });

  it('so does a question waiting behind a running operation, which itself runs to its end', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    h.modsEnableImpact.mockResolvedValue(
      ok({ requirements: [{ sha1: 'f', name: 'Fabric API' }], order: ['f', 'b'] }),
    );
    const running = deferred<ReturnType<typeof ok<null>>>();
    h.modsDisable.mockReturnValueOnce(running.promise);
    const { rerender } = render(ModOpsHostToggle, { props: { shown: true } });
    const first = disableMods(scope, [sodium]);
    await fireEvent.click(await screen.findByRole('button', { name: 'Disable all 2' }));
    const second = enableMods(scope, [{ sha1: 'b', name: 'Beta' }]);
    await flush();
    await rerender({ shown: false });
    await expect(settledSoon(second)).resolves.toBe('cancelled');
    running.resolve(ok(null));
    await expect(settledSoon(first)).resolves.toBe('applied');
    expect(h.modsEnable).not.toHaveBeenCalled();
  });
});

describe('uninstall and undo', () => {
  it('removes at once and offers a 10-second Undo that restores by token', async () => {
    host();
    await expect(uninstallMods(scope, [sodium])).resolves.toBe('applied');
    expect(h.modsUninstall).toHaveBeenCalledWith('inst', 's');
    const [kind, title, action, lines, opts] = toast(0);
    expect([kind, title, action.label, lines, opts]).toEqual([
      'success',
      'Removed Sodium',
      'Undo',
      [],
      { ttlMs: UNDO_TTL_MS },
    ]);
    expect(UNDO_TTL_MS).toBe(10_000);
    action.run();
    await waitFor(() => expect(h.pushSuccess).toHaveBeenCalledWith('Restored 1 mod', []));
    expect(h.modsRestoreUninstalled).toHaveBeenCalledWith('inst', 'tok');
  });

  it('nothing moved (the mod was already gone): no Undo to offer', async () => {
    h.modsUninstall.mockResolvedValue(ok({ token: 'tok', items: [] }));
    host();
    await expect(uninstallMods(scope, [sodium])).resolves.toBe('applied');
    expect(h.pushActionToast).not.toHaveBeenCalled();
  });

  it('a mod others need: «Remove and disable 1 dependent», dependents disabled AFTER the removal', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    host();
    const done = uninstallMods(scope, [sodium]);
    const dialog = await screen.findByRole('dialog', { name: 'Remove Sodium?' });
    const primary = within(dialog).getByRole('button', { name: 'Remove and disable 1 dependent' });
    expect(primary).toHaveBtnVariant('danger');
    await fireEvent.click(primary);
    await expect(done).resolves.toBe('applied');
    expect(h.modsUninstall.mock.invocationCallOrder[0]).toBeLessThan(
      h.modsDisable.mock.invocationCallOrder[0],
    );
    expect(toast(0)[3]).toEqual(['Also disabled: Indium']);
    // Undo reverses the whole operation: once everything is back, the dependent comes back on.
    toast(0)[2].run();
    await waitFor(() => expect(h.modsEnable).toHaveBeenCalledWith('inst', 'i'));
  });

  it('an Undo that could not bring everything back leaves the dependents disabled and says so', async () => {
    h.modsRemovalImpact.mockResolvedValue(sodiumImpact);
    host();
    const done = uninstallMods(scope, [sodium]);
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Remove and disable 1 dependent' }),
    );
    await done;
    h.modsRestoreUninstalled.mockResolvedValueOnce(
      ok({
        restored: [],
        skipped: [{ name: 'Sodium', reason: 'already_installed' }],
        expired: false,
      }),
    );
    toast(0)[2].run();
    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith("Couldn't restore", [
        'Sodium is installed again',
        'Still disabled: Indium',
      ]),
    );
    expect(h.modsEnable).not.toHaveBeenCalled();
  });

  it('a bulk removal is ONE call, ONE token, ONE toast', async () => {
    h.modsUninstallMany.mockResolvedValue(
      ok({
        token: 't2',
        items: [
          { sha1: 's', name: 'a' },
          { sha1: 'i', name: 'b' },
        ],
      }),
    );
    host();
    await uninstallMods(scope, [sodium, { sha1: 'i', name: 'Indium' }]);
    expect(h.modsUninstallMany.mock.calls).toEqual([['inst', ['s', 'i']]]);
    expect(h.modsUninstall).not.toHaveBeenCalled();
    expect(h.pushActionToast).toHaveBeenCalledTimes(1);
    expect(toast(0).slice(1, 2)).toEqual(['Removed 2 mods']);
    expect(toast(0)[3]).toEqual(['Sodium', 'Indium']);
  });

  it('a refused removal warns, removes nothing and offers no Undo', async () => {
    h.modsUninstall.mockResolvedValue(busyErr);
    host();
    await expect(uninstallMods(scope, [sodium])).resolves.toBe('failed');
    expect(h.pushWarning).toHaveBeenCalledWith("Couldn't remove Sodium", [BUSY]);
    expect(h.pushActionToast).not.toHaveBeenCalled();
  });

  it('a restore refused while the profile is busy becomes a sticky toast that retries', async () => {
    host();
    await uninstallMods(scope, [sodium]);
    h.modsRestoreUninstalled.mockResolvedValueOnce(busyErr);
    toast(0)[2].run();
    await waitFor(() => expect(h.pushActionToast).toHaveBeenCalledTimes(2));
    const [kind, title, action, , opts] = toast(1);
    expect(kind).toBe('warning');
    expect(title).toBe("Can't restore Sodium while another operation is using the profile");
    expect(action.label).toBe('Undo');
    expect(opts).toBeUndefined(); // no TTL: it waits for the user
    action.run();
    await waitFor(() => expect(h.pushSuccess).toHaveBeenCalledWith('Restored 1 mod', []));
  });

  it('a restore names what it skipped, and says so when the batch has expired', async () => {
    host();
    await uninstallMods(scope, [sodium]);
    h.modsRestoreUninstalled
      .mockResolvedValueOnce(
        ok({
          restored: [],
          skipped: [{ name: 'sodium.jar', reason: 'name_taken' }],
          expired: false,
        }),
      )
      .mockResolvedValueOnce(ok({ restored: [], skipped: [], expired: true }));
    toast(0)[2].run();
    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith("Couldn't restore", [
        "Couldn't restore sodium.jar: a file with that name already exists",
      ]),
    );
    toast(0)[2].run();
    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith(
        "Couldn't restore: the removed files are no longer kept",
        [],
      ),
    );
  });

  it('a restore that brought nothing back is never reported as a success', async () => {
    host();
    await uninstallMods(scope, [sodium]);
    h.modsRestoreUninstalled.mockResolvedValueOnce(
      ok({ restored: [], skipped: [], expired: false }),
    );
    toast(0)[2].run();
    await waitFor(() => expect(h.pushWarning).toHaveBeenCalledWith("Couldn't restore", []));
    expect(h.pushSuccess).not.toHaveBeenCalled();
  });

  it('a restore that stopped partway never claims nothing came back', async () => {
    host();
    await uninstallMods(scope, [sodium]);
    h.modsRestoreUninstalled.mockResolvedValueOnce(ioErr);
    toast(0)[2].run();
    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith("Couldn't restore everything", [
        expect.any(String),
      ]),
    );
  });

  it('a restore after a profile switch targets the original profile and names it', async () => {
    const { rerender } = host('inst');
    await uninstallMods(scope, [sodium]);
    await rerender({ activeInstanceId: 'other' });
    toast(0)[2].run();
    await waitFor(() =>
      expect(h.pushSuccess).toHaveBeenCalledWith('Restored 1 mod', ['Profile: Alpha Pack']),
    );
    expect(h.modsRestoreUninstalled).toHaveBeenCalledWith('inst', 'tok');
  });

  it('bulk: offers unneeded libraries only when there are some, removing all in one call', async () => {
    h.modsFindOrphans.mockResolvedValue(
      ok([{ sha1: 'c', name: 'Cloth Config', project_id: 'cc' }]),
    );
    h.modsUninstallMany.mockResolvedValue(
      ok({
        token: 't3',
        items: [
          { sha1: 's', name: 's' },
          { sha1: 'c', name: 'c' },
        ],
      }),
    );
    host();
    const done = uninstallMods(scope, [sodium], { offerOrphans: true });
    const dialog = await screen.findByRole('dialog', {
      name: 'Also remove libraries nothing else needs?',
    });
    await fireEvent.click(within(dialog).getByRole('checkbox'));
    await fireEvent.click(within(dialog).getByRole('button', { name: /uninstall 2 mods/i }));
    await expect(done).resolves.toBe('applied');
    expect(h.modsUninstallMany).toHaveBeenCalledWith('inst', ['s', 'c']);
    expect(toast(0)[3]).toEqual(['Sodium', 'Cloth Config']);
  });
});

// A row action has no busy state of its own: a double click on Remove fired two guarded calls, and
// the second one failed or asked about a mod that was already gone.
describe('a mod already being changed', () => {
  const removed = ok({ token: 'tok', items: [{ sha1: 's', name: 'sodium-jar' }] });

  it('a second click does nothing — one removal, one report — and the mod is free once it settles', async () => {
    const removing = deferred<unknown>();
    h.modsUninstall.mockReturnValueOnce(removing.promise);
    host();
    const first = uninstallMods(scope, [sodium]);
    await flush();
    expect(h.modsUninstall).toHaveBeenCalledTimes(1); // the removal is under way
    // A double click on Remove, and Disable on the same row, while it runs: nothing, silently.
    await expect(uninstallMods(scope, [sodium])).resolves.toBe('cancelled');
    await expect(disableMods(scope, [sodium])).resolves.toBe('cancelled');
    removing.resolve(removed);
    await expect(first).resolves.toBe('applied');
    expect(h.modsRemovalImpact).toHaveBeenCalledTimes(1);
    expect(h.modsUninstall).toHaveBeenCalledTimes(1);
    expect(h.modsDisable).not.toHaveBeenCalled();
    expect(h.pushWarning).not.toHaveBeenCalled();
    expect(h.pushActionToast).toHaveBeenCalledTimes(1); // one Undo notice
    expect(screen.queryByRole('dialog')).toBeNull();
    // Settled: the next request for it runs.
    await expect(disableMods(scope, [sodium])).resolves.toBe('applied');
    expect(h.modsDisable).toHaveBeenCalledWith('inst', 's');
  });

  it('another mod, or the same mod in another profile, is not held up', async () => {
    const removing = deferred<unknown>();
    h.modsUninstall.mockReturnValueOnce(removing.promise);
    host();
    const first = uninstallMods(scope, [sodium]);
    await flush();
    await expect(disableMods(scope, [{ sha1: 'x', name: 'Xaero' }])).resolves.toBe('applied');
    await expect(disableMods({ instanceId: 'other' }, [sodium])).resolves.toBe('applied');
    expect(h.modsDisable.mock.calls).toEqual([
      ['inst', 'x'],
      ['other', 's'],
    ]);
    removing.resolve(removed);
    await expect(first).resolves.toBe('applied');
  });

  it('a batch leaves out a mod already being changed and does the rest', async () => {
    const removing = deferred<unknown>();
    h.modsUninstall.mockReturnValueOnce(removing.promise);
    host();
    const first = uninstallMods(scope, [sodium]);
    await flush();
    const xaero = { sha1: 'x', name: 'Xaero' };
    await expect(disableMods(scope, [sodium, xaero], { bulk: true })).resolves.toBe('applied');
    expect(h.modsRemovalImpact).toHaveBeenLastCalledWith('inst', ['x']);
    expect(flipped(h.modsDisable)).toEqual(['x']);
    expect(h.pushSuccess).toHaveBeenCalledWith('Disabled 1 mod', []);
    removing.resolve(removed);
    await expect(first).resolves.toBe('applied');
  });
});

it('is mounted after </main> (above the pack drawer) and before the data-move host', () => {
  const src = readFileSync(resolve('src/routes/+page.svelte'), 'utf8');
  const at = src.indexOf('<ModOpsHost');
  expect(at).toBeGreaterThan(src.indexOf('</main>'));
  expect(at).toBeLessThan(src.indexOf('<DataMoveHost />'));
});
