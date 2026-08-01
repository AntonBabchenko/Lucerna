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

// Real `default_target_code` is idempotent for a full code (ru_ru -> ru_ru),
// so the real backend always echoes back whatever full code it was given.
// `mockCoverageOk`'s fixed `lang` doesn't model that — it would make the
// write-back logic in LocalizationModal look like it "reverts" a user's
// pick to whatever the fixture happened to hardcode. Use this instead
// whenever a test picks a language and needs a realistic response.
function mockCoverageEcho(availableCodes: string[]) {
  vi.mocked(commands.l10nCoverage).mockImplementation(
    async (_id: string, lang: string) =>
      ({ status: 'ok', data: coverage({ lang, availableCodes }) }) as Awaited<
        ReturnType<typeof commands.l10nCoverage>
      >,
  );
}

afterEach(() => {
  vi.clearAllMocks();
});

describe('LocalizationModal', () => {
  it('does not fetch while closed', () => {
    mockCoverageOk(coverage());
    render(LocalizationModal, { props: { open: false, instanceId: 'inst-1', lang: 'en_us' } });
    expect(commands.l10nCoverage).not.toHaveBeenCalled();
  });

  it('does not fetch when no instance is selected', () => {
    mockCoverageOk(coverage());
    render(LocalizationModal, { props: { open: true, instanceId: null, lang: 'en_us' } });
    expect(commands.l10nCoverage).not.toHaveBeenCalled();
  });

  // The modal no longer owns the target language (see +page.svelte's
  // l10nLang) — it's handed a value by the caller and fetches with it
  // directly. An empty-string special case belongs to the backend only.
  it('fetches with the given lang on first open', async () => {
    mockCoverageOk(coverage({ lang: 'ru_ru', namespaces: [ns()] }));
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1', lang: 'ru_ru' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('inst-1', 'ru_ru'));
  });

  it('shows the given language pre-selected in the picker', async () => {
    mockCoverageEcho(['en_us', 'ru_ru']);
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1', lang: 'ru_ru' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('inst-1', 'ru_ru'));

    const combo = await screen.findByRole('combobox');
    expect(combo.textContent).toContain('ru_ru');

    // The response's resolved language matches what was requested, so there
    // is nothing to write back — give an errant extra fetch every chance to
    // fire before checking the call count stayed at one.
    await new Promise((r) => setTimeout(r, 0));
    expect(commands.l10nCoverage).toHaveBeenCalledTimes(1);
  });

  // Regression coverage for the shared-state fix: +page.svelte seeds `lang`
  // with the launcher's resolved UI locale, which can be a bare code like
  // "ru" (see resolve.ts — AVAILABLE_LOCALES only has 2-letter codes). The
  // backend resolves that to a full Minecraft code and returns it in
  // `coverage.lang`; the modal must write it back through the bindable prop
  // so its own picker (and, via the same binding, the Overview row) settle
  // on the full code instead of being stuck on the bare guess.
  it('writes the backend-resolved language back through the bindable prop', async () => {
    mockCoverageOk(coverage({ lang: 'ru_ru', availableCodes: ['en_us', 'ru_ru'] }));
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1', lang: 'ru' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('inst-1', 'ru'));

    // The write-back changes `lang`, which the fetch effect depends on, so
    // a second — cheap, cache-hit — request follows for the resolved code.
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('inst-1', 'ru_ru'));
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledTimes(2));

    const combo = await screen.findByRole('combobox');
    expect(combo.textContent).toContain('ru_ru');
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
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1', lang: 'en_us' } });
    const rows = await screen.findAllByTestId('l10n-namespace-row');
    expect(rows[0].textContent).toContain('ae2');
    expect(rows[1].textContent).toContain('create');
    expect(rows[2].textContent).toContain('thermal');
  });

  it('shows the empty state when the instance has no translatable namespaces', async () => {
    mockCoverageOk(coverage({ namespaces: [] }));
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1', lang: 'en_us' } });
    expect(await screen.findByTestId('l10n-empty')).toBeTruthy();
  });

  it('surfaces a read failure instead of pretending the instance has nothing to translate', async () => {
    vi.mocked(commands.l10nCoverage).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'p', details: 'nope' },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(LocalizationModal, { props: { open: true, instanceId: 'inst-1', lang: 'en_us' } });
    expect(await screen.findByTestId('l10n-error')).toBeTruthy();
    expect(screen.queryByTestId('l10n-empty')).toBeNull();
  });

  it('refetches when the instance prop changes', async () => {
    mockCoverageOk(coverage());
    const { rerender } = render(LocalizationModal, {
      props: { open: true, instanceId: 'a', lang: 'en_us' },
    });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', 'en_us'));

    await rerender({ open: true, instanceId: 'b', lang: 'en_us' });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('b', 'en_us'));
  });

  it('refetches with the newly picked target language', async () => {
    mockCoverageEcho(['en_us', 'ru_ru']);
    render(LocalizationModal, { props: { open: true, instanceId: 'a', lang: 'en_us' } });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', 'en_us'));

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

    const { rerender } = render(LocalizationModal, {
      props: { open: true, instanceId: 'a', lang: 'en_us' },
    });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', 'en_us'));

    // Switch to instance b before a's response arrives; b resolves immediately.
    mockCoverageOk(coverage({ namespaces: [ns({ namespace: 'b-namespace' })] }));
    await rerender({ open: true, instanceId: 'b', lang: 'en_us' });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('b', 'en_us'));
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

  // Regression coverage for the shared-state redesign: the target language
  // is now a page-level selection (+page.svelte's l10nLang), not something
  // this modal resets per instance. An explicit pick must survive an
  // instance switch instead of quietly reverting.
  it('keeps an explicitly picked language when switching instances', async () => {
    mockCoverageEcho(['en_us', 'ru_ru']);
    const { rerender } = render(LocalizationModal, {
      props: { open: true, instanceId: 'a', lang: 'en_us' },
    });
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', 'en_us'));

    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: 'ru_ru' }));
    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('a', 'ru_ru'));

    vi.mocked(commands.l10nCoverage).mockClear();
    mockCoverageEcho(['en_us', 'ru_ru']);
    // The caller (here: the test) owns `lang` for a bindable prop with no
    // real parent binding — mirror the picked value forward exactly like
    // +page.svelte's bind:lang would, so the instance switch doesn't
    // silently revert the pick.
    await rerender({ open: true, instanceId: 'b', lang: 'ru_ru' });

    await waitFor(() => expect(commands.l10nCoverage).toHaveBeenCalledWith('b', 'ru_ru'));
    expect(commands.l10nCoverage).not.toHaveBeenCalledWith('b', 'en_us');
  });

  it('closes via the close button', async () => {
    mockCoverageOk(coverage());
    render(LocalizationModal, { props: { open: true, instanceId: 'a', lang: 'en_us' } });
    await screen.findByRole('dialog');
    await fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
