// The AI pre-fill's user-facing surface: the two trigger buttons in
// LocalizationModal (both gated) and the estimate → progress → summary dialog
// they open.
//
// What is worth pinning here is honesty, not layout: a run that spent nothing
// measurable must show no cost rather than a zero, a run whose strings landed
// but whose pack rebuild failed must read as "done, with a caveat" rather than
// as a success or a failure, and a local model must say up front that more
// strings will stay English.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  ApplyGate,
  InstanceCoverage,
  NamespaceCoverage,
  PrefillEstimate,
  RunSummary,
} from '$lib/ipc/bindings';
import type { PrefillReadiness } from '$lib/l10n/prefill-readiness';

// Every Channel the runner constructs, in construction order. `vi.hoisted` is
// required: the `vi.mock` factory below is lifted above every `const`.
const { channels } = vi.hoisted(() => ({
  channels: [] as { onmessage: ((m: unknown) => void) | null }[],
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    l10nCoverage: vi.fn(),
    l10nNamespaceKeys: vi.fn(),
    l10nSetOverride: vi.fn(),
    l10nApply: vi.fn(),
    l10nPrefillEstimate: vi.fn(),
    l10nPrefillStart: vi.fn(),
    l10nPrefillCancel: vi.fn(),
    l10nRevertMachine: vi.fn(),
    // A finished run now opens the apply-elsewhere offer, which awaits this
    // command inside its own onMount and reads `.status` off the result. A
    // bare vi.fn() resolves undefined, and that read becomes an unhandled
    // rejection which fails the run while every assertion still passes — the
    // worst kind of red. An empty list is what production does with nothing
    // to offer: the dialog finds nothing actionable and closes itself.
    l10nApplyTargets: vi.fn(async () => ({ status: 'ok', data: [] })),
  },
}));

// prefill-runner does `new Channel<PrefillProgress>()` and then assigns
// `.onmessage` — `Channel: vi.fn()` would not survive that assignment (see
// tests/export-pack-dialog.test.ts). This stub also records each instance so a
// test can push progress ticks exactly the way the backend would.
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
    constructor() {
      channels.push(this as { onmessage: ((m: unknown) => void) | null });
    }
  },
}));

import { locale } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
// Static imports: a dynamic `await import('*.svelte')` inside a test body
// never resolves under this vitest setup.
import LocalizationModal from '$lib/l10n/LocalizationModal.svelte';
import PrefillDialog from '$lib/l10n/PrefillDialog.svelte';
import { markSeen } from '$lib/onboarding/contextual-tours';

function ns(over: Partial<NamespaceCoverage> = {}): NamespaceCoverage {
  return { namespace: 'create', totalKeys: 10, fromMod: 5, overridden: 0, ...over };
}

function coverage(over: Partial<InstanceCoverage> = {}): InstanceCoverage {
  return {
    lang: 'en_us',
    percent: 50,
    namespaces: [ns()],
    availableCodes: ['en_us'],
    applyGate: 'ready',
    packState: 'not_applied',
    ...over,
  };
}

function estimate(over: Partial<PrefillEstimate> = {}): PrefillEstimate {
  return {
    missing: 120,
    fromCache: 20,
    fromGlossary: 10,
    toTranslate: 90,
    estimatedTokens: 4500,
    provider: 'anthropic',
    billable: true,
    ...over,
  };
}

function summary(over: Partial<RunSummary> = {}): RunSummary {
  return {
    written: 42,
    fromCache: 7,
    fromGlossary: 3,
    rejected: 2,
    cancelled: false,
    promptTokens: 1200,
    completionTokens: 800,
    usageKnown: true,
    packRebuilt: true,
    packActivated: true,
    packRebuildError: null,
    unusableBatches: 0,
    failed: null,
    ...over,
  };
}

// biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
const ok = (data: unknown) => ({ status: 'ok', data }) as any;

function mockCoverage(data: InstanceCoverage) {
  vi.mocked(commands.l10nCoverage).mockResolvedValue(ok(data));
}

function dialogProps(over: Record<string, unknown> = {}) {
  return {
    instanceId: 'inst-1',
    lang: 'ru_ru',
    namespace: null,
    onClose: vi.fn(),
    ...over,
  };
}

beforeEach(() => {
  locale.set('en');
  channels.length = 0;
  // The trigger cases below render the FULL LocalizationModal, which hosts the
  // `l10n` contextual tour. It auto-fires on first open, so seed it seen — a
  // live spotlight would sit over the very buttons these tests click.
  markSeen('l10n');
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('LocalizationModal — prefill triggers', () => {
  // Every state below renders BOTH triggers. A button that vanishes teaches a
  // user the feature does not exist; one that is visible and greyed teaches
  // them it exists and what to go and fix. The reason has to be reachable, so
  // each case pins the inline copy in the header as well — the tooltip is
  // hover-only by construction and a keyboard user never reaches it.
  const gatedStates: { aiReady: PrefillReadiness; gate: ApplyGate; reason: RegExp }[] = [
    { aiReady: 'no_consent', gate: 'ready', reason: /turned off in Settings/i },
    { aiReady: 'no_key', gate: 'ready', reason: /Add an API key/i },
    // The version gate is not fixable in Settings, but it is still a reason —
    // and the pre-fill borrows Apply's wording rather than inventing a second
    // vocabulary for the same fact about the same instance.
    { aiReady: 'ready', gate: 'too_old', reason: /can't load resource-pack overrides/i },
  ];

  for (const state of gatedStates) {
    it(`keeps both translate buttons visible and disabled — ${state.aiReady}/${state.gate}`, async () => {
      mockCoverage(coverage({ applyGate: state.gate }));
      render(LocalizationModal, {
        props: { open: true, instanceId: 'a', lang: 'en_us', aiReady: state.aiReady },
      });
      // Wait for coverage, otherwise "present" would be measured before the
      // gate that is supposed to disable it has even been read.
      await screen.findByTestId('l10n-namespace-row');

      const all = screen.getByTestId('l10n-prefill-all') as HTMLButtonElement;
      const perRow = screen.getAllByTestId('l10n-prefill-namespace') as HTMLButtonElement[];
      expect(all.disabled).toBe(true);
      expect(perRow.length).toBeGreaterThan(0);
      for (const b of perRow) expect(b.disabled).toBe(true);
      expect(screen.getByTestId('l10n-prefill-reason').textContent).toMatch(state.reason);
    });
  }

  it('enables both translate buttons once nothing is missing', async () => {
    // The control render for the three cases above: without it they would all
    // pass against a component that renders the buttons permanently dead.
    mockCoverage(coverage({ applyGate: 'ready' }));
    render(LocalizationModal, {
      props: { open: true, instanceId: 'b', lang: 'en_us', aiReady: 'ready' },
    });
    await screen.findByTestId('l10n-namespace-row');

    expect((screen.getByTestId('l10n-prefill-all') as HTMLButtonElement).disabled).toBe(false);
    for (const b of screen.getAllByTestId('l10n-prefill-namespace') as HTMLButtonElement[]) {
      expect(b.disabled).toBe(false);
    }
    // No reason to give, so no line taking up header space.
    expect(screen.queryByTestId('l10n-prefill-reason')).toBeNull();
  });

  it('refreshes the open key table when a run finishes, not just the coverage', async () => {
    // A finished run rewrites the very rows the user is looking at. KeyTable
    // reloads on (instanceId, namespace, lang) and a run changes none of them,
    // so without an explicit nudge the table keeps showing pre-run values until
    // the modal is reopened — the strings are on disk and invisible.
    mockCoverage(coverage({ applyGate: 'ready' }));
    vi.mocked(commands.l10nNamespaceKeys).mockResolvedValue(ok([]));
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(ok(summary()));

    render(LocalizationModal, {
      props: { open: true, instanceId: 'inst-1', lang: 'ru_ru', aiReady: 'ready' },
    });

    await fireEvent.click(await screen.findByTestId('l10n-namespace-row'));
    await waitFor(() => expect(commands.l10nNamespaceKeys).toHaveBeenCalledTimes(1));

    await fireEvent.click(screen.getByTestId('l10n-prefill-namespace'));
    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    await screen.findByTestId('l10n-prefill-summary');
    await waitFor(() => expect(commands.l10nNamespaceKeys).toHaveBeenCalledTimes(2));
  });
});

describe('PrefillDialog', () => {
  it('shows the estimate and does not start until confirmed', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    expect(screen.getByTestId('l10n-prefill-missing').textContent).toContain('120');
    expect(screen.getByTestId('l10n-prefill-to-translate').textContent).toContain('90');
    expect(screen.getByTestId('l10n-prefill-from-cache').textContent).toContain('20');
    // Showing an estimate must never be the same act as paying for the run.
    expect(commands.l10nPrefillStart).not.toHaveBeenCalled();

    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(ok(summary()));
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    await waitFor(() =>
      expect(commands.l10nPrefillStart).toHaveBeenCalledWith(
        'inst-1',
        'ru_ru',
        null,
        expect.anything(),
      ),
    );
  });

  it('renders progress as done of total', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    // Never resolves: the run must be observable while it is still in flight.
    vi.mocked(commands.l10nPrefillStart).mockReturnValue(new Promise(() => {}));
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    await waitFor(() => expect(channels).toHaveLength(1));
    channels[0].onmessage?.({ done: 3, total: 10, phase: 'name' });

    await waitFor(() => {
      const el = screen.getByTestId('l10n-prefill-progress');
      expect(el.textContent).toContain('3');
      expect(el.textContent).toContain('10');
    });
  });

  it('says so when batches came back unreadable and were skipped', async () => {
    // Losing a batch instead of the whole run is only acceptable BECAUSE it
    // is reported; otherwise coverage just refuses to move with no reason.
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(ok(summary({ unusableBatches: 3 })));
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    await screen.findByTestId('l10n-prefill-summary');
    expect(screen.getByTestId('l10n-prefill-unusable').textContent).toContain('3');
  });

  it('says so when the pack was built but is not switched on', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(
      ok(summary({ packRebuilt: true, packActivated: false })),
    );
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    await screen.findByTestId('l10n-prefill-summary');
    expect(screen.getByTestId('l10n-prefill-pack-not-on')).toBeTruthy();
  });

  it('reports written, cached and rejected counts in the summary', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(
      ok(summary({ written: 42, fromCache: 7, rejected: 2 })),
    );
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    await screen.findByTestId('l10n-prefill-summary');
    expect(screen.getByTestId('l10n-prefill-written').textContent).toContain('42');
    expect(screen.getByTestId('l10n-prefill-cached').textContent).toContain('7');
    expect(screen.getByTestId('l10n-prefill-rejected').textContent).toContain('2');
  });

  it('omits cost entirely when usage is unknown', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(
      ok(summary({ usageKnown: false, promptTokens: 0, completionTokens: 0 })),
    );
    const unknown = render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));
    await screen.findByTestId('l10n-prefill-summary');

    // A run that cost nothing and a run whose cost is unknown are different
    // claims — a zero here would make the second look like the first.
    expect(screen.queryByTestId('l10n-prefill-cost')).toBeNull();
    // The rest of the summary still has to be there.
    expect(screen.getByTestId('l10n-prefill-written').textContent).toContain('42');
    unknown.unmount();

    // Control: with usage reported, the cost IS shown — otherwise this test
    // would pass against a dialog that never renders a cost at all.
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(
      ok(summary({ usageKnown: true, promptTokens: 1200, completionTokens: 800 })),
    );
    render(PrefillDialog, { props: dialogProps() });
    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));
    await screen.findByTestId('l10n-prefill-summary');
    expect((await screen.findByTestId('l10n-prefill-cost')).textContent).toContain('1200');
  });

  it('warns that more strings stay English on a local model', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(
      ok(estimate({ provider: 'local', billable: false })),
    );
    const local = render(PrefillDialog, { props: dialogProps() });

    const note = await screen.findByTestId('l10n-prefill-local-note');
    expect(note.textContent).toMatch(/english/i);
    local.unmount();

    // Control: a cloud provider does not carry the local caveat.
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    render(PrefillDialog, { props: dialogProps() });
    await screen.findByTestId('l10n-prefill-estimate');
    expect(screen.queryByTestId('l10n-prefill-local-note')).toBeNull();
  });

  it('reports a written run whose pack rebuild failed as done-with-a-caveat', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(
      ok(
        summary({
          written: 12,
          packRebuilt: false,
          packRebuildError: 'instance is running',
        }),
      ),
    );
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    // Done, not failed: the summary is shown and the strings are counted...
    await screen.findByTestId('l10n-prefill-summary');
    expect(screen.getByTestId('l10n-prefill-written').textContent).toContain('12');
    expect(screen.queryByTestId('l10n-prefill-error')).toBeNull();
    // ...with the caveat that they are on disk but not in the pack yet.
    const warning = screen.getByTestId('l10n-prefill-pack-warning');
    expect(warning.textContent).toContain('instance is running');
  });

  it('surfaces a run that stopped part-way alongside what it did buy', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockResolvedValue(
      ok(summary({ written: 30, failed: 'provider rejected the key' })),
    );
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));

    await screen.findByTestId('l10n-prefill-summary');
    expect(screen.getByTestId('l10n-prefill-written').textContent).toContain('30');
    expect(screen.getByTestId('l10n-prefill-failed').textContent).toContain(
      'provider rejected the key',
    );
  });

  it('asks the backend to stop when the user cancels a run in flight', async () => {
    vi.mocked(commands.l10nPrefillEstimate).mockResolvedValue(ok(estimate()));
    vi.mocked(commands.l10nPrefillStart).mockReturnValue(new Promise(() => {}));
    vi.mocked(commands.l10nPrefillCancel).mockResolvedValue(ok(null));
    render(PrefillDialog, { props: dialogProps() });

    await screen.findByTestId('l10n-prefill-estimate');
    await fireEvent.click(screen.getByTestId('l10n-prefill-start'));
    await screen.findByTestId('l10n-prefill-progress');

    await fireEvent.click(screen.getByTestId('l10n-prefill-stop'));
    await waitFor(() => expect(commands.l10nPrefillCancel).toHaveBeenCalledWith('inst-1'));
  });
});
