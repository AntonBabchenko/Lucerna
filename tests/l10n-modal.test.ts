import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InstanceCoverage, NamespaceCoverage } from '$lib/ipc/bindings';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { l10nCoverage: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import LocalizationModal from '$lib/l10n/LocalizationModal.svelte';

function ns(over: Partial<NamespaceCoverage> = {}): NamespaceCoverage {
  return { namespace: 'create', totalKeys: 10, fromMod: 5, overridden: 0, ...over };
}

function coverage(over: Partial<InstanceCoverage> = {}): InstanceCoverage {
  return { lang: 'en_us', percent: 50, namespaces: [], availableCodes: ['en_us'], ...over };
}

function mockCoverageOk(data: InstanceCoverage) {
  vi.mocked(commands.l10nCoverage).mockResolvedValue({
    status: 'ok',
    data,
    // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
  } as any);
}

afterEach(() => {
  vi.clearAllMocks();
});

describe('LocalizationModal', () => {
  it('does not fetch while closed', () => {
    mockCoverageOk(coverage());
    render(LocalizationModal, { props: { open: false, instanceId: 'inst-1' } });
    expect(commands.l10nCoverage).not.toHaveBeenCalled();
  });

  it('does not fetch when no instance is selected', () => {
    mockCoverageOk(coverage());
    render(LocalizationModal, { props: { open: true, instanceId: null } });
    expect(commands.l10nCoverage).not.toHaveBeenCalled();
  });

  it('fetches with an empty lang on first open, letting the backend resolve it', async () => {
    mockCoverageOk(coverage({ lang: 'ru_ru', namespaces: [ns()] }));
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('inst-1', ''));
  });

  it('seeds the language picker from the backend-resolved language, without re-fetching', async () => {
    mockCoverageOk(coverage({ lang: 'ru_ru', availableCodes: ['en_us', 'ru_ru'] }));
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('inst-1', ''));

    const combo = await screen.findByRole('combobox');
    expect(combo.textContent).toContain('ru_ru');

    // Seeding the picker from the response must not itself trigger a second
    // fetch — give any errant reactive loop a chance to fire, then check the
    // call count stayed at one.
    await new Promise((r) => setTimeout(r, 0));
    expect(commands.l10nCoverage).toHaveBeenCalledTimes(1);
  });

  it('renders namespaces least-translated-first', async () => {
    mockCoverageOk(
      coverage({
        namespaces: [
          ns({ namespace: 'thermal', totalKeys: 10, fromMod: 10 }), // 100%
          ns({ namespace: 'create', totalKeys: 100, fromMod: 20 }), // 20%
          ns({ namespace: 'ae2', totalKeys: 50, fromMod: 0 }), // 0%
        ],
      }),
    );
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1' } });
    const rows = await screen.findAllByTestId('l10n-namespace-row');
    expect(rows[0].textContent).toContain('ae2');
    expect(rows[1].textContent).toContain('create');
    expect(rows[2].textContent).toContain('thermal');
  });

  it('shows the empty state when the instance has no translatable namespaces', async () => {
    mockCoverageOk(coverage({ namespaces: [] }));
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1' } });
    expect(await screen.findByTestId('l10n-empty')).toBeTruthy();
  });

  it('surfaces a read failure instead of pretending the instance has nothing to translate', async () => {
    vi.mocked(commands.l10nCoverage).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'p', details: 'nope' },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1' } });
    expect(await screen.findByTestId('l10n-error')).toBeTruthy();
    expect(screen.queryByTestId('l10n-empty')).toBeNull();
  });

  it('refetches when the instance prop changes', async () => {
    mockCoverageOk(coverage());
    const { rerender } = render(LocalizationModal, { props: { open: true, instanceId: 'a' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', ''));

    await rerender({ open: true, instanceId: 'b' });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('b', ''));
  });

  it('refetches with the newly picked target language', async () => {
    mockCoverageOk(coverage({ lang: 'en_us', availableCodes: ['en_us', 'ru_ru'] }));
    render(LocalizationModal, { props: { open: true, instanceId: 'a' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', ''));

    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: 'ru_ru' }));

    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', 'ru_ru'));
  });

  it('ignores a late response for an instance the user has since switched away from', async () => {
    let resolveA!: (v: Awaited<ReturnType<typeof commands.l10nCoverage>>) => void;
    vi.mocked(commands.l10nCoverage).mockImplementationOnce(
      () =>
        new Promise((r) => {
          resolveA = r;
        }),
    );

    const { rerender } = render(LocalizationModal, { props: { open: true, instanceId: 'a' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', ''));

    // Switch to instance b before a's response arrives; b resolves immediately.
    mockCoverageOk(coverage({ namespaces: [ns({ namespace: 'b-namespace' })] }));
    await rerender({ open: true, instanceId: 'b' });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('b', ''));
    expect(await screen.findAllByTestId('l10n-namespace-row')).toHaveLength(1);

    // The stale A response now lands — it must not clobber b's already-newer state.
    resolveA({ status: 'ok', data: coverage({ namespaces: [ns({ namespace: 'a-namespace' })] }) });

    // waitFor polls rather than checking once: a fixed number of microtask
    // ticks isn't a reliable stand-in for "Svelte has flushed the DOM", and a
    // false pass here would defeat the point of the test.
    await waitFor(() => {
      const rows = screen.getAllByTestId('l10n-namespace-row');
      expect(rows).toHaveLength(1);
      expect(rows[0].textContent).toContain('b-namespace');
    });
    // Give the stale response every chance to land wrong before declaring success.
    await new Promise((r) => setTimeout(r, 20));
    const rows = screen.getAllByTestId('l10n-namespace-row');
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toContain('b-namespace');
  });

  it('does not double-fetch when switching instance right after an explicit pick', async () => {
    // Regression coverage for a reactive-loop trap: resetting the pending
    // language pick on instance switch must not itself look like a second
    // "user changed the language" trigger.
    mockCoverageOk(coverage({ lang: 'en_us', availableCodes: ['en_us', 'ru_ru'] }));
    const { rerender } = render(LocalizationModal, { props: { open: true, instanceId: 'a' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', ''));

    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: 'ru_ru' }));
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', 'ru_ru'));

    vi.mocked(commands.l10nCoverage).mockClear();
    mockCoverageOk(coverage({ lang: 'en_us', availableCodes: ['en_us', 'ru_ru'] }));
    await rerender({ open: true, instanceId: 'b' });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('b', ''));

    // Give an errant extra reactive pass every chance to fire before checking.
    await new Promise((r) => setTimeout(r, 30));
    expect(commands.l10nCoverage).toHaveBeenCalledTimes(1);
  });

  it('closes via the close button', async () => {
    mockCoverageOk(coverage());
    render(LocalizationModal, { props: { open: true, instanceId: 'a' } });
    await screen.findByRole('dialog');
    await fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
